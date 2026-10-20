#!/usr/bin/env python3
"""Verify news deadlines and optional snapshot migration on an isolated Golem server."""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
import shutil
import tempfile
import time
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
ARTIFACT = REPO / "golem/golem-temp/agents/helios_hypothesis_shard_debug.wasm"
PROOF = Path.home() / ".local/share/helios-alpha/private/signal-deadline-golem-proof.json"
spec = importlib.util.spec_from_file_location(
    "journal_proof", REPO / "scripts/verify-broker-journal.py"
)
helper = importlib.util.module_from_spec(spec)
spec.loader.exec_module(helper)


def invoke(server, agent, method, arguments, *, denied=False):
    response = json.loads(
        server.connected(
            [
                "agent",
                "invoke",
                "--no-stream",
                agent,
                method,
                *[json.dumps(argument) for argument in arguments],
            ]
        )
    )
    value = response["result_json"]["value"]
    assert set(value) == ({"err"} if denied else {"ok"}), value
    return value["err"] if denied else json.loads(value["ok"])


def verify(server, artifact, previous, update_mode, snapshot_before_update):
    agent = f'{server.app}/test/CryptoControlAgent("synthetic-deadline")'

    def command(action, payload=None, *, denied=False, actor="test"):
        return invoke(
            server,
            agent,
            "command",
            [actor, action, helper.canonical(payload or {})],
            denied=denied,
        )

    limits = {
        "capital_micros": 300_000_000,
        "order_micros": 30_000_000,
        "daily_loss_micros": 20_000_000,
        "drawdown_micros": 60_000_000,
        "symbol_exposure_micros": 180_000_000,
        "max_daily_orders": 12,
        "max_spread_bps": 50,
        "max_quote_age_ms": 5000,
        "symbols": ["BTC-USD", "ETH-USD"],
        "leverage": False,
    }
    bootstrap = {
        "limits": limits,
        "clients": {
            "test": ["risk:read", "status:read", "runtime:write", "decisions:reserve"],
            "observer": ["status:read"],
        },
        "decision_daily_calls": 3,
    }
    invoke(server, agent, "initialize", [helper.canonical(bootstrap)])
    initial = command(
        "initialize_paper",
        {
            "initial_cash_micros": 300_000_000,
            "fee_bps": 50,
            "slippage_bps": 10,
            "minimum_order_micros": 1_000_000,
        },
    )
    assert "signal_deadline_version" not in initial
    if previous is None:
        assert command("enable_paper_signal_deadlines")["signal_deadline_version"] == 1

    def step(sequence, *, signal=None, target=30_000_000, symbol="BTC-USD"):
        at = helper.millis()
        return {
            "source_id": "synthetic-deadline-stream",
            "source_sequence": sequence,
            "expected_cursor": sequence - 1,
            "expected_policy_revision": 1,
            "event_id": f"synthetic-{sequence}",
            "available_ms": at,
            "quote": {
                "symbol": symbol,
                "bid_micros": 100_000_000,
                "ask_micros": 100_010_000,
                "observed_ms": at,
            },
            "portfolio_quotes": [],
            "target_micros": target,
            "observation_only": False,
            **({"signal": signal} if signal is not None else {}),
        }

    now = helper.millis()
    signal = {"input_digest": "a" * 64, "available_ms": now, "expires_ms": now + 120_000}
    entry = step(1, signal=signal if previous is None else None)
    first = command("paper_step", entry)
    assert first["outcome"] == "simulated_fill", first
    assert command("reserve_decision", {"request_id": "synthetic-once"})["send_allowed"]
    before = command("status")
    checks = []

    if previous is not None:
        if snapshot_before_update:
            print("Waiting for the component's periodic snapshot boundary.", flush=True)
            time.sleep(31)
            command("status")
            history = server.connected(
                ["agent", "oplog", agent, "--query", "snapshot"], output_format="text"
            )
            assert "\nSNAPSHOT\n" in history, history
            checks.append("existing periodic snapshot is present before component update")
        manifest_path = server.root / "golem.yaml"
        metadata_command = [
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
        old_revision = json.loads(server.run(metadata_command))["componentRevision"]
        manifest = json.loads(manifest_path.read_text())
        manifest["components"][server.component]["componentWasm"] = str(artifact)
        manifest_path.write_text(json.dumps(manifest))
        # A different prebuilt componentWasm path can reuse the previous metadata output.
        # Force composition and verify the deployed revision before touching an agent.
        server.deploy(force_build=True)
        revision = json.loads(server.run(metadata_command))["componentRevision"]
        assert revision > old_revision, "candidate component was not deployed"
        server.connected(["--yes", "agent", "update", agent, update_mode, str(revision), "--await"])
        after = command("status")
        assert after["paper"] == before["paper"], (before["paper"], after["paper"])
        assert after["policy_revision"] == before["policy_revision"]
        assert after["decision_budget"] == before["decision_budget"]
        assert command("paper_step", entry) == first
        checks.append("snapshot update preserves legacy fill, cursor, policy and decision budget")
        expected_paper = {**before["paper"], "signal_deadline_version": 1}
        command("enable_paper_signal_deadlines", actor="observer", denied=True)
        assert command("status")["paper"] == before["paper"]
        assert command("enable_paper_signal_deadlines") == expected_paper
        assert command("enable_paper_signal_deadlines") == expected_paper
        assert command("paper_step", entry) == first
        checks.append("one-way deadline activation preserves legacy receipts and rejects observers")

    # Fresh quote and queue-delivery timestamps must not extend the original news lifetime.
    now = helper.millis()
    expired = {
        "input_digest": "b" * 64,
        "available_ms": now - 120_001,
        "expires_ms": now - 1,
    }
    queued = step(2, signal=expired, target=60_000_000)
    balances = command("status")["paper"]
    stale = command("paper_step", queued)
    assert stale["outcome"] == "stale_signal" and stale["cursor"] == 2, stale
    server.connected(["agent", "simulate-crash", agent])
    assert command("paper_step", queued) == stale
    changed = json.loads(json.dumps(queued))
    changed["signal"]["expires_ms"] = helper.millis() + 1000
    command("paper_step", changed, denied=True)
    after = command("status")["paper"]
    for field in ("cash_micros", "positions", "total_orders", "fees_micros"):
        assert after[field] == balances[field], field
    checks.append("expired signal with fresh quote is consumed without buying across crash replay")
    checks.append("changing the deadline cannot replace a previously consumed input")

    missing = command("paper_step", step(3, target=60_000_000))
    assert missing["outcome"] == "missing_signal_validity", missing
    checks.append("legacy queued entry without validity is consumed without increasing inventory")

    # Exits only need a fresh execution quote; expired news does not prevent reducing risk.
    exited = command("paper_step", step(4, signal=expired, target=0))
    assert exited["outcome"] == "simulated_fill", exited
    final = command("status")
    assert final["paper"]["total_orders"] == 2
    assert final["paper"]["positions"]["BTC-USD"] == 0
    assert [f["side"] for f in final["paper"]["recent_fills"]] == ["buy", "sell"]
    command("paper_step", step(5, signal=signal), actor="observer", denied=True)
    assert command("status")["paper"] == final["paper"]
    assert not command("reserve_decision", {"request_id": "synthetic-once"})["send_allowed"]
    checks.append("local exit and permission checks remain intact")
    server.hard_crash()
    server.start()
    assert command("status")["paper"] == final["paper"]
    assert command("paper_step", queued)["outcome"] == "already_consumed"
    checks.append("hard server restart preserves fills and does not replay a consumed entry")
    return {
        "checks": checks,
        "snapshot_migration_tested": previous is not None,
        "update_mode": update_mode if previous is not None else None,
        "periodic_snapshot_before_update": snapshot_before_update,
        "simulated_fills": 2,
        "agent_crashes": 1,
        "hard_server_crashes": server.hard_crashes,
        "live_orders": 0,
        "paid_api_calls": 0,
        "main_runtime_touched": False,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--golem", default=shutil.which("golem"))
    parser.add_argument("--artifact", type=Path, default=ARTIFACT)
    parser.add_argument("--previous-artifact", type=Path)
    parser.add_argument("--update-mode", choices=["automatic", "manual"], default="automatic")
    parser.add_argument("--snapshot-before-update", action="store_true")
    parser.add_argument("--proof", type=Path, default=PROOF)
    args = parser.parse_args()
    os.umask(0o077)
    assert args.golem, "Golem executable is required"
    artifact = args.artifact.resolve(strict=True)
    previous = args.previous_artifact.resolve(strict=True) if args.previous_artifact else None

    def digest(path):
        return hashlib.sha256(path.read_bytes()).hexdigest()

    with tempfile.TemporaryDirectory(prefix="helios-signal-proof-") as directory:
        server = helper.IsolatedGolem(
            args.golem,
            Path(directory),
            previous or artifact,
            app="helios-signal-proof",
            component="helios:hypothesis-shard",
        )
        try:
            server.start()
            print("Isolated Golem ready; verifying original signal deadlines.", flush=True)
            server.deploy()
            result = verify(
                server, artifact, previous, args.update_mode, args.snapshot_before_update
            )
        finally:
            server.close()
    proof = {
        "schema_version": 1,
        "passed": True,
        "verified_ms": helper.millis(),
        "artifact_sha256": digest(artifact),
        "previous_artifact_sha256": digest(previous) if previous else None,
        **result,
    }
    helper.private_write(args.proof, proof)
    print(json.dumps(proof, indent=2))


if __name__ == "__main__":
    main()
