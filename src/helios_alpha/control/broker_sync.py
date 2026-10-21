"""Read-only Robinhood MCP account synchronization, with a separate OAuth grant.

No model calls, orders, previews, or trading permissions are exposed by this worker.
Only the existing observation importer writes account data; Golem checks its role.
"""

from __future__ import annotations

import argparse
import asyncio
import fcntl
import hashlib
import json
import logging
import os
import secrets
import signal
import tempfile
import time
from pathlib import Path
from urllib.parse import parse_qs, urlparse

import httpx
from mcp import ClientSession
from mcp.client.auth import OAuthClientProvider
from mcp.client.streamable_http import streamable_http_client
from mcp.shared.auth import OAuthClientInformationFull, OAuthClientMetadata, OAuthToken

from .broker_snapshot import _load_observation, import_broker_observation
from .golem_store import GolemControlStore, read_private_json
from .store import DEFAULT_ROOT, ControlError, canonical, private_root
from .strategy_observation import account_permissions, project

SERVER = "https://agent.robinhood.com/mcp/trading"
READ_TOOLS = frozenset(
    {
        "get_accounts",
        "get_portfolio",
        "get_trade_approval_setting",
        "get_crypto_positions",
        "get_crypto_orders",
        "get_crypto_quotes",
    }
)


def now_ms():
    return time.time_ns() // 1_000_000


def write_private(path, data):
    root = private_root(path.parent)
    fd, temporary = tempfile.mkstemp(prefix=".broker-sync-", dir=root)
    try:
        with os.fdopen(fd, "w") as handle:
            os.fchmod(handle.fileno(), 0o600)
            handle.write(canonical(data))
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary, path)
        directory = os.open(root, os.O_RDONLY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


class PrivateOAuthStorage:
    def __init__(self, root):
        self.root = root

    async def get_tokens(self):
        path = self.root / "broker-sync-oauth-tokens.json"
        if not path.exists():
            return None
        data = read_private_json(path)
        token = OAuthToken.model_validate(data["token"])
        if token.expires_in is not None:
            token.expires_in = max(
                0, token.expires_in - max(0, now_ms() - data["saved_ms"]) // 1000
            )
        return token

    async def set_tokens(self, tokens):
        write_private(
            self.root / "broker-sync-oauth-tokens.json",
            {"saved_ms": now_ms(), "token": tokens.model_dump(mode="json")},
        )

    async def get_client_info(self):
        path = self.root / "broker-sync-oauth-client.json"
        return (
            OAuthClientInformationFull.model_validate(read_private_json(path))
            if path.exists()
            else None
        )

    async def set_client_info(self, client_info):
        write_private(
            self.root / "broker-sync-oauth-client.json", client_info.model_dump(mode="json")
        )


def account_key(account):
    return hashlib.sha256(
        f"robinhood\n{account['account_number']}\n{account['rhs_account_number']}\n{account['rhc_account_number']}".encode()
    ).hexdigest()


def select_account(accounts, expected_key):
    eligible = [
        a
        for a in accounts
        if a.get("agentic_allowed") is True
        and a.get("rhc_account_number")
        and a.get("state") == "active"
        and a.get("deactivated") is False
        and a.get("permanently_deactivated") is False
    ]
    if expected_key:
        eligible = [a for a in eligible if account_key(a) == expected_key]
    if len(eligible) != 1:
        raise ControlError("select exactly one matching active agentic account")
    return eligible[0]


def response_data(result):
    if result.isError:
        raise ControlError("broker read failed")
    payload = result.structuredContent
    if payload is None:
        texts = [item.text for item in result.content if item.type == "text"]
        if len(texts) != 1:
            raise ControlError("unexpected broker response")
        payload = json.loads(texts[0])
    if not isinstance(payload, dict) or not isinstance(payload.get("data"), dict):
        raise ControlError("unexpected broker response")
    return payload["data"]


async def collect(session, expected_key):
    async def call(tool, arguments):
        if tool not in READ_TOOLS:
            raise ControlError("only account read tools are permitted")
        return response_data(await session.call_tool(tool, arguments))

    account = select_account((await call("get_accounts", {}))["accounts"], expected_key)
    result = {
        "schema_version": 1,
        "account": account,
        "account_received_ms": now_ms(),
        "calls": {},
        "previews": [],
    }
    brokerage, numeric = account["account_number"], account["rhs_account_number"]

    async def read(name, tool, arguments, identity):
        data = await call(tool, arguments)
        result["calls"][name] = {
            "requested_account": identity,
            "received_ms": now_ms(),
            "data": data,
        }
        return data

    # Bounded concurrency; each successful cycle is published atomically only
    # after all account identities, complete collections, and clocks validate.
    _, _, positions, _ = await asyncio.gather(
        read("portfolio", "get_portfolio", {"account_number": brokerage}, brokerage),
        read("approvals", "get_trade_approval_setting", {"account_number": brokerage}, brokerage),
        read("positions", "get_crypto_positions", {"rhs_account_number": numeric}, numeric),
        read(
            "open_orders",
            "get_crypto_orders",
            {"rhs_account_number": numeric, "state_group": "open"},
            numeric,
        ),
    )
    symbols = sorted({p["currency"]["code"] + "-USD" for p in positions["results"]})
    if len(symbols) > 100:
        raise ControlError("too many holdings")
    quotes = []
    for offset in range(0, len(symbols), 20):
        data = await call(
            "get_crypto_quotes",
            {"rhs_account_number": numeric, "symbols": symbols[offset : offset + 20]},
        )
        if data.get("next"):
            raise ControlError("incomplete quotes")
        quotes.extend(data["results"])
    result["calls"]["quotes"] = {
        "requested_account": numeric,
        "received_ms": now_ms(),
        "data": {"results": quotes},
    }
    return result


class SyncWorker:
    def __init__(self, root, client, interval, callback_port):
        self.root, self.client = private_root(root), client
        self.interval, self.callback_port = interval, callback_port
        self.storage = PrivateOAuthStorage(self.root)
        self.store = GolemControlStore(self.root)
        self.state = {
            "state": "authorization_required",
            "interval_ms": interval * 1000,
            "last_success_ms": None,
            "last_attempt_ms": None,
            "next_poll_ms": None,
            "authorization_url": None,
            "execution_authority": False,
        }
        self.pending_state = None
        self.callback = None
        self.interactive = False
        self.last_request = None
        self.task = None
        self.failures = 0

    def publish(self, **changes):
        self.state.update(changes)
        write_private(
            self.root / "broker-sync-status.json", {**self.state, "heartbeat_ms": now_ms()}
        )

    async def redirect(self, url):
        if not self.interactive:
            raise ControlError("authorization required")
        parsed = urlparse(url)
        state = parse_qs(parsed.query).get("state", [None])[0]
        if (
            parsed.scheme != "https"
            or not (
                parsed.hostname == "robinhood.com"
                or (parsed.hostname or "").endswith(".robinhood.com")
            )
            or not state
        ):
            raise ControlError("unexpected authorization destination")
        self.pending_state = state
        self.callback = asyncio.get_running_loop().create_future()
        self.publish(state="authorizing", authorization_url=url)

    async def wait_callback(self):
        return await asyncio.wait_for(self.callback, timeout=300)

    async def accept_callback(self, reader, writer):
        code = 400
        try:
            request = await asyncio.wait_for(reader.readuntil(b"\r\n\r\n"), timeout=5)
            first = request.split(b"\r\n", 1)[0].decode("ascii")
            method, target, _ = first.split(" ", 2)
            url, query = urlparse(target), parse_qs(urlparse(target).query)
            supplied_state = query.get("state", [""])[0]
            issuer = query.get("iss", [SERVER])[0]
            auth_code = query.get("code", [""])[0]
            if (
                method == "GET"
                and url.path == "/callback"
                and self.pending_state
                and secrets.compare_digest(supplied_state, self.pending_state)
                and issuer.rstrip("/") == SERVER
                and 0 < len(auth_code) <= 8192
                and self.callback is not None
                and not self.callback.done()
            ):
                self.callback.set_result((auth_code, supplied_state))
                self.pending_state = None
                self.publish(state="syncing", authorization_url=None)
                code = 200
        except (
            ValueError,
            UnicodeError,
            TimeoutError,
            asyncio.IncompleteReadError,
            asyncio.LimitOverrunError,
        ):
            pass
        message = (
            "Authorization received. You can return to Helios; account sync is starting."
            if code == 200
            else "Authorization could not be accepted. Start a new connection from Helios."
        )
        body = message.encode()
        writer.write(
            (
                f"HTTP/1.1 {code} {'OK' if code == 200 else 'Bad Request'}\r\n"
                "Content-Type: text/plain; charset=utf-8\r\nCache-Control: no-store\r\n"
                "Referrer-Policy: no-referrer\r\n"
                f"Content-Length: {len(body)}\r\nConnection: close\r\n\r\n"
            ).encode()
            + body
        )
        await writer.drain()
        writer.close()
        await writer.wait_closed()

    async def sync(self, interactive=False):
        self.interactive = interactive
        self.publish(
            state="syncing", last_attempt_ms=now_ms(), authorization_url=None, next_poll_ms=None
        )
        try:
            # Verify the configured import role before requesting broker access.
            settings = await asyncio.to_thread(self.store.settings, self.client)
            if "risk:write" not in settings["client"]["capabilities"]:
                raise ControlError("observation import permission required")
            previous = self.root / "broker-observation.json"
            expected = _load_observation(previous)["account_key"] if previous.exists() else None
            auth = OAuthClientProvider(
                SERVER,
                OAuthClientMetadata(
                    client_name="Helios account monitor",
                    redirect_uris=[f"http://127.0.0.1:{self.callback_port}/callback"],
                    token_endpoint_auth_method="none",
                    grant_types=["authorization_code", "refresh_token"],
                ),
                self.storage,
                self.redirect,
                self.wait_callback,
            )
            async with (
                asyncio.timeout(360 if interactive else 60),
                httpx.AsyncClient(auth=auth, timeout=30, follow_redirects=True) as http,
            ):
                async with streamable_http_client(SERVER, http_client=http) as (receive, send, _):
                    async with ClientSession(receive, send) as session:
                        await session.initialize()
                        raw = await collect(session, expected)
                        await asyncio.to_thread(
                            import_broker_observation, self.store, self.client, raw
                        )
                        self.failures = 0
                        self.publish(
                            state="connected",
                            last_success_ms=now_ms(),
                            next_poll_ms=now_ms() + self.interval * 1000,
                            account_permissions=account_permissions(raw),
                        )
                        # Optional research reads share this grant, never its account payload.
                        # A failed option feed must not invalidate a successful account read.
                        if (self.root / "natgas-config.json").exists():
                            from helios_alpha.natgas.broker import capture

                            try:
                                async with asyncio.timeout(15):
                                    await capture(session, self.root, now_ms)
                            except Exception:
                                from helios_alpha.natgas.lab import Journal

                                journal = Journal(self.root)
                                try:
                                    journal.health("robinhood-options", "unavailable", now_ms())
                                finally:
                                    journal.close()
        except asyncio.CancelledError:
            raise
        except Exception:
            # Provider exception strings may contain URLs or OAuth credentials.
            # Surface a fixed state only; never log response bodies or tokens.
            self.failures += 1
            needs_auth = self.pending_state is not None or not await self.storage.get_tokens()
            self.publish(
                state="authorization_required" if needs_auth else "degraded",
                next_poll_ms=None
                if needs_auth
                else now_ms() + min(300, self.interval * 2 ** min(self.failures, 4)) * 1000,
            )
        finally:
            self.pending_state = None
            self.callback = None
            self.interactive = False
            self.publish(authorization_url=None)

    async def inspect_strategy(self):
        # Independent of broker polling: a slow broker cannot freeze the book monitor.
        # Read Golem itself; a local settings export never supplies authority.
        while True:
            try:
                status = await asyncio.to_thread(self.store.status, self.client)
                self.publish(strategy=project(status, now_ms()))
            except asyncio.CancelledError:
                raise
            except Exception:
                self.publish(strategy=None)
            await asyncio.sleep(5)

    async def run(self):
        task = asyncio.current_task()
        asyncio.get_running_loop().add_signal_handler(signal.SIGTERM, task.cancel)
        lock = os.open(
            self.root / "broker-sync.lock", os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600
        )
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        # A fixed loopback callback keeps the registered redirect URI valid on restart.
        server = await asyncio.start_server(
            self.accept_callback, "127.0.0.1", self.callback_port, limit=16384
        )
        strategy_task = asyncio.create_task(self.inspect_strategy())
        try:
            if await self.storage.get_tokens():
                self.task = asyncio.create_task(self.sync())
            while True:
                path = self.root / "broker-sync-request.json"
                action = None
                if path.exists():
                    request = read_private_json(path)
                    if request.get("id") != self.last_request:
                        self.last_request = request.get("id")
                        if 0 <= now_ms() - request.get("requested_ms", 0) <= 30_000:
                            action = request.get("action")
                idle = self.task is None or self.task.done()
                due = (
                    self.state["next_poll_ms"] is not None
                    and now_ms() >= self.state["next_poll_ms"]
                )
                if idle and (action in {"connect", "refresh"} or due):
                    self.task = asyncio.create_task(self.sync(interactive=action == "connect"))
                self.publish()
                await asyncio.sleep(1)
        finally:
            strategy_task.cancel()
            await asyncio.gather(strategy_task, return_exceptions=True)
            server.close()
            await server.wait_closed()
            if self.task and not self.task.done():
                self.task.cancel()
                await asyncio.gather(self.task, return_exceptions=True)
            self.publish(state="offline", next_poll_ms=None, authorization_url=None)
            os.close(lock)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=DEFAULT_ROOT)
    parser.add_argument("--client", help="Existing role permitted to import observations")
    parser.add_argument("--interval", type=int)
    parser.add_argument("--callback-port", type=int)
    args = parser.parse_args()
    config_path = args.root / "broker-sync-config.json"
    config = read_private_json(config_path) if config_path.exists() else {}
    args.client = args.client or config.get("client")
    args.interval = args.interval or config.get("interval_seconds", 30)
    args.callback_port = args.callback_port or config.get("callback_port", 8766)
    if not args.client:
        parser.error("configure an existing observation-import role in broker-sync-config.json")
    if not 15 <= args.interval <= 300 or not 1024 <= args.callback_port <= 65535:
        parser.error("interval must be 15–300s and callback port 1024–65535")
    os.umask(0o077)
    logging.disable(logging.CRITICAL)
    try:
        asyncio.run(SyncWorker(args.root, args.client, args.interval, args.callback_port).run())
    except (KeyboardInterrupt, asyncio.CancelledError):
        pass
    except Exception:
        raise SystemExit(
            "Broker sync stopped. Check local configuration and callback port; "
            "no credentials were logged."
        ) from None


if __name__ == "__main__":
    main()
