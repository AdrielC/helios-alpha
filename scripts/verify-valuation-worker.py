#!/usr/bin/env python3
"""Verify the valuation worker against isolated Golem with synthetic broker read sets."""

from __future__ import annotations

import argparse
import asyncio
import hashlib
import importlib.util
import json
import os
import shutil
import tempfile
import uuid
from pathlib import Path

from helios_alpha.control.store import ControlError, canonical
from helios_alpha.crypto.valuation_reconciler import (
    FeeBound,
    GolemValuationJournal,
    ValuationWorker,
    digest,
    millis,
)

REPO = Path(__file__).resolve().parents[1]


def load(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


support = load("journal_proof", REPO / "scripts/verify-broker-journal.py")
fixtures = load("valuation_fixtures", REPO / "tests/test_allocation_valuation.py")


def verify(server) -> dict:
    account = fixtures.ACCOUNT
    agent = f'{server.app}/test/CryptoBrokerJournalAgent("{account}")'
    server.invoke(
        agent,
        "initialize",
        [
            canonical(
                {
                    "schema_version": 1,
                    "clients": {
                        "observer": ["journal:read"],
                        "broker-reconciler": ["journal:read", "journal:reconcile"],
                        "executor": ["journal:read", "journal:prepare", "journal:dispatch"],
                    },
                }
            )
        ],
    )
    support.private_write(
        server.root / "broker-journal-connection.json",
        {"schema_version": 1, "account_key": account},
    )

    class Store(fixtures.Control):
        root = server.root
        lost_mark_reply = False
        mark_calls = 0

        def _invoke_json_at(self, address, method, arguments):
            assert address == agent
            result = server.invoke(address, method, arguments)
            if arguments[1] in {"mark_allocation", "mark_prepared_allocation"}:
                self.mark_calls += 1
                if self.lost_mark_reply:
                    self.lost_mark_reply = False
                    raise ControlError("synthetic lost response after durable commit")
            return {"schema_version": 1, "execution_authority": False, "result": result}

    def command(action, payload=None, actor="observer"):
        return server.invoke(agent, "command", [actor, action, canonical(payload or {})])

    store = Store()
    journal = GolemValuationJournal(store)
    # This fixture targets its disposable application/environment only.
    journal._transport.address = agent
    read_batches = []
    prices = {"BTC-USD": "100000", "ETH-USD": "2000"}

    async def read(*, markets):
        read_batches.append(markets)
        result = fixtures.read_set(markets, millis())
        for envelope in result["markets"]:
            snapshot = envelope["snapshot"]
            snapshot["quote"]["bid"] = snapshot["quote"]["ask"] = prices[
                snapshot["quote"]["symbol"]
            ]
            envelope["snapshot_digest"] = digest(snapshot)
        return result

    fee = FeeBound(account, 100, "c" * 64, millis() + 3_600_000)

    def worker():
        return ValuationWorker(store, journal, read, fee)

    assert asyncio.run(worker().cycle())["state"] == "allocation_uninitialized"
    assert not read_batches
    command(
        "initialize_accounting",
        {
            "schema_version": 1,
            "account_key": account,
            "cash_micros": 100_000_000,
            "quantities": {"BTC-USD": "0.001", "ETH-USD": "0.01"},
            "settlement_quantum_micros": 10000,
            "source_digest": "b" * 64,
            "observed_ms": millis(),
        },
        "broker-reconciler",
    )
    first = asyncio.run(worker().cycle())
    assert first["state"] == "marked" and first["risk"]["equity_micros"] == 218_800_000
    assert first["risk"]["gross_exposure_micros"] == 120_000_000
    assert first["broker_requests"] == 6 and first["execution_authority"] is False
    print("Fresh BTC/ETH reads committed as conservative allocation equity.", flush=True)

    prices.update({"BTC-USD": "80000", "ETH-USD": "1500"})
    store.lost_mark_reply = True
    before_calls = store.mark_calls
    try:
        asyncio.run(worker().cycle())
    except ControlError:
        pass
    else:
        raise AssertionError("the synthetic lost acknowledgement was not propagated")
    assert store.mark_calls == before_calls + 1
    lost = command("risk_state")["risk"]
    assert lost["equity_micros"] == 194_050_000
    assert lost["peak_equity_micros"] == 218_800_000
    assert lost["day_loss_latched"] and lost["drawdown_latched"]
    server.connected(["agent", "simulate-crash", agent])
    restored = command("risk_state")["risk"]
    assert restored["valuation_digest"] == lost["valuation_digest"]
    # A new worker first reads the durable state, then values from a new read set.
    recovered = asyncio.run(worker().cycle())
    assert recovered["risk"]["day_open_equity_micros"] == 218_800_000
    assert recovered["risk"]["day_loss_latched"] and recovered["risk"]["drawdown_latched"]
    print("Lost acknowledgement and worker/agent restart preserved the loss baseline.", flush=True)

    order_id = str(uuid.uuid4())
    at = millis()
    order = {
        "account_key": account,
        "client_order_id": order_id,
        "symbol": "BTC-USD",
        "side": "buy",
        "quantity": "0.0001",
        "limit_price": "80000",
        "reserved_cash_micros": 10_000_000,
        "policy_revision": 1,
        "strategy_fingerprint": "1" * 64,
        "input_digest": "2" * 64,
        "risk_reservation_id": "3" * 64,
        "created_ms": at,
        "expires_ms": at + 120_000,
    }
    command("prepare", order, "executor")
    reserved = command("account_state")
    assert reserved["balance"]["available_cash_micros"] == 90_000_000
    before_reads = len(read_batches)
    assert asyncio.run(worker().cycle())["state"] == "order_reconciliation_required"
    assert len(read_batches) == before_reads
    prepared = asyncio.run(worker().cycle(client_order_id=order_id))
    assert prepared["state"] == "marked"
    assert prepared["prepared_order"]["client_order_id"] == order_id
    assert command("account_state") == reserved
    assert not command("risk_state")["risk"]["current"]
    valued = command("prepared_risk_account_snapshot", prepared["prepared_order"])
    assert valued["account"]["loss_stop_latched"]
    assert valued["prepared_order"] == prepared["prepared_order"]

    # Trigger a real journal snapshot before a hard server crash.
    for _ in range(12):
        command("status")
    before = command("status")
    server.hard_crash()
    server.start()
    assert command("status") == before
    assert command("account_state") == reserved
    after_restart = asyncio.run(worker().cycle(client_order_id=order_id))
    assert after_restart["risk"]["peak_equity_micros"] == 218_800_000
    assert after_restart["risk"]["day_loss_latched"]
    assert after_restart["risk"]["drawdown_latched"]
    assert command("account_state") == reserved
    print("Server restart preserved the reservation and scoped valuation history.", flush=True)

    # These are durable journal markers only. No broker adapter can submit from this verifier.
    command("mark_dispatch_started", {"client_order_id": order_id}, "executor")
    before_reads = len(read_batches)
    try:
        asyncio.run(worker().cycle(client_order_id=order_id))
    except AssertionError as exc:
        # The isolated CLI adapter rejects the kernel's InvalidPhase response.
        assert "operation is incompatible with the recorded order phase" in str(exc)
    else:
        raise AssertionError("a dispatched order retained its prepared valuation scope")
    assert len(read_batches) == before_reads
    return {
        "checks": [
            "all_holdings_valued",
            "no_implicit_allocation_initialization",
            "exact_equity_and_exposure",
            "lost_mark_reply_not_retried",
            "loss_baselines_survive_agent_restart",
            "prepared_reservation_preserved",
            "full_server_restart",
            "dispatched_scope_rejected_before_broker_read",
        ],
        "synthetic_read_batches": len(read_batches),
        "agent_crashes": 1,
        "server_hard_crashes": server.hard_crashes,
        "broker_requests": 0,
        "real_orders": 0,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--golem", default=shutil.which("golem"))
    parser.add_argument("--artifact", type=Path, default=support.ARTIFACT)
    parser.add_argument("--proof", type=Path, required=True)
    args = parser.parse_args()
    if not args.golem or not args.artifact.is_file():
        parser.error("the Golem CLI and built journal component are required")
    os.umask(0o077)
    proof = {
        "schema_version": 1,
        "passed": False,
        "started_ms": millis(),
        "artifact_sha256": hashlib.sha256(args.artifact.read_bytes()).hexdigest(),
        "scope": "Python valuation worker to isolated Golem; synthetic broker and control inputs",
    }
    with tempfile.TemporaryDirectory(prefix="helios-valuation-proof-") as directory:
        server = support.IsolatedGolem(args.golem, Path(directory), args.artifact.resolve())
        try:
            server.start()
            server.deploy()
            proof.update(verify(server))
            proof["passed"] = True
        except Exception as exc:
            proof["error_type"] = type(exc).__name__
            raise
        finally:
            server.close()
            proof["finished_ms"] = millis()
            support.private_write(args.proof, proof)
    print(json.dumps({"passed": True, "proof": str(args.proof), "checks": proof["checks"]}))


if __name__ == "__main__":
    main()
