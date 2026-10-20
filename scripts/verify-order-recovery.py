#!/usr/bin/env python3
"""Exercise deadline cancellation and lost replies through isolated Golem and a fake broker."""

from __future__ import annotations

import argparse
import asyncio
import hashlib
import importlib.util
import json
import os
import shutil
import subprocess
import tempfile
import time
import uuid
from datetime import UTC, datetime
from pathlib import Path

from helios_alpha.crypto.order_recovery import RecoveryError, RecoveryWorker

REPO = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location(
    "journal_proof", REPO / "scripts/verify-broker-journal.py"
)
helper = importlib.util.module_from_spec(spec)
spec.loader.exec_module(helper)


def timestamp(at: int) -> str:
    return datetime.fromtimestamp(at / 1000, UTC).isoformat(timespec="milliseconds")


def verify_legacy_upgrade(server, artifact: Path) -> dict:
    account = hashlib.sha256(str(uuid.uuid4()).encode()).hexdigest()
    agent = f'{server.app}/test/CryptoBrokerJournalAgent("{account}")'
    server.invoke(
        agent,
        "initialize",
        [
            helper.canonical(
                {
                    "schema_version": 1,
                    "clients": {
                        "legacy": [
                            "journal:read",
                            "journal:prepare",
                            "journal:dispatch",
                            "journal:reconcile",
                        ]
                    },
                }
            )
        ],
    )

    def command(action, payload=None):
        return server.invoke(agent, "command", ["legacy", action, helper.canonical(payload or {})])

    at = helper.millis()
    planned = {
        "account_key": account,
        "client_order_id": str(uuid.uuid4()),
        "symbol": "BTC-USD",
        "side": "buy",
        "quantity": "0.00012345",
        "limit_price": "100000",
        "reserved_cash_micros": 20_000_000,
        "policy_revision": 7,
        "strategy_fingerprint": "2" * 64,
        "input_digest": "3" * 64,
        "risk_reservation_id": "4" * 64,
        "created_ms": at,
        "expires_ms": at + 120_000,
    }
    command("prepare", planned)
    command("mark_dispatch_started", {"client_order_id": planned["client_order_id"]})
    before = command("status")
    original = command("record", {"client_order_id": planned["client_order_id"]})
    manifest_path = server.root / "golem.yaml"
    metadata = [
        "--app-manifest-path",
        str(manifest_path),
        "--environment",
        "test",
        "--format",
        "json",
        "component",
        "get",
        server.component,
    ]
    old_revision = json.loads(server.run(metadata))["componentRevision"]
    manifest = json.loads(manifest_path.read_text())
    manifest["components"][server.component]["componentWasm"] = str(artifact)
    manifest_path.write_text(json.dumps(manifest))
    server.artifact = artifact
    server.deploy(force_build=True)
    revision = json.loads(server.run(metadata))["componentRevision"]
    assert revision > old_revision
    server.connected(["--yes", "agent", "update", agent, "auto", str(revision), "--await"])
    assert command("status") == before
    assert command("record", {"client_order_id": planned["client_order_id"]}) == original
    assert command("account_state") == {"initialized": False, "balance": None}
    server.connected(["agent", "simulate-crash", agent])
    assert command("status") == before
    return {
        "old_revision": old_revision,
        "new_revision": revision,
        "legacy_snapshot_digest_preserved": True,
        "pending_reservation_preserved": True,
        "accounting_activated": False,
    }


def verify(server, fixture: Path, artifact: Path) -> dict:
    server.start()
    server.deploy()
    upgrade = verify_legacy_upgrade(server, artifact) if server.artifact != artifact else None
    account = hashlib.sha256(str(uuid.uuid4()).encode()).hexdigest()
    agent = f'{server.app}/test/CryptoBrokerJournalAgent("{account}")'
    server.invoke(
        agent,
        "initialize",
        [
            helper.canonical(
                {
                    "schema_version": 1,
                    "clients": {
                        "observer": ["journal:read"],
                        "worker": [
                            "journal:read",
                            "journal:prepare",
                            "journal:dispatch",
                            "journal:cancel",
                            "journal:reconcile",
                        ],
                    },
                }
            )
        ],
    )

    def command(action, payload=None, *, actor="worker", denied=False):
        return server.invoke(
            agent, "command", [actor, action, helper.canonical(payload or {})], denied=denied
        )

    def record(identity):
        return command("record", {"client_order_id": identity}, actor="observer")

    def status():
        return command("status", actor="observer")

    seed = {
        "schema_version": 1,
        "account_key": account,
        "cash_micros": 600_000_000,
        "settlement_quantum_micros": 10_000,
        "quantities": {},
        "source_digest": "7" * 64,
        "observed_ms": helper.millis(),
    }
    command("initialize_accounting", seed, actor="observer", denied=True)
    command("initialize_accounting", seed)
    now = helper.millis()
    planned = {
        "account_key": account,
        "client_order_id": str(uuid.uuid4()),
        "symbol": "BTC-USD",
        "side": "buy",
        "quantity": "0.00012345",
        "limit_price": "100000",
        "reserved_cash_micros": 20_000_000,
        "policy_revision": 7,
        "strategy_fingerprint": "2" * 64,
        "input_digest": "3" * 64,
        "risk_reservation_id": "4" * 64,
        "created_ms": now,
        "expires_ms": now + 2_000,
    }
    original_id = planned["client_order_id"]
    unsent = {
        **planned,
        "client_order_id": str(uuid.uuid4()),
        "risk_reservation_id": "5" * 64,
    }
    command("prepare", planned)
    command("prepare", unsent)
    command("mark_dispatch_started", {"client_order_id": original_id})
    command("plan_recovery", actor="observer", denied=True)
    pending = command("plan_recovery")
    assert pending["items"][0]["action"] == "lookup_order"
    assert status()["reservations"]["cash_micros"] == 40_000_000
    time.sleep(max(0, (planned["expires_ms"] - helper.millis() + 20) / 1000))

    work = command("plan_recovery")
    assert work["execution_authority"] is False
    assert [item["client_order_id"] for item in work["items"]] == [original_id]
    assert work["items"][0]["action"] == "cancel_order"
    assert record(unsent["client_order_id"])["phase"] == "expired_unsent"
    original = record(original_id)
    assert original["phase"] == "submission_uncertain"
    assert original["cancel_requested_ms"] >= planned["expires_ms"]
    assert status()["reservations"]["cash_micros"] == 20_000_000
    server.connected(["agent", "simulate-crash", agent])
    assert record(original_id) == original
    assert command("plan_recovery")["items"] == work["items"]
    print("Durable deadline work survived an agent crash.", flush=True)

    # Every HTTP-looking request below is served in memory by the Rust fixture. It
    # rejects order submissions and has no native HTTP transport or real credentials.
    observed = helper.millis() - 10
    broker_order = {
        "id": str(uuid.uuid4()),
        "account_number": "syntheticJournalApiAccount",
        "client_order_id": original_id,
        "symbol": "BTC-USD",
        "side": "buy",
        "type": "limit",
        "state": "partially_filled",
        "limit_order_config": {
            "asset_quantity": planned["quantity"],
            "limit_price": planned["limit_price"],
            "time_in_force": "gtc",
        },
        "fee_charged": "0.000000000000000123",
        "filled_asset_quantity": "0.000000000000000007",
        "executions": [
            {
                "effective_price": "99999.123456789",
                "quantity": "0.000000000000000007",
                "timestamp": timestamp(observed),
            }
        ],
        "created_at": timestamp(planned["created_ms"] + 1),
        "updated_at": timestamp(observed),
    }

    def recover(current_record, responses):
        result = subprocess.run(
            [str(fixture)],
            input=helper.canonical(
                {
                    "operation": "recover_and_cancel",
                    "record": current_record,
                    "responses": responses,
                    "received_ms": helper.millis(),
                }
            ),
            text=True,
            capture_output=True,
            timeout=10,
            check=True,
        )
        return json.loads(result.stdout)

    def page(value):
        return {"status": 200, "body": helper.canonical({"results": [value], "next": None})}

    class ProofJournal:
        account_key = account
        lose_accounting_reply = False

        def plan(self):
            return command("plan_recovery")

        def record(self, identity):
            return record(identity)

        def observe(self, observation):
            return command("observe", observation)

        def reconcile_terminal(self, identity, observation_digest):
            result = command(
                "reconcile_terminal",
                {
                    "client_order_id": identity,
                    "observation_digest": observation_digest,
                },
            )
            if self.lose_accounting_reply:
                self.lose_accounting_reply = False
                raise RecoveryError("synthetic_lost_accounting_reply")
            return result

    class FixtureBackend:
        def __init__(self, responses):
            self.responses = responses
            self.results = []

        async def recover(self, current_record):
            result = await asyncio.to_thread(recover, current_record, self.responses)
            self.results.append(result)
            if "error" in result:
                raise RecoveryError("synthetic_unknown_outcome")
            return result

    journal = ProofJournal()
    lost_backend = FixtureBackend([page(broker_order), None])
    first_worker = RecoveryWorker(journal, lost_backend)
    first_cycle = asyncio.run(first_worker.cycle())
    assert first_cycle["visited"] == first_cycle["unresolved"] == 1
    lost_reply = lost_backend.results[0]
    assert lost_reply["error"] == "AmbiguousOutcome"
    assert lost_reply["synthetic_get_requests"] == 1
    assert lost_reply["synthetic_cancel_requests"] == 1
    assert record(original_id) == original
    assert status()["reservations"]["cash_micros"] == 20_000_000
    server.hard_crash()
    server.start()
    assert record(original_id) == original
    assert command("plan_recovery")["items"] == work["items"]

    now = helper.millis()
    broker_order.update(
        state="filled",
        filled_asset_quantity=planned["quantity"],
        fee_charged="0.01",
        updated_at=timestamp(now),
    )
    broker_order["executions"].append(
        {
            "effective_price": "99998",
            "quantity": "0.000123449999999993",
            "timestamp": timestamp(now),
        }
    )
    # A fresh production worker resumes from Golem alone after the hard restart.
    recovered_backend = FixtureBackend([page(broker_order)])
    journal.lose_accounting_reply = True
    restarted_worker = RecoveryWorker(journal, recovered_backend, reconcile_accounting=True)
    recovered_cycle = asyncio.run(restarted_worker.cycle())
    assert recovered_cycle["visited"] == recovered_cycle["unresolved"] == 1
    recovered = recovered_backend.results[0]
    assert "error" not in recovered
    assert recovered["synthetic_get_requests"] == 1
    assert recovered["synthetic_cancel_requests"] == 0
    observed = record(original_id)
    assert observed["phase"] == "filled"
    assert command("plan_recovery")["items"] == []
    balance = command("account_state", actor="observer")["balance"]
    assert balance["cash_micros"] == balance["available_cash_micros"] == 587_630_000
    assert balance["quantities"] == {"BTC-USD": planned["quantity"]}
    assert balance["accounted_orders"] == 1 and balance["cash_deficit_micros"] == 0
    coverage = {
        "client_order_id": original_id,
        "coverage": {
            "account_key": account,
            "observation_digest": observed["observation_digest"],
            "account_snapshot_digest": "6" * 64,
            "snapshot_observed_ms": helper.millis(),
        },
    }
    command("cover_account", coverage, actor="observer", denied=True)
    command("cover_account", coverage, denied=True)
    command(
        "reconcile_terminal",
        {
            "client_order_id": original_id,
            "observation_digest": observed["observation_digest"],
        },
        actor="observer",
        denied=True,
    )
    assert command("plan_recovery")["items"] == []
    final = status()
    assert final["reservations"]["cash_micros"] == 0
    assert record(original_id)["intent"] == planned
    server.connected(["agent", "simulate-crash", agent])
    assert status() == final
    snapshots_before = server.connected(
        ["agent", "oplog", agent, "--query", "snapshot"], output_format="text"
    ).count("\nSNAPSHOT\n")
    # The component snapshots every ten successful calls, not every ten seconds.
    # Cross that boundary after booking, then recover from the resulting snapshot.
    for _ in range(10):
        assert command("account_state", actor="observer")["balance"] == balance
    snapshots_after = server.connected(
        ["agent", "oplog", agent, "--query", "snapshot"], output_format="text"
    ).count("\nSNAPSHOT\n")
    assert snapshots_after > snapshots_before, "new accounting snapshot was not recorded"
    server.hard_crash()
    server.start()
    assert status() == final
    assert command("account_state", actor="observer")["balance"] == balance
    assert command("plan_recovery")["items"] == []
    restarted_worker = RecoveryWorker(journal, recovered_backend, reconcile_accounting=True)
    empty_cycle = asyncio.run(restarted_worker.cycle())
    assert empty_cycle["visited"] == 0 and len(recovered_backend.results) == 1
    return {
        "checks": [
            "expiry only releases reservations for provably unsent orders",
            "deadline cancellation work and original identity survive crash recovery",
            "lost cancellation replies never release uncertain reservations",
            "hard server restart resumes lookup of the original order",
            "late fill is recovered exactly without resubmission or another cancellation",
            "terminal fills and fees are booked atomically with reservation release",
            "lost accounting acknowledgement survives hard restart without double booking",
            "automatic accounting snapshot survives hard server restart",
            "settlement rounding never overstates available strategy cash",
            "observer cannot plan cancellation work or cover account state",
            "production recovery worker resumes solely from Golem after restart",
            "covered terminal orders generate no further broker calls",
        ],
        "production_worker_used": True,
        "legacy_upgrade": upgrade,
        "new_accounting_snapshots": snapshots_after - snapshots_before,
        "worker_restarts": 2,
        "agent_crashes": 2 + int(upgrade is not None),
        "hard_server_crashes": server.hard_crashes,
        "synthetic_get_requests": 2,
        "synthetic_cancel_requests": 1,
        "synthetic_submit_requests": 0,
        "real_broker_requests": 0,
        "live_orders": 0,
        "main_runtime_touched": False,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--golem", default=shutil.which("golem"))
    parser.add_argument("--artifact", type=Path, default=helper.ARTIFACT)
    parser.add_argument("--previous-artifact", type=Path)
    parser.add_argument(
        "--adapter-fixture",
        type=Path,
        default=REPO / "rust/target/debug/examples/recover_journal_fixture",
    )
    parser.add_argument(
        "--proof",
        type=Path,
        default=Path.home() / ".local/share/helios-alpha/private/order-recovery-golem-proof.json",
    )
    args = parser.parse_args()
    os.umask(0o077)
    artifact = args.artifact.resolve(strict=True)
    previous = args.previous_artifact.resolve(strict=True) if args.previous_artifact else None
    fixture = args.adapter_fixture.resolve(strict=True)
    assert args.golem, "Golem executable is required"
    proof = {
        "schema_version": 1,
        "passed": False,
        "started_ms": helper.millis(),
        "artifact_sha256": hashlib.sha256(artifact.read_bytes()).hexdigest(),
        "previous_artifact_sha256": hashlib.sha256(previous.read_bytes()).hexdigest()
        if previous
        else None,
        "adapter_fixture_sha256": hashlib.sha256(fixture.read_bytes()).hexdigest(),
        "worker_source_sha256": hashlib.sha256(
            (REPO / "src/helios_alpha/crypto/order_recovery.py").read_bytes()
        ).hexdigest(),
    }
    with tempfile.TemporaryDirectory(prefix="helios-order-recovery-") as directory:
        server = helper.IsolatedGolem(args.golem, Path(directory), previous or artifact)
        try:
            proof.update(verify(server, fixture, artifact))
            proof["passed"] = True
        finally:
            server.close()
            proof["finished_ms"] = helper.millis()
            helper.private_write(args.proof, proof)
    print(json.dumps(proof, indent=2))


if __name__ == "__main__":
    main()
