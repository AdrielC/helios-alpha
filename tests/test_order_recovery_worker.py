"""Recovery retries must preserve journal ownership, funds and cancellation uncertainty."""

import asyncio
import copy
import hashlib
import json
import os
import sys
import uuid

import pytest

from helios_alpha.control.store import ControlError
from helios_alpha.crypto.order_recovery import (
    MAX_RESPONSE_BYTES,
    GolemRecoveryJournal,
    NativeRecoveryBackend,
    RecoveryError,
    RecoveryWorker,
    serve,
)

ACCOUNT = "a" * 64


class MemoryJournal:
    account_key = ACCOUNT

    def __init__(self, count=1):
        self.records = {}
        self.commits = 0
        self.lose_reply = False
        self.accounting_calls = 0
        self.lose_accounting_reply = False
        for i in range(count):
            identity = str(uuid.UUID(int=i + 1, version=4))
            self.records[identity] = {
                "intent": {
                    "account_key": ACCOUNT,
                    "client_order_id": identity,
                    "expires_ms": 100,
                },
                "phase": "submission_uncertain",
                "revision": 3,
                "dispatch_started_ms": 2,
                "cancel_requested_ms": 100,
                "account_coverage": None,
            }

    def plan(self):
        return {
            "execution_authority": False,
            "evaluated_ms": 200,
            "items": [
                {
                    "client_order_id": identity,
                    "revision": record["revision"],
                    "expires_ms": 100,
                    "action": (
                        "reconcile_account" if record["phase"] == "filled" else "cancel_order"
                    ),
                }
                for identity, record in self.records.items()
                if record["account_coverage"] is None
            ],
        }

    def record(self, identity):
        return copy.deepcopy(self.records[identity])

    def observe(self, observation):
        record = self.records[observation["client_order_id"]]
        record["revision"] += 1
        record["phase"] = "filled"
        record["observation_digest"] = "b" * 64
        self.commits += 1
        if self.lose_reply:
            self.lose_reply = False
            raise ControlError("synthetic lost acknowledgement")
        return {"execution_authority": False, "record": copy.deepcopy(record)}

    def reconcile_terminal(self, identity, observation_digest):
        record = self.records[identity]
        assert record["observation_digest"] == observation_digest
        self.accounting_calls += 1
        record["account_coverage"] = {"observation_digest": observation_digest}
        record["revision"] += 1
        if self.lose_accounting_reply:
            self.lose_accounting_reply = False
            raise ControlError("synthetic lost accounting acknowledgement")
        return {"execution_authority": False, "record": copy.deepcopy(record)}


class MemoryBackend:
    def __init__(self):
        self.visited = []
        self.cancel_attempts = 0
        self.active = self.peak = 0
        self.fail = False

    async def recover(self, record):
        self.active += 1
        self.peak = max(self.peak, self.active)
        try:
            self.visited.append(record["intent"]["client_order_id"])
            await asyncio.sleep(0.001)
            if self.fail:
                raise RecoveryError("synthetic unknown outcome")
            self.cancel_attempts += record["phase"] != "filled"
            return {
                "observation": {
                    "account_key": ACCOUNT,
                    "client_order_id": record["intent"]["client_order_id"],
                }
            }
        finally:
            self.active -= 1


def test_failed_orders_do_not_starve_later_work_and_concurrency_is_bounded():
    journal, backend = MemoryJournal(17), MemoryBackend()
    backend.fail = True
    worker = RecoveryWorker(journal, backend, batch_size=3, concurrency=2)

    async def check():
        for _ in range(6):
            result = await worker.cycle()
            assert result["visited"] == result["unresolved"] == 3

    asyncio.run(check())
    assert set(backend.visited) == set(journal.records)
    assert backend.peak <= 2
    assert journal.commits == 0


def test_lost_commit_reply_is_recovered_from_the_journal_after_worker_restart():
    journal, backend = MemoryJournal(), MemoryBackend()
    journal.lose_reply = True
    first = asyncio.run(RecoveryWorker(journal, backend).cycle())
    assert first["unresolved"] == 1 and journal.commits == 1
    assert backend.cancel_attempts == 1
    second = asyncio.run(RecoveryWorker(journal, backend).cycle())
    assert second["account_pending"] == 1
    assert backend.cancel_attempts == 1
    assert all(record["account_coverage"] is None for record in journal.records.values())
    assert second["can_submit_orders"] is False
    assert second["can_release_reservations"] is False


def test_accounting_requires_opt_in_and_recovers_a_lost_commit_reply_without_double_booking():
    journal, backend = MemoryJournal(), MemoryBackend()
    pending = asyncio.run(RecoveryWorker(journal, backend).cycle())
    assert pending["account_pending"] == 1 and journal.accounting_calls == 0
    journal.lose_accounting_reply = True
    first = asyncio.run(RecoveryWorker(journal, backend, reconcile_accounting=True).cycle())
    assert first["unresolved"] == 1 and journal.accounting_calls == 1
    calls = len(backend.visited)
    second = asyncio.run(RecoveryWorker(journal, backend, reconcile_accounting=True).cycle())
    assert second["visited"] == 0
    assert journal.accounting_calls == 1 and len(backend.visited) == calls


def test_confirmed_accounting_receipt_must_cover_the_observation_that_was_committed():
    journal, backend = MemoryJournal(), MemoryBackend()
    first = asyncio.run(RecoveryWorker(journal, backend, reconcile_accounting=True).cycle())
    assert first["accounted"] == 1 and first["can_release_reservations"] is True
    journal = MemoryJournal()
    journal.reconcile_terminal = lambda *args: {
        "execution_authority": False,
        "record": {
            **next(iter(journal.records.values())),
            "account_coverage": {
                "observation_digest": "c" * 64,
            },
        },
    }
    second = asyncio.run(RecoveryWorker(journal, backend, reconcile_accounting=True).cycle())
    assert second["unresolved"] == 1
    assert next(iter(journal.records.values()))["account_coverage"] is None


@pytest.mark.parametrize(
    "change",
    [
        {"account_key": "b" * 64},
        {"client_order_id": str(uuid.uuid4())},
        {"expires_ms": 101},
    ],
)
def test_changed_record_binding_never_reaches_broker(change):
    journal, backend = MemoryJournal(), MemoryBackend()
    original = journal.record

    def changed(identity):
        record = original(identity)
        record["intent"].update(change)
        return record

    journal.record = changed
    assert asyncio.run(RecoveryWorker(journal, backend).cycle())["unresolved"] == 1
    assert not backend.visited and journal.commits == 0


def test_missing_or_wrong_broker_identity_never_commits():
    for response in [None, {"account_key": "b" * 64, "client_order_id": str(uuid.uuid4())}]:
        journal = MemoryJournal()

        class Backend:
            async def recover(self, _):
                return {"observation": response}

        result = asyncio.run(RecoveryWorker(journal, Backend()).cycle())
        assert result["missing" if response is None else "unresolved"] == 1
        assert journal.commits == 0


@pytest.mark.parametrize("bad", ["identity", "revision", "duplicate", "authority", "submit"])
def test_invalid_work_list_is_rejected_before_a_broker_call(bad):
    journal, backend = MemoryJournal(), MemoryBackend()
    plan = journal.plan()
    if bad == "identity":
        plan["items"][0]["client_order_id"] = "arbitrary-command"
    elif bad == "revision":
        plan["items"][0]["revision"] = True
    elif bad == "duplicate":
        plan["items"] *= 2
    elif bad == "authority":
        plan["execution_authority"] = True
    else:
        plan["items"][0]["action"] = "submit"
    journal.plan = lambda: plan
    with pytest.raises(RecoveryError, match="invalid_work"):
        asyncio.run(RecoveryWorker(journal, backend).cycle())
    assert not backend.visited


def test_heartbeat_failure_does_not_stop_the_worker():
    async def check():
        journal, backend = MemoryJournal(), MemoryBackend()
        stopped = asyncio.Event()
        calls = 0

        async def publish(_):
            nonlocal calls
            calls += 1
            if calls == 2:
                stopped.set()
            raise ControlError("synthetic heartbeat failure")

        await asyncio.wait_for(
            serve(RecoveryWorker(journal, backend), stopped, publish, poll_seconds=0.1), 2
        )
        assert calls == 2

    asyncio.run(check())


def test_stop_cancels_inflight_recovery_without_retry():
    async def check():
        journal = MemoryJournal()
        stopped, entered, canceled = asyncio.Event(), asyncio.Event(), asyncio.Event()

        class Backend:
            async def recover(self, _):
                entered.set()
                try:
                    await asyncio.Event().wait()
                finally:
                    canceled.set()

        async def publish(_):
            pass

        task = asyncio.create_task(serve(RecoveryWorker(journal, Backend()), stopped, publish))
        await asyncio.wait_for(entered.wait(), 1)
        stopped.set()
        await asyncio.wait_for(task, 1)
        assert canceled.is_set() and journal.commits == 0

    asyncio.run(check())


def backend_files(tmp_path, body):
    executable = tmp_path / "adapter"
    executable.write_text(f"#!{sys.executable}\n" + body)
    executable.chmod(0o700)
    credentials = tmp_path / "credentials.json"
    credentials.write_text(
        json.dumps(
            {
                "schema_version": 1,
                "account_number": "syntheticAccount",
                "journal_account_key": ACCOUNT,
                "api_key": "synthetic-secret-key",
                "private_key_base64": "synthetic-private-key",
            }
        )
    )
    credentials.chmod(0o600)
    digest = hashlib.sha256(executable.read_bytes()).hexdigest()
    return NativeRecoveryBackend(executable, digest, credentials, ACCOUNT, "observe")


def test_native_adapter_receives_credentials_only_on_stdin(tmp_path, capsys):
    backend = backend_files(
        tmp_path,
        """
import json,sys
request=json.load(sys.stdin)
assert len(sys.argv)==1
assert request["api_key"]=="synthetic-secret-key"
assert request["cancel_allowed"] is False
print("synthetic-private-key",file=sys.stderr)
print(json.dumps({"schema_version":1,"observation":None}))
""",
    )
    assert asyncio.run(backend.recover({}))["observation"] is None
    assert "synthetic" not in capsys.readouterr().out
    backend.executable.write_text("changed binary")
    with pytest.raises(RecoveryError, match="backend_version"):
        asyncio.run(backend.recover({}))


def test_backend_rejects_other_account_and_public_credentials_before_launch(tmp_path):
    backend = backend_files(tmp_path, "raise AssertionError('must not launch')")
    value = json.loads(backend.credentials.read_text())
    value["journal_account_key"] = "b" * 64
    backend.credentials.write_text(json.dumps(value))
    with pytest.raises(RecoveryError, match="broker_credentials"):
        asyncio.run(backend.recover({}))
    backend.credentials.chmod(0o644)
    with pytest.raises(ControlError, match="private"):
        asyncio.run(backend.recover({}))


def test_oversized_adapter_reply_is_killed_without_unbounded_buffering(tmp_path):
    pid = tmp_path / "pid"
    backend = backend_files(
        tmp_path,
        f"""
import os,sys,time
from pathlib import Path
Path({str(pid)!r}).write_text(str(os.getpid()))
sys.stdin.read()
sys.stdout.write("x"*{MAX_RESPONSE_BYTES + 1})
sys.stdout.flush()
time.sleep(60)
""",
    )
    with pytest.raises(RecoveryError, match="response_bound"):
        asyncio.run(backend.recover({}))
    with pytest.raises(ProcessLookupError):
        os.kill(int(pid.read_text()), 0)


def test_canceling_backend_reaps_the_child_process(tmp_path):
    pid = tmp_path / "pid"
    backend = backend_files(
        tmp_path,
        f"""
import os,sys,time
from pathlib import Path
sys.stdin.read()
Path({str(pid)!r}).write_text(str(os.getpid()))
time.sleep(60)
""",
    )

    async def check():
        task = asyncio.create_task(backend.recover({}))
        for _ in range(100):
            if pid.exists():
                break
            await asyncio.sleep(0.01)
        assert pid.exists()
        task.cancel()
        with pytest.raises(asyncio.CancelledError):
            await task

    asyncio.run(check())
    with pytest.raises(ProcessLookupError):
        os.kill(int(pid.read_text()), 0)


def test_golem_gateway_uses_fixed_roles_and_rejects_execution_authority(tmp_path):
    connection = tmp_path / "broker-journal-connection.json"
    connection.write_text(json.dumps({"schema_version": 1, "account_key": ACCOUNT}))
    connection.chmod(0o600)

    class Store:
        root = tmp_path
        calls = []
        reply = {"schema_version": 1, "execution_authority": False, "result": {}}

        def _invoke_json_at(self, address, method, arguments):
            self.calls.append((address, method, arguments))
            return self.reply

    store = Store()
    journal = GolemRecoveryJournal(store)
    journal.plan()
    journal.record(str(uuid.uuid4()))
    journal.observe({})
    journal.reconcile_terminal(str(uuid.uuid4()), "b" * 64)
    assert [call[2][0] for call in store.calls] == [
        "broker-executor",
        "broker-reconciler",
        "broker-reconciler",
        "broker-reconciler",
    ]
    store.reply["execution_authority"] = True
    with pytest.raises(RecoveryError, match="journal_response"):
        journal.plan()
