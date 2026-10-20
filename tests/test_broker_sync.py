"""Read-only synchronization, OAuth isolation and recovery behavior."""

import asyncio
import json
from contextlib import asynccontextmanager
from types import SimpleNamespace

import pytest
import test_broker_snapshot as snapshot_fixture
from mcp.shared.auth import OAuthToken

from helios_alpha.control import broker_sync as sync
from helios_alpha.control.store import ControlError

NOW = snapshot_fixture.NOW
observation = snapshot_fixture.observation


@pytest.fixture
def anyio_backend():
    return "asyncio"


@pytest.fixture
def root(tmp_path):
    root = tmp_path / "private"
    root.mkdir(mode=0o700)
    return root


@pytest.fixture
def worker(root, monkeypatch):
    store = SimpleNamespace(
        root=root, settings=lambda _: {"client": {"capabilities": ["risk:write"]}}
    )
    monkeypatch.setattr(sync, "GolemControlStore", lambda _: store)
    return sync.SyncWorker(root, "test-importer", 30, 18766)


def test_account_selection_requires_unique_active_match(observation):
    account = observation["account"]
    key = sync.account_key(account)
    assert sync.select_account([account], key) == account
    for accounts, expected in [
        ([account, account], None),
        ([account], "wrong-account"),
        ([{**account, "deactivated": True}], key),
        ([], None),
    ]:
        with pytest.raises(ControlError):
            sync.select_account(accounts, expected)


@pytest.mark.anyio
async def test_collector_discovers_symbols_and_only_uses_read_tools(observation):
    used = []
    names = {
        "get_portfolio": "portfolio",
        "get_trade_approval_setting": "approvals",
        "get_crypto_positions": "positions",
        "get_crypto_orders": "open_orders",
        "get_crypto_quotes": "quotes",
    }

    class Session:
        async def call_tool(self, tool, arguments):
            used.append(tool)
            assert tool in sync.READ_TOOLS
            if tool == "get_accounts":
                data = {"accounts": [observation["account"]]}
            else:
                data = observation["calls"][names[tool]]["data"]
            if tool == "get_crypto_quotes":
                assert arguments["symbols"] == ["BTC-USD"]
            return SimpleNamespace(isError=False, structuredContent={"data": data})

    result = await sync.collect(Session(), sync.account_key(observation["account"]))
    assert set(used) == sync.READ_TOOLS
    assert result["previews"] == []
    assert result["calls"]["positions"]["data"] == observation["calls"]["positions"]["data"]


@pytest.mark.anyio
async def test_tokens_are_private_and_expiry_does_not_reset_on_restart(root, monkeypatch):
    monkeypatch.setattr(sync, "now_ms", lambda: NOW)
    storage = sync.PrivateOAuthStorage(root)
    await storage.set_tokens(
        OAuthToken(access_token="synthetic-token", token_type="Bearer", expires_in=60)
    )
    path = root / "broker-sync-oauth-tokens.json"
    assert path.stat().st_mode & 0o777 == 0o600
    monkeypatch.setattr(sync, "now_ms", lambda: NOW + 61_000)
    assert (await sync.PrivateOAuthStorage(root).get_tokens()).expires_in == 0
    path.chmod(0o644)
    with pytest.raises(ControlError):
        await storage.get_tokens()


@pytest.mark.anyio
async def test_oauth_rejects_wrong_state_and_replay(worker):
    worker.interactive = True
    await worker.redirect("https://robinhood.com/oauth?state=test-state")

    class Writer:
        def __init__(self):
            self.data = b""

        def write(self, data):
            self.data += data

        async def drain(self):
            pass

        def close(self):
            pass

        async def wait_closed(self):
            pass

    async def callback(state):
        reader = asyncio.StreamReader()
        reader.feed_data(
            (
                f"GET /callback?code=synthetic-code&state={state} HTTP/1.1\r\n"
                "Host: localhost\r\n\r\n"
            ).encode()
        )
        reader.feed_eof()
        writer = Writer()
        await worker.accept_callback(reader, writer)
        return writer.data

    assert b"400" in await callback("wrong-state")
    assert not worker.callback.done()
    assert b"200" in await callback("test-state")
    assert await worker.wait_callback() == ("synthetic-code", "test-state")
    assert b"400" in await callback("test-state")
    status = (worker.root / "broker-sync-status.json").read_text()
    assert "synthetic-code" not in status and "test-state" not in status


@pytest.mark.anyio
async def test_no_background_login_prompt_or_foreign_authorization_host(worker):
    with pytest.raises(ControlError):
        await worker.redirect("https://robinhood.com/oauth?state=test")
    worker.interactive = True
    with pytest.raises(ControlError):
        await worker.redirect("https://robinhood.com.evil.example/oauth?state=test")


@pytest.mark.anyio
async def test_failed_read_retains_last_account_and_backs_off(worker, monkeypatch):
    before = b'{"sentinel":"last-successful-account"}'
    path = worker.root / "broker-observation.json"
    path.write_bytes(before)
    monkeypatch.setattr(sync, "_load_observation", lambda _: {"account_key": "expected"})
    await worker.storage.set_tokens(
        OAuthToken(access_token="synthetic-token", token_type="Bearer", expires_in=60)
    )

    @asynccontextmanager
    async def transport(*args, **kwargs):
        raise RuntimeError("secret-provider-response-must-not-leak")
        yield

    monkeypatch.setattr(sync, "streamable_http_client", transport)
    await worker.sync()
    status = json.loads((worker.root / "broker-sync-status.json").read_text())
    assert status["state"] == "degraded"
    assert status["last_success_ms"] is None
    assert status["next_poll_ms"] > sync.now_ms()
    assert "secret-provider-response" not in json.dumps(status)
    assert path.read_bytes() == before


@pytest.mark.anyio
async def test_permission_is_checked_before_any_broker_connection(worker, monkeypatch):
    worker.store.settings = lambda _: {"client": {"capabilities": ["status:read"]}}

    def no_network(*args, **kwargs):
        pytest.fail("must not contact broker without the configured import permission")

    monkeypatch.setattr(sync, "streamable_http_client", no_network)
    await worker.sync()
    assert worker.state["last_success_ms"] is None
