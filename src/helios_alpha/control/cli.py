from __future__ import annotations

import argparse
import json
from pathlib import Path

from helios_alpha.crypto.evidence import read_news_evidence
from helios_alpha.crypto.news_study import read_news_study
from helios_alpha.crypto.snapshots import (
    read_decision_input,
    read_decision_snapshots,
    replay_local_proposal,
)

from .broker_journal import read_broker_allocation, read_broker_allocation_risk, read_broker_journal
from .broker_snapshot import import_broker_observation, read_broker_observation
from .golem_store import GolemControlStore, read_private_json
from .settings import cached_settings, export_settings, mcp_connection
from .store import DEFAULT_ROOT


def main() -> None:
    parser = argparse.ArgumentParser(description="Private Golem-backed Helios controls")
    parser.add_argument("--root", type=Path, default=DEFAULT_ROOT)
    parser.add_argument("--client")
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("init")
    for name in ("status", "policy", "pause", "resume-paper"):
        commands.add_parser(name)
    settings = commands.add_parser("settings", help="Read the current settings from Golem")
    settings.add_argument("--cached", action="store_true", help="Inspection-only offline export")
    commands.add_parser("export-settings", help="Write a private settings.json inspection snapshot")
    commands.add_parser("broker-risk", help="Inspect durable allocation equity and loss history")
    commands.add_parser("mcp-config", help="Print local MCP JSON; defaults to the observer role")
    commands.add_parser(
        "broker-observation", help="Inspect a saved broker snapshot; never a live balance read"
    )
    journal = commands.add_parser(
        "broker-journal", help="Inspect durable Golem order recovery state; no broker requests"
    )
    journal.add_argument("--client-order-id", help="Read one exact recorded order identity")
    commands.add_parser(
        "broker-allocation",
        help="Inspect durable strategy balances; no broker requests or adoption",
    )
    broker_import = commands.add_parser(
        "import-broker-observation",
        help="Import private connected-MCP observations; does not authenticate or authorize orders",
    )
    broker_import.add_argument("path", type=Path)
    evidence = commands.add_parser(
        "news-evidence", help="Inspect forward news and decision evidence"
    )
    evidence.add_argument("--limit", type=int, default=20)
    evidence.add_argument("--before-sequence", type=int)
    study = commands.add_parser(
        "news-study", help="Summarize prospective news cohorts; not strategy profit or loss"
    )
    study.add_argument("--strategy-fingerprint")
    snapshots = commands.add_parser("decision-snapshots", help="List private local-proposal inputs")
    snapshots.add_argument("--limit", type=int, default=10)
    snapshots.add_argument("--before-sequence", type=int)
    inputs = commands.add_parser("decision-input", help="Read one original decision input")
    inputs.add_argument("snapshot_id")
    replay = commands.add_parser(
        "replay-decision", help="Run the archived private strategy locally; no model or fill replay"
    )
    replay.add_argument("snapshot_id")
    replay.add_argument("--fee-bps", type=int, help="Optional fee sensitivity scenario")
    args = parser.parse_args()
    if args.command == "settings" and args.cached:
        print(json.dumps(cached_settings(args.root), sort_keys=True, indent=2))
        return
    client = args.client or ("observer" if args.command == "mcp-config" else "codex-operator")
    store = GolemControlStore(args.root)
    if args.command == "init":
        result = store.initialize(read_private_json(store.root / "control-bootstrap.json"))
    elif args.command == "export-settings":
        result = export_settings(store, client)
    elif args.command == "mcp-config":
        result = mcp_connection(store, client)
    elif args.command == "broker-observation":
        result = read_broker_observation(store, client)
    elif args.command == "broker-journal":
        result = read_broker_journal(store, client, client_order_id=args.client_order_id)
    elif args.command == "broker-allocation":
        result = read_broker_allocation(store, client)
    elif args.command == "broker-risk":
        result = read_broker_allocation_risk(store, client)
    elif args.command == "import-broker-observation":
        result = import_broker_observation(store, client, read_private_json(args.path))
    elif args.command == "news-evidence":
        result = read_news_evidence(
            store, client, limit=args.limit, before_sequence=args.before_sequence
        )
    elif args.command == "news-study":
        result = read_news_study(store, client, strategy_fingerprint=args.strategy_fingerprint)
    elif args.command == "decision-snapshots":
        result = read_decision_snapshots(
            store, client, limit=args.limit, before_sequence=args.before_sequence
        )
    elif args.command in {"decision-input", "replay-decision"}:
        result = read_decision_input(store, client, args.snapshot_id)
        if args.command == "replay-decision":
            result = replay_local_proposal(store.root, result, fee_bps=args.fee_bps)
    else:
        result = getattr(store, args.command.replace("-", "_"))(client)
    print(json.dumps(result, sort_keys=True, indent=2))


if __name__ == "__main__":
    main()
