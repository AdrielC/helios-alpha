"""Capture official releases or replay a frozen, explicitly classified quote dataset."""

from __future__ import annotations

import argparse
import asyncio
import fcntl
import json
import os
import signal
import time
from pathlib import Path
from urllib.parse import urljoin, urlsplit

import httpx

from helios_alpha.control.broker_sync import write_private
from helios_alpha.control.golem_store import GolemControlStore
from helios_alpha.control.store import DEFAULT_ROOT, canonical, private_root

from . import decisions
from .lab import Journal, digest, settings
from .replay import ReplayDataset, ReplayPolicy, purged_folds, replay
from .sources import SOURCES, parse_source


def clock():
    return time.time_ns() // 1_000_000


async def fetch_source(client, url):
    allowed = {"ir.eia.gov", "ir2.eia.gov", "ftp.cpc.ncep.noaa.gov"}
    for _ in range(3):
        parsed = urlsplit(url)
        if parsed.scheme != "https" or parsed.hostname not in allowed:
            raise ValueError("unexpected source redirect")
        async with client.stream("GET", url) as response:
            if response.status_code in {301, 302, 303, 307, 308}:
                url = urljoin(url, response.headers["location"])
                continue
            response.raise_for_status()
            raw = bytearray()
            async for chunk in response.aiter_bytes():
                raw.extend(chunk)
                if len(raw) > 262144:
                    raise ValueError("source too large")
            return raw.decode("utf-8-sig")
    raise ValueError("too many source redirects")


async def collect(root, *, once=False, prepare=False):
    os.umask(0o077)
    root = private_root(root, create=True)
    lock = (root / "natgas-collector.lock").open("a+")
    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    journal = Journal(root)
    stopped = asyncio.Event()
    loop = asyncio.get_running_loop()
    for name in (signal.SIGTERM, signal.SIGINT):
        loop.add_signal_handler(name, stopped.set)
    try:
        config = settings(root)
        if prepare:
            write_private(root / "natgas-status.json", journal.snapshot(clock(), config))
            return
        async with httpx.AsyncClient(timeout=15, follow_redirects=False) as client:
            while not stopped.is_set():
                config = settings(root)

                async def read(source, url):
                    try:
                        text = await fetch_source(client, url)
                        journal.record(source, parse_source(source, text), text, clock())
                        journal.health(source, "receiving", clock())
                    except (httpx.HTTPError, ValueError, KeyError, StopIteration):
                        journal.health(source, "unavailable", clock())

                await asyncio.gather(*(read(source, url) for source, url in SOURCES.items()))
                snapshot = journal.snapshot(clock(), config)
                if config["decision_enabled"]:
                    store = GolemControlStore(root)
                    executable = (
                        Path(__file__).resolve().parents[3] / "rust/target/debug/helios-decision"
                    )
                    for candidate in snapshot["signals"]:
                        if not candidate["candidate"]:
                            continue
                        evidence = [
                            e
                            for e in journal.evidence("noaa-" + candidate["kind"], clock())
                            if e["id"] in candidate["evidence_ids"]
                        ]
                        await decisions.review(
                            journal, store, config, candidate, evidence, executable
                        )
                write_private(root / "natgas-status.json", journal.snapshot(clock(), config))
                if once:
                    break
                due = time.monotonic() + config["poll_seconds"]
                while not stopped.is_set() and time.monotonic() < due:
                    # Fresh heartbeat is separate from unchanged source observation times.
                    write_private(root / "natgas-status.json", journal.snapshot(clock(), config))
                    try:
                        await asyncio.wait_for(
                            stopped.wait(), min(5, max(0.01, due - time.monotonic()))
                        )
                    except TimeoutError:
                        pass
    finally:
        journal.close()
        lock.close()


def run_replay(dataset, config):
    if dataset.get("data_class") not in {"recorded", "synthetic"}:
        raise ValueError("declare recorded or synthetic data; OHLC is unsupported")
    if not isinstance(dataset.get("provenance"), str) or not dataset["provenance"].strip():
        raise ValueError("dataset provenance is required")
    parsed = ReplayDataset(**dataset)
    contracts, quotes, plans = parsed.contracts, parsed.quotes, parsed.plans
    report = replay(contracts, quotes, plans, ReplayPolicy(**config["replay"]))
    profiles = []
    for profile in config["exit_profiles"]:
        policy = ReplayPolicy(
            **{**config["replay"], **{k: v for k, v in profile.items() if k != "id"}}
        )
        comparison = replay(contracts, quotes, plans, policy)
        profiles.append(
            {
                "id": profile["id"],
                "policy": policy.model_dump(),
                **{
                    k: comparison[k]
                    for k in (
                        "complete",
                        "pnl_cents",
                        "closed_trades",
                        "unresolved_trades",
                        "max_observed_drawdown_cents",
                    )
                },
            }
        )
    report["exit_profile_sensitivity"] = profiles
    report["selection_warning"] = "Same-sample comparisons are not out-of-sample strategy evidence."
    windows = [
        {"event_id": t["event_id"], "start_ms": t["entry_ms"], "end_ms": t["exit_ms"]}
        for t in report["trades"]
        if t["exit_ms"] is not None
    ]
    report.update(
        data_class=dataset["data_class"],
        dataset_digest=digest(dataset),
        config_digest=digest(config),
        policy=config["replay"],
        provenance=dataset["provenance"],
        folds=purged_folds(windows),
        evaluation="descriptive replay; folds supplied for subsequent frozen validation",
    )
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["collect", "status", "replay", "schema"])
    parser.add_argument("--root", type=Path, default=DEFAULT_ROOT)
    parser.add_argument("--once", action="store_true")
    parser.add_argument("--prepare", action="store_true")
    parser.add_argument("--dataset", type=Path)
    args = parser.parse_args()
    if args.action == "schema":
        print(json.dumps(ReplayDataset.model_json_schema(), indent=2))
        return
    root = private_root(args.root, create=True)
    if args.action == "collect":
        asyncio.run(collect(root, once=args.once, prepare=args.prepare))
    elif args.action == "status":
        journal = Journal(root)
        try:
            print(canonical(journal.snapshot(clock(), settings(root))))
        finally:
            journal.close()
    else:
        if not args.dataset or args.dataset.stat().st_size > 64 * 1024 * 1024:
            parser.error("provide a bounded --dataset JSON file")
        dataset = json.loads(args.dataset.read_text())
        report = run_replay(dataset, settings(root))
        write_private(root / "natgas-backtest-latest.json", report)
        print(canonical(report))


if __name__ == "__main__":
    main()
