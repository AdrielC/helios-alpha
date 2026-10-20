"""Bounded order recovery, with optional atomic accounting in the durable journal."""

from __future__ import annotations

import argparse
import asyncio
import fcntl
import hashlib
import json
import os
import re
import signal
import sys
import time
import uuid
from pathlib import Path
from typing import Protocol

from helios_alpha.control.golem_store import GolemControlStore, read_private_json
from helios_alpha.control.store import DEFAULT_ROOT, ControlError, canonical, private_root

MAX_RECORDS = 256
MAX_RESPONSE_BYTES = 262_144
DIGEST = re.compile(r"[a-f0-9]{64}\Z")
TERMINAL_PHASES = {"filled", "canceled", "failed"}


class RecoveryError(RuntimeError):
    """Errors are reported by category, never by broker payload or secret-bearing text."""


class Journal(Protocol):
    account_key: str

    def plan(self) -> dict: ...
    def record(self, identity: str) -> dict: ...
    def observe(self, observation: dict) -> dict: ...
    def reconcile_terminal(self, identity: str, observation_digest: str) -> dict: ...


class Backend(Protocol):
    async def recover(self, record: dict) -> dict: ...


def identity_valid(value) -> bool:
    try:
        return isinstance(value, str) and str(uuid.UUID(value)) == value
    except (ValueError, AttributeError):
        return False


def uint(value) -> bool:
    return type(value) is int and 0 <= value < 2**64


class GolemRecoveryJournal:
    """Trusted local worker binds the existing executor and reconciler roles."""

    def __init__(self, store: GolemControlStore):
        self.store = store
        connection = read_private_json(store.root / "broker-journal-connection.json")
        if (
            set(connection) != {"schema_version", "account_key"}
            or type(connection["schema_version"]) is not int
            or connection["schema_version"] != 1
            or not isinstance(connection["account_key"], str)
            or not DIGEST.fullmatch(connection["account_key"])
        ):
            raise RecoveryError("journal_connection")
        self.account_key = connection["account_key"]
        self.address = f'helios-alpha/local/CryptoBrokerJournalAgent("{self.account_key}")'

    def _call(self, actor: str, action: str, payload: dict | None = None) -> dict:
        response = self.store._invoke_json_at(
            self.address, "command", [actor, action, canonical(payload or {})]
        )
        if (
            not isinstance(response, dict)
            or type(response.get("schema_version")) is not int
            or response["schema_version"] != 1
            or response.get("execution_authority") is not False
            or not isinstance(response.get("result"), dict)
        ):
            raise RecoveryError("journal_response")
        return response["result"]

    def plan(self) -> dict:
        return self._call("broker-executor", "plan_recovery")

    def record(self, identity: str) -> dict:
        return self._call("broker-reconciler", "record", {"client_order_id": identity})

    def observe(self, observation: dict) -> dict:
        return self._call("broker-reconciler", "observe", observation)

    def reconcile_terminal(self, identity: str, observation_digest: str) -> dict:
        return self._call(
            "broker-reconciler",
            "reconcile_terminal",
            {"client_order_id": identity, "observation_digest": observation_digest},
        )


class NativeRecoveryBackend:
    """One subprocess per order, credentials on STDIN, wall-time bound, no submit command."""

    def __init__(
        self, executable: Path, sha256: str, credentials: Path, account_key: str, mode: str
    ):
        if (
            mode not in {"observe", "cancel"}
            or not isinstance(sha256, str)
            or not DIGEST.fullmatch(sha256)
            or not credentials.is_absolute()
        ):
            raise RecoveryError("backend_configuration")
        if not executable.is_absolute() or executable.is_symlink() or not executable.is_file():
            raise RecoveryError("backend_executable")
        info = executable.stat()
        if info.st_uid != os.getuid() or info.st_mode & 0o022:
            raise RecoveryError("backend_ownership")
        if hashlib.sha256(executable.read_bytes()).hexdigest() != sha256:
            raise RecoveryError("backend_version")
        self.executable_identity = (
            info.st_dev,
            info.st_ino,
            info.st_size,
            info.st_mtime_ns,
            info.st_ctime_ns,
        )
        self.executable = executable
        self.credentials = credentials
        self.account_key = account_key
        self.mode = mode

    def credentials_checked(self) -> dict:
        credentials = read_private_json(self.credentials)
        if (
            set(credentials)
            != {
                "schema_version",
                "account_number",
                "journal_account_key",
                "api_key",
                "private_key_base64",
            }
            or type(credentials["schema_version"]) is not int
            or credentials["schema_version"] != 1
            or credentials["journal_account_key"] != self.account_key
            or any(
                not isinstance(credentials[key], str) or not credentials[key]
                for key in ("account_number", "api_key", "private_key_base64")
            )
        ):
            raise RecoveryError("broker_credentials")
        return credentials

    async def recover(self, record: dict) -> dict:
        response = await self._request({"cancel_allowed": self.mode == "cancel", "record": record})
        if "observation" not in response:
            raise RecoveryError("broker_outcome_unknown")
        return response

    async def _request(self, arguments: dict) -> dict:
        """Shared bounded process transport; the pinned executable defines its operation."""
        info = self.executable.lstat()
        if (
            info.st_dev,
            info.st_ino,
            info.st_size,
            info.st_mtime_ns,
            info.st_ctime_ns,
        ) != self.executable_identity:
            raise RecoveryError("backend_version")
        credentials = self.credentials_checked()
        if credentials.keys() & arguments.keys():
            raise RecoveryError("broker_request_shape")
        payload = canonical({**credentials, **arguments}).encode()
        if len(payload) > MAX_RESPONSE_BYTES:
            raise RecoveryError("broker_request_bound")
        process = await asyncio.create_subprocess_exec(
            str(self.executable),
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.DEVNULL,
            env={"PATH": os.defpath},
            limit=MAX_RESPONSE_BYTES,
        )

        async def communicate():
            process.stdin.write(payload)
            await process.stdin.drain()
            process.stdin.close()
            output = bytearray()
            while True:
                chunk = await process.stdout.read(min(8192, MAX_RESPONSE_BYTES + 1 - len(output)))
                if not chunk:
                    break
                output.extend(chunk)
                if len(output) > MAX_RESPONSE_BYTES:
                    raise RecoveryError("broker_response_bound")
            await process.wait()
            return bytes(output)

        try:
            stdout = await asyncio.wait_for(communicate(), timeout=12)
            if process.returncode != 0 or len(stdout) > MAX_RESPONSE_BYTES:
                raise RecoveryError("broker_process")
            response = json.loads(stdout)
            if (
                not isinstance(response, dict)
                or type(response.get("schema_version")) is not int
                or response["schema_version"] != 1
                or "error" in response
            ):
                raise RecoveryError("broker_outcome_unknown")
            return response
        except (TimeoutError, ValueError) as exc:
            raise RecoveryError("broker_outcome_unknown") from exc
        finally:
            if process.returncode is None:
                try:
                    process.kill()
                except ProcessLookupError:
                    pass
            await process.wait()


class RecoveryWorker:
    """Fair batches with fresh journal reads; failures cannot advance money or resubmit."""

    def __init__(
        self,
        journal: Journal,
        backend: Backend,
        *,
        batch_size=8,
        concurrency=2,
        reconcile_accounting=False,
    ):
        if (
            type(batch_size) is not int
            or not 1 <= batch_size <= 32
            or type(concurrency) is not int
            or not 1 <= concurrency <= 4
            or type(reconcile_accounting) is not bool
        ):
            raise RecoveryError("worker_bounds")
        self.journal, self.backend = journal, backend
        self.batch_size, self.concurrency = batch_size, concurrency
        self.reconcile_accounting = reconcile_accounting
        self.after = ""

    def _plan(self, plan: dict) -> list[dict]:
        if (
            not isinstance(plan, dict)
            or plan.get("execution_authority") is not False
            or not uint(plan.get("evaluated_ms"))
            or not isinstance(plan.get("items"), list)
            or len(plan["items"]) > MAX_RECORDS
        ):
            raise RecoveryError("invalid_work")
        seen = set()
        for item in plan["items"]:
            if (
                not isinstance(item, dict)
                or not identity_valid(item.get("client_order_id"))
                or item["client_order_id"] in seen
                or not uint(item.get("revision"))
                or item["revision"] == 0
                or not uint(item.get("expires_ms"))
                or item.get("action") not in {"lookup_order", "cancel_order", "reconcile_account"}
            ):
                raise RecoveryError("invalid_work")
            seen.add(item["client_order_id"])
        items = sorted(plan["items"], key=lambda item: item["client_order_id"])
        return [item for item in items if item["client_order_id"] > self.after] + [
            item for item in items if item["client_order_id"] <= self.after
        ]

    def _record(self, record: dict, item: dict) -> None:
        if (
            not isinstance(record, dict)
            or not isinstance(record.get("intent"), dict)
            or record["intent"].get("account_key") != self.journal.account_key
            or record["intent"].get("client_order_id") != item["client_order_id"]
            or record["intent"].get("expires_ms") != item["expires_ms"]
            or not uint(record.get("revision"))
            or record["revision"] < item["revision"]
            or not uint(record.get("dispatch_started_ms"))
            or (
                item["action"] == "cancel_order"
                and record.get("cancel_requested_ms") is None
                and record.get("phase") not in TERMINAL_PHASES
            )
        ):
            raise RecoveryError("journal_identity_or_revision")

    async def cycle(self) -> dict:
        items = self._plan(await asyncio.to_thread(self.journal.plan))
        selected = items[: self.batch_size]
        semaphore = asyncio.Semaphore(self.concurrency)

        async def recover(item):
            async with semaphore:
                try:
                    record = await asyncio.to_thread(self.journal.record, item["client_order_id"])
                    self._record(record, item)
                    if record.get("account_coverage") is not None:
                        return "covered"
                    result = await self.backend.recover(record)
                    observation = result.get("observation")
                    if observation is None:
                        return "missing"
                    if (
                        not isinstance(observation, dict)
                        or observation.get("account_key") != self.journal.account_key
                        or observation.get("client_order_id") != item["client_order_id"]
                    ):
                        raise RecoveryError("broker_identity")
                    receipt = await asyncio.to_thread(self.journal.observe, observation)
                    if receipt.get("execution_authority") is not False or not isinstance(
                        receipt.get("record"), dict
                    ):
                        raise RecoveryError("journal_commit_unconfirmed")
                    self._record(receipt["record"], item)
                    if receipt["record"]["phase"] not in TERMINAL_PHASES:
                        return "observed"
                    if not self.reconcile_accounting:
                        return "account_pending"
                    digest = receipt["record"].get("observation_digest")
                    if not isinstance(digest, str) or not DIGEST.fullmatch(digest):
                        raise RecoveryError("journal_observation_digest")
                    accounted = await asyncio.to_thread(
                        self.journal.reconcile_terminal, item["client_order_id"], digest
                    )
                    if (
                        not isinstance(accounted, dict)
                        or accounted.get("execution_authority") is not False
                        or not isinstance(accounted.get("record"), dict)
                    ):
                        raise RecoveryError("journal_accounting_unconfirmed")
                    covered = accounted["record"]
                    self._record(covered, item)
                    if (
                        covered.get("observation_digest") != digest
                        or not isinstance(covered.get("account_coverage"), dict)
                        or covered["account_coverage"].get("observation_digest") != digest
                    ):
                        raise RecoveryError("journal_accounting_unconfirmed")
                    return "accounted"
                except (RecoveryError, ControlError, OSError, ValueError, KeyError, TypeError):
                    # Never include exception text: broker responses can contain sensitive data.
                    return "unresolved"

        outcomes = await asyncio.gather(*(recover(item) for item in selected))
        if selected:
            self.after = selected[-1]["client_order_id"]
        return {
            "state": "running",
            "work_items": len(items),
            "visited": len(selected),
            **{
                key: outcomes.count(key)
                for key in (
                    "covered",
                    "missing",
                    "observed",
                    "account_pending",
                    "accounted",
                    "unresolved",
                )
            },
            "can_submit_orders": False,
            "can_release_reservations": self.reconcile_accounting,
        }


async def serve(worker: RecoveryWorker, stopped: asyncio.Event, publish, *, poll_seconds=1.0):
    if type(poll_seconds) not in (int, float) or not 0.1 <= poll_seconds <= 60:
        raise RecoveryError("poll_interval")
    stop_task = asyncio.create_task(stopped.wait())
    cycle = None
    try:
        while not stopped.is_set():
            cycle = asyncio.create_task(worker.cycle())
            done, _ = await asyncio.wait([cycle, stop_task], return_when=asyncio.FIRST_COMPLETED)
            if stop_task in done:
                break
            try:
                status = cycle.result()
            except (RecoveryError, ControlError, OSError, ValueError):
                status = {"state": "journal_unavailable", "can_submit_orders": False}
            try:
                await publish(status)
            except (RecoveryError, ControlError, OSError, ValueError):
                # A missing heartbeat acknowledgement cannot stop recovery of an order.
                pass
            try:
                await asyncio.wait_for(stopped.wait(), poll_seconds)
            except TimeoutError:
                pass
    finally:
        for task in (cycle, stop_task):
            if task is not None and not task.done():
                task.cancel()
        await asyncio.gather(
            *(t for t in (cycle, stop_task) if t is not None), return_exceptions=True
        )


async def run(root: Path, *, once=False):
    config_path = root / "order-recovery.json"
    if not config_path.exists():
        return {"state": "not_configured"}
    config = read_private_json(config_path)
    if (
        set(config) - {"reconcile_accounting"}
        != {
            "schema_version",
            "enabled",
            "mode",
            "adapter_path",
            "adapter_sha256",
            "credentials_path",
            "batch_size",
            "concurrency",
            "poll_seconds",
        }
        or type(config["schema_version"]) is not int
        or config["schema_version"] != 1
        or type(config["enabled"]) is not bool
        or config["mode"] not in {"observe", "cancel"}
    ):
        raise RecoveryError("worker_configuration")
    if not config["enabled"]:
        return {"state": "disabled", "broker_requests": 0}
    fd = os.open(root / "order-recovery.lock", os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
    with os.fdopen(fd, "a+") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        store = GolemControlStore(root)
        journal = GolemRecoveryJournal(store)
        backend = NativeRecoveryBackend(
            Path(config["adapter_path"]),
            config["adapter_sha256"],
            Path(config["credentials_path"]),
            journal.account_key,
            config["mode"],
        )
        backend.credentials_checked()
        worker = RecoveryWorker(
            journal,
            backend,
            batch_size=config["batch_size"],
            concurrency=config["concurrency"],
            reconcile_accounting=config.get("reconcile_accounting", False),
        )
        if once:
            return await worker.cycle()
        stopped = asyncio.Event()
        for sig in (signal.SIGTERM, signal.SIGINT):
            asyncio.get_running_loop().add_signal_handler(sig, stopped.set)
        last_heartbeat = 0.0

        async def publish(status):
            nonlocal last_heartbeat
            if time.monotonic() - last_heartbeat >= 10:
                await asyncio.to_thread(
                    store.heartbeat,
                    "crypto-runtime",
                    "broker-recovery",
                    {**status, "mode": config["mode"]},
                )
                last_heartbeat = time.monotonic()

        await serve(worker, stopped, publish, poll_seconds=config["poll_seconds"])
        return {"state": "stopped"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=DEFAULT_ROOT)
    parser.add_argument("--once", action="store_true")
    args = parser.parse_args()
    os.umask(0o077)
    try:
        result = asyncio.run(run(private_root(args.root), once=args.once))
    except (RecoveryError, ControlError, OSError, ValueError, TypeError):
        print(json.dumps({"state": "unavailable", "detail": "check private worker configuration"}))
        sys.exit(1)
    print(json.dumps(result))


if __name__ == "__main__":
    main()
