"""Plan, estimate, archive and normalize historical option NBBO for causal replay.

Run with --help for commands. Nothing here can place an order. Downloads require
an explicit reviewed estimate and approved amount; imports and exports are offline.

Discovery: query --schema definition --stype-in parent --symbols UNG.OPT, then
estimate, fetch, catalog. Verify deliverables and freeze the selected contracts
using the JSON shape from `schema contracts`. Quotes: query with exact padded OCC
symbols, estimate, fetch, import with --contracts and optional --plans. Replay:
python -m helios_alpha.natgas.cli replay --dataset <imported-dataset.json>.
Keep raw CSV settings: map_symbols=true, pretty_px=false, pretty_ts=false.
"""

from __future__ import annotations

import argparse
import getpass
import json
import os
import sqlite3
import warnings
from pathlib import Path

from helios_alpha.control.broker_sync import write_private
from helios_alpha.control.store import DEFAULT_ROOT, canonical, private_root

from .databento import DatabentoHistory, HistoryError, load_key
from .history import (
    ContractManifest,
    HistoryQuery,
    archive_blob,
    bounded_read,
    csv_rows,
    expiry_close_ms,
    import_quotes,
    integer,
    raw_contract,
)
from .replay import Contract, Plan, Quote, ReplayDataset


def read_json(path):
    return json.loads(bounded_read(path))


def plans_file(path):
    if path is None:
        return []
    rows = read_json(path)
    if not isinstance(rows, list) or len(rows) > 100000:
        raise ValueError("expected a bounded list of frozen plans")
    return [Plan(**p) for p in rows]


def discover_contracts(raw, query):
    """A catalog is not a selected strategy universe or verified deliverable manifest."""
    if query.record_schema != "definition":
        raise ValueError("contract discovery requires a definition query")
    from .history import timestamp_ns

    start, end = timestamp_ns(query.start), timestamp_ns(query.end)
    candidates, count = {}, 0
    for _, row in csv_rows(raw, {"raw_symbol", "ts_recv", "ts_event", "rtype"}, query.record_limit):
        count += 1
        if integer(row["rtype"]) != 19:
            raise ValueError("unexpected definition record type")
        received = integer(row["ts_recv"])
        if not start <= received < end:
            raise ValueError("definition outside query window")
        symbol = row["raw_symbol"]
        try:
            expiry, kind, strike = raw_contract(symbol)
        except ValueError:
            continue  # Adjusted roots cannot enter the standard-contract replay.
        multiplier = row.get("contract_multiplier", "")
        candidate = {
            "id": symbol,
            "symbol": "UNG",
            "kind": kind,
            "strike_cents": strike,
            "expiry": expiry,
            "available_ms": (received + 999999) // 1000000,
            "close_by_ms": expiry_close_ms(expiry),
            "provider_multiplier": int(multiplier) if multiplier == "100" else None,
        }
        existing = candidates.get(symbol)
        if existing is None or candidate["available_ms"] < existing["available_ms"]:
            candidates[symbol] = candidate
    if count >= query.record_limit:
        raise ValueError("definition record cap reached; narrow the query")
    return {
        "schema_version": 1,
        "query": query.model_dump(),
        "records": count,
        "candidates": sorted(candidates.values(), key=lambda c: c["id"]),
        "limitation": "Verify deliverables and freeze selection before creating a manifest.",
    }


def export_capture(root, plans):
    """Consistent read-only snapshot; never backdate current contract/evidence availability."""
    root = private_root(root)
    path = root / "natgas.sqlite3"
    if path.is_symlink() or not path.is_file():
        raise ValueError("no local option capture journal")
    with sqlite3.connect(path.as_uri() + "?mode=ro", uri=True) as connection:
        connection.execute("BEGIN")
        contracts = [
            Contract(**json.loads(row[0]))
            for row in connection.execute("SELECT data FROM ng_contracts ORDER BY id LIMIT 10001")
        ]
        quotes = [
            Quote(**json.loads(row[0]))
            for row in connection.execute(
                "SELECT data FROM ng_quotes ORDER BY available_ms,observed_ms,id LIMIT 1000001"
            )
        ]
    dataset = ReplayDataset(
        data_class="recorded",
        provenance="Helios local Robinhood option capture; actual first-seen availability, "
        "sampled quotes, not tick-complete OPRA history. Consistent read-only export.",
        contracts=contracts,
        quotes=quotes,
        plans=plans,
    )
    if not quotes:
        raise ValueError("local capture has no quotes yet")
    destination = archive_blob(root, canonical(dataset.model_dump()).encode(), "json")
    return {
        "dataset": str(destination),
        "contracts": len(contracts),
        "quotes": len(quotes),
        "first_observed_ms": min(q.observed_ms for q in quotes),
        "last_observed_ms": max(q.observed_ms for q in quotes),
        "plans": len(plans),
        "ready_for_replay": bool(plans),
        "profitability_established": False,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=DEFAULT_ROOT)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("setup", help="Save a Databento API key locally with hidden input")
    schema = commands.add_parser("schema", help="Show a query or contract-manifest JSON schema")
    schema.add_argument("kind", choices=["query", "contracts"])
    plan = commands.add_parser("query", help="Save a bounded query; makes no provider request")
    plan.add_argument("--start", required=True)
    plan.add_argument("--end", required=True)
    plan.add_argument("--symbols", nargs="+", required=True, help="Exact padded OCC symbols")
    plan.add_argument("--schema", choices=["cmbp-1", "definition"], default="cmbp-1")
    plan.add_argument("--stype-in", choices=["raw_symbol", "parent"], default="raw_symbol")
    plan.add_argument("--record-limit", type=int, default=250000)
    estimate = commands.add_parser("estimate", help="Read free provider cost/record-count metadata")
    estimate.add_argument("--query", type=Path, required=True)
    fetch = commands.add_parser("fetch", help="Billed download; no automatic retries")
    fetch.add_argument("--estimate", type=Path, required=True)
    fetch.add_argument(
        "--approve-usd", required=True, help="Reviewed estimate ceiling, not a provider cap"
    )
    catalog = commands.add_parser(
        "catalog", help="Read archived definitions for historical candidates"
    )
    catalog.add_argument("--query", type=Path, required=True)
    catalog.add_argument("--csv", type=Path, required=True)
    ingest = commands.add_parser("import", help="Offline import of raw Databento cmbp-1 CSV")
    ingest.add_argument("--query", type=Path, required=True)
    ingest.add_argument("--csv", type=Path, required=True)
    ingest.add_argument("--contracts", type=Path, required=True)
    ingest.add_argument(
        "--plans", type=Path, help="Frozen timestamped plans; omission imports quotes only"
    )
    ingest.add_argument("--feed-latency-ms", type=int, default=0)
    capture = commands.add_parser(
        "export-capture", help="Archive locally recorded quotes for replay"
    )
    capture.add_argument("--plans", type=Path)
    args = parser.parse_args()
    os.umask(0o077)
    try:
        if args.command == "schema":
            cls = HistoryQuery if args.kind == "query" else ContractManifest
            print(json.dumps(cls.model_json_schema(), indent=2))
            return
        root = private_root(args.root, create=True)
        result = None
        if args.command == "setup":
            print("Create a key at https://databento.com/portal/keys. Input is hidden.")
            with warnings.catch_warnings():
                warnings.simplefilter("error", getpass.GetPassWarning)
                key = getpass.getpass("DATABENTO_API_KEY: ").strip()
            if not 8 <= len(key) <= 4096 or any(c.isspace() for c in key):
                raise HistoryError("invalid API key")
            write_private(root / "market-data-credentials.json", {"DATABENTO_API_KEY": key})
            result = {"credential_saved": True, "connection_verified": False}
        elif args.command == "query":
            query = HistoryQuery(
                start=args.start,
                end=args.end,
                symbols=args.symbols,
                record_schema=args.schema,
                stype_in=args.stype_in,
                record_limit=args.record_limit,
            )
            path = archive_blob(root, canonical(query.model_dump()).encode(), "json")
            result = {"query": str(path), "provider_request_made": False}
        elif args.command in {"estimate", "fetch"}:
            client = DatabentoHistory(load_key(root))
            try:
                if args.command == "estimate":
                    result = client.estimate(HistoryQuery(**read_json(args.query)))
                    path = archive_blob(root, canonical(result).encode(), "json")
                    result = {**result, "estimate_file": str(path)}
                else:
                    original, receipt = client.fetch(
                        root, read_json(args.estimate), args.approve_usd
                    )
                    result = {**receipt, "csv": str(original)}
            finally:
                client.close()
        elif args.command == "catalog":
            raw = bounded_read(args.csv)
            original = archive_blob(root, raw, "csv")
            result = discover_contracts(raw, HistoryQuery(**read_json(args.query)))
            result["raw_sha256"] = original.stem
            path = archive_blob(root, canonical(result).encode(), "json")
            result = {"catalog": str(path), "candidates": len(result["candidates"])}
        elif args.command == "import":
            destination, receipt, result = import_quotes(
                root,
                bounded_read(args.csv),
                HistoryQuery(**read_json(args.query)),
                ContractManifest(**read_json(args.contracts)),
                plans_file(args.plans),
                feed_latency_ms=args.feed_latency_ms,
            )
            result = {**result, "dataset": str(destination), "receipt": str(receipt)}
        else:
            result = export_capture(root, plans_file(args.plans))
        print(canonical(result))
    except HistoryError as exc:
        parser.exit(1, f"{exc}\n")
    except (ValueError, OSError, KeyError, TypeError, getpass.GetPassWarning):
        # Provider/Pydantic exceptions can embed secrets or input payloads. Keep stdout safe.
        parser.exit(
            1,
            "Historical data operation failed. Check private inputs, query bounds, "
            "credential setup and download receipts. No automatic retry was made.\n",
        )


if __name__ == "__main__":
    main()
