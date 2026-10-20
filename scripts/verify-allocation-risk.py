#!/usr/bin/env python3
"""Exercise durable risk history on a disposable Golem server using synthetic balances."""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
import re
import tempfile
import uuid
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location(
    "journal_verifier", REPO / "scripts/verify-broker-journal.py"
)
assert spec and spec.loader
support = importlib.util.module_from_spec(spec)
spec.loader.exec_module(support)


def verify(server, previous: Path | None, artifact: Path, *, prepared_checks=False) -> dict:
    account = "a" * 64
    agent = f'{server.app}/test/CryptoBrokerJournalAgent("{account}")'
    bootstrap = {
        "schema_version": 1,
        "clients": {
            "observer": ["journal:read"],
            "reconciler": ["journal:read", "journal:reconcile"],
            "executor": ["journal:read", "journal:prepare", "journal:dispatch"],
        },
    }
    server.invoke(agent, "initialize", [support.canonical(bootstrap)])

    def command(action, payload=None, client="observer", *, denied=False, key=None):
        return server.invoke(
            agent,
            "command",
            [client, action, support.canonical(payload or {})],
            denied=denied,
            key=key,
        )

    seed = {
        "schema_version": 1,
        "account_key": account,
        "cash_micros": 100_000_000,
        "quantities": {"BTC-USD": "0.001"},
        "settlement_quantum_micros": 10_000,
        "source_digest": "b" * 64,
        "observed_ms": support.millis(),
    }
    command("initialize_accounting", seed, "reconciler")
    # This insertion order is CryptoRiskLimits' canonical Rust field order.
    limits = {
        "capital_micros": 500_000_000,
        "order_micros": 20_000_000,
        "daily_loss_micros": 10_000_000,
        "drawdown_micros": 15_000_000,
        "symbol_exposure_micros": 250_000_000,
        "max_daily_orders": 7,
        "max_spread_bps": 20,
        "max_quote_age_ms": 5_000,
        "symbols": ["BTC-USD", "ETH-USD"],
        "leverage": False,
    }
    policy = {
        "schema_version": 1,
        "revision": 1,
        "limits": limits,
        "sha256": hashlib.sha256(json.dumps(limits, separators=(",", ":")).encode()).hexdigest(),
        "updated_ns": 0,
    }

    def mark(price, prepared=None):
        state = command("risk_state")
        at = support.millis()
        payload = {
            "schema_version": 1,
            "account_key": account,
            "expected_balance_digest": state["balance_digest"],
            "policy": policy,
            "observed_ms": at,
            "source_digest": "c" * 64,
            "fee_reserve_bps": 100,
            "paused": False,
            "quotes": [
                {
                    "quote": {
                        "account_key": account,
                        "symbol": "BTC-USD",
                        "bid": price,
                        "ask": price,
                        "observed_ms": at,
                    },
                    "quantity": "0.001",
                    "source_digest": "d" * 64,
                }
            ],
        }
        if prepared is None:
            receipt = command("mark_allocation", payload, "reconciler")
        else:
            payload["expected_balance_digest"] = prepared["balance_digest"]
            receipt = command(
                "mark_prepared_allocation",
                {"scope": prepared["scope"], "valuation": payload},
                "reconciler",
            )
        assert receipt["risk"]["current"] and receipt["execution_authority"] is False
        return receipt, payload

    if prepared_checks:
        # The preceding component already supports ordinary risk marks; preserve a populated
        # history and a completed snapshot when adding order-specific reservation views.
        original_risk, _ = mark("100000")
        for _ in range(12):
            command("status")
    baseline = command("status")
    balance = command("account_state")
    previous_revision = json.loads(server.connected(["agent", "get", agent]))["metadata"][
        "componentRevision"
    ]
    if previous:
        manifest_path = server.root / "golem.yaml"
        manifest = json.loads(manifest_path.read_text())
        manifest["components"][server.component]["componentWasm"] = str(artifact)
        manifest_path.write_text(json.dumps(manifest))
        server.artifact = artifact
        server.deploy(force_build=True)
        try:
            server.connected(["--yes", "agent", "update", agent, "auto", "--await"])
        except RuntimeError:
            # Some CLI versions miss the completion event; inspect authoritative metadata.
            metadata = json.loads(server.connected(["agent", "get", agent]))["metadata"]
            if metadata["componentRevision"] <= previous_revision:
                raise
        metadata = json.loads(server.connected(["agent", "get", agent]))["metadata"]
        assert metadata["componentRevision"] > previous_revision, metadata
        assert command("status") == baseline
        assert command("account_state") == balance
    if prepared_checks:
        assert (
            command("risk_state")["risk"]["valuation_digest"]
            == original_risk["risk"]["valuation_digest"]
        )
    else:
        assert command("risk_state")["risk"] is None
    print("Existing allocation and risk history preserved through component update.", flush=True)

    first, _ = mark("100000")
    assert first["risk"]["equity_micros"] == 199_000_000
    loss, payload = mark("80000")
    assert loss["risk"]["day_loss_latched"] and loss["risk"]["drawdown_latched"]
    # Reusing the same idempotency key returns the original receipt, not fresh authority.
    key = str(uuid.uuid4())
    duplicate = command("mark_allocation", payload, "reconciler", key=key)
    assert duplicate["changed"] is False
    before = command("status")
    command("mark_allocation", payload, "observer", denied=True)
    command("mark_allocation", payload, "executor", denied=True)
    assert command("status") == before
    server.connected(["agent", "simulate-crash", agent])
    assert command("status") == before
    assert command("mark_allocation", payload, "reconciler", key=key) == duplicate
    restored = command("risk_state")["risk"]
    assert restored["day_loss_latched"] and restored["drawdown_latched"]
    recovered, _ = mark("110000")
    assert recovered["risk"]["day_loss_latched"] and recovered["risk"]["drawdown_latched"]
    snapshot = command("risk_account_snapshot", {"symbol": "BTC-USD"})
    assert snapshot["account"]["loss_stop_latched"] and snapshot["execution_authority"] is False
    print("Loss history and permission boundaries survived agent crash.", flush=True)

    now = support.millis()
    client_id = str(uuid.uuid4())
    order = {
        "account_key": account,
        "client_order_id": client_id,
        "symbol": "BTC-USD",
        "side": "buy",
        "quantity": "0.001",
        "limit_price": "100",
        "reserved_cash_micros": 1_000_000,
        "policy_revision": 1,
        "strategy_fingerprint": "1" * 64,
        "input_digest": "2" * 64,
        "risk_reservation_id": "3" * 64,
        "created_ms": now,
        "expires_ms": now + 120_000,
    }
    command("prepare", order, "executor")
    assert not command("risk_state")["risk"]["current"]
    command("risk_account_snapshot", {"symbol": "BTC-USD"}, denied=True)
    if prepared_checks:
        reserved = command("account_state")
        prepared = command("prepared_allocation", {"client_order_id": client_id})
        assert prepared["balance"]["available_cash_micros"] == 100_000_000
        assert reserved["balance"]["available_cash_micros"] == 99_000_000
        _, payload = mark("110000", prepared)
        before = command("status")
        command(
            "mark_prepared_allocation",
            {
                "scope": prepared["scope"],
                "valuation": payload,
            },
            "observer",
            denied=True,
        )
        assert command("status") == before
        valued = command("prepared_risk_account_snapshot", prepared["scope"])
        assert valued["prepared_order"] == prepared["scope"]
        assert valued["account"]["loss_stop_latched"]
        command("risk_account_snapshot", {"symbol": "BTC-USD"}, denied=True)
        assert command("account_state") == reserved
        server.connected(["agent", "simulate-crash", agent])
        assert command("status") == before
        for _ in range(12):
            command("status")
        server.hard_crash()
        server.start()
        assert command("status") == before
        assert command("account_state") == reserved
        assert command("prepared_allocation", {"client_order_id": client_id}) == prepared
        mark("110000", prepared)
        print(
            "Prepared-order reservation and loss history survived agent/server crashes.", flush=True
        )
    command("mark_dispatch_started", {"client_order_id": client_id}, "executor")
    if prepared_checks:
        command("prepared_allocation", {"client_order_id": client_id}, denied=True)
        command("prepared_risk_account_snapshot", prepared["scope"], denied=True)
    now = support.millis()
    report = {
        "account_key": account,
        "client_order_id": client_id,
        "broker_order_id": str(uuid.uuid4()),
        "symbol": "BTC-USD",
        "side": "buy",
        "ordered_quantity": "0.001",
        "limit_price": "100",
        "state": "Failed",
        "filled_quantity": "0",
        "fee_usd": "0",
        "executions": [],
        "updated_ms": now,
        "received_ms": now,
    }
    observed = command("observe", report, "reconciler")
    command(
        "reconcile_terminal",
        {
            "client_order_id": client_id,
            "observation_digest": observed["record"]["observation_digest"],
        },
        "reconciler",
    )
    counted, _ = mark("110000")
    assert counted["risk"]["daily_order_count"] == 1
    for _ in range(12):
        command("status")
    oplog = server.connected(["agent", "oplog", agent], output_format="text")
    snapshots = re.findall(r"(?m)^#\d+:\s+SNAPSHOT\b", oplog)
    assert snapshots, "No completed Golem snapshot"
    final = command("status")
    server.hard_crash()
    server.start()
    assert command("status") == final
    risk = command("risk_state")["risk"]
    assert risk["daily_order_count"] == 1
    assert risk["day_loss_latched"] and risk["drawdown_latched"]
    assert risk["valuation_digest"] == counted["risk"]["valuation_digest"]
    assert risk["peak_equity_micros"] == 208_900_000
    print(
        "Snapshot recovery retained losses and failed-order count after server restart.", flush=True
    )
    return {
        "status": "passed",
        "legacy_allocation_update_verified": previous is not None,
        "completed_snapshots": len(snapshots),
        "agent_crashes": 2 if prepared_checks else 1,
        "prepared_order_checks": prepared_checks,
        "populated_risk_update_verified": bool(previous and prepared_checks),
        "hard_server_restarts": server.hard_crashes,
        "broker_requests": 0,
        "real_orders": 0,
        "execution_authority": False,
        "final_snapshot_digest": final["snapshot_digest"],
        "final_risk": risk,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--golem", default=str(Path.home() / ".local/bin/golem"))
    parser.add_argument("--artifact", type=Path, default=support.ARTIFACT)
    parser.add_argument("--previous-artifact", type=Path)
    parser.add_argument(
        "--prepared-checks",
        action="store_true",
        help="Also test order-specific views; preceding artifact must support ordinary risk marks",
    )
    parser.add_argument("--proof", type=Path, required=True)
    args = parser.parse_args()
    os.umask(0o077)
    with tempfile.TemporaryDirectory(prefix="helios-risk-proof-") as name:
        server = support.IsolatedGolem(
            args.golem,
            Path(name),
            args.previous_artifact or args.artifact,
            app="helios-allocation-risk-proof",
        )
        try:
            server.start()
            server.deploy()
            proof = verify(
                server, args.previous_artifact, args.artifact, prepared_checks=args.prepared_checks
            )
            proof["artifact_sha256"] = hashlib.sha256(args.artifact.read_bytes()).hexdigest()
            if args.previous_artifact:
                proof["previous_artifact_sha256"] = hashlib.sha256(
                    args.previous_artifact.read_bytes()
                ).hexdigest()
            support.private_write(args.proof, proof)
        finally:
            server.close()
    print(json.dumps({"status": "passed", "proof": str(args.proof)}))


if __name__ == "__main__":
    main()
