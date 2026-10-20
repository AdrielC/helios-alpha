"""Read-only Atlas price history using the strategy's public market-data adapter."""

from __future__ import annotations

import argparse
import asyncio
import fcntl
import os
import signal
import sqlite3
import time
from datetime import UTC, datetime
from pathlib import Path

from helios_alpha.control.golem_store import read_private_json
from helios_alpha.control.store import DEFAULT_ROOT, private_root
from helios_alpha.shadow.operator_projection import write_operator_projection

from .market import SYMBOLS
from .streaming import QuoteBook, watch_quotes


def stamp(ms):
    return (
        datetime.fromtimestamp(ms / 1000, UTC)
        .isoformat(timespec="milliseconds")
        .replace("+00:00", "Z")
    )


def series_id(symbol, metric):
    return f"coinbase-{symbol.lower()}-{metric}"


def descriptors(symbols):
    return [
        dict(
            id=series_id(symbol, metric),
            label=f"{symbol} {label}",
            shortLabel=f"{symbol} {label}",
            domain="market",
            unit=unit,
            precision=precision,
            color=color,
            render="line",
            provenance="Coinbase Exchange · level2_batch · reference data",
            sourceNames=["coinbase", symbol],
            freshness="Sampled every 2s",
            defaultVisible=False,
        )
        for symbol in symbols
        for metric, label, unit, precision, color in (
            ("mid", "midpoint", "USD", 2, "#75b5f5"),
            ("spread", "spread", "bps", 3, "#cfa86a"),
        )
    ]


class AtlasHistory:
    def __init__(self, path):
        self.db = sqlite3.connect(path)
        os.chmod(path, 0o600)
        self.db.executescript("""
          PRAGMA journal_mode=WAL;
          PRAGMA synchronous=FULL;
          CREATE TABLE IF NOT EXISTS samples(
            symbol TEXT NOT NULL, observed_ms INTEGER NOT NULL, received_ms INTEGER NOT NULL,
            bid INTEGER NOT NULL, ask INTEGER NOT NULL, PRIMARY KEY(symbol, observed_ms));
          CREATE TABLE IF NOT EXISTS meta(id INTEGER PRIMARY KEY CHECK(id=1), sequence INTEGER);
          INSERT OR IGNORE INTO meta VALUES(1,0);
        """)

    def sample(self, book, symbols, now):
        # Never restamp a cached quote or carry it across a disconnection.
        with self.db:
            for q in book.available_quotes(symbols, now, 5000):
                raw = book.latest[q["symbol"]]
                self.db.execute(
                    "INSERT OR IGNORE INTO samples VALUES(?,?,?,?,?)",
                    (
                        q["symbol"],
                        q["observed_ms"],
                        raw["received_ms"],
                        q["bid_micros"],
                        q["ask_micros"],
                    ),
                )
            self.db.execute("DELETE FROM samples WHERE observed_ms < ?", (now - 86_400_000,))

    def projection(self, symbols, now):
        # Durable sequence also advances through empty/disconnected observations.
        with self.db:
            self.db.execute("UPDATE meta SET sequence=sequence+1")
            sequence = self.db.execute("SELECT sequence FROM meta").fetchone()[0]
        series = []
        for symbol in symbols:
            rows = self.db.execute(
                "SELECT observed_ms,received_ms,bid,ask FROM samples "
                "WHERE symbol=? AND observed_ms<=? ORDER BY observed_ms",
                (symbol, now),
            ).fetchall()
            # Bound the wire to 5k points per series, retaining the latest point.
            stride = max(1, (len(rows) + 4998) // 4999)
            selected = rows[::stride]
            if rows and selected[-1] != rows[-1]:
                selected.append(rows[-1])
            for metric in ("mid", "spread"):
                series.append(
                    {
                        "id": series_id(symbol, metric),
                        "points": [
                            {
                                "kind": "scalar",
                                "timestamp": stamp(at),
                                "availableAt": stamp(received),
                                "value": (bid + ask) / 2_000_000
                                if metric == "mid"
                                else (ask - bid) * 20_000 / (bid + ask),
                            }
                            for at, received, bid, ask in selected
                        ],
                    }
                )
        return dict(
            schemaVersion=1,
            projectionId="coinbase-atlas",
            sequence=sequence,
            observedAt=stamp(now),
            series=series,
        )

    def close(self):
        self.db.close()


async def run(root, prepare=False):
    os.umask(0o077)
    root = private_root(root)
    config = read_private_json(root / "runtime.json")
    symbols = config["symbols"]
    if not symbols or len(symbols) != len(set(symbols)) or not set(symbols) <= SYMBOLS:
        raise ValueError("unsupported configured market symbols")
    lock = (root / "atlas-market.lock").open("a+")
    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    history = AtlasHistory(root / "atlas-market.sqlite3")
    projection = root / "atlas-market-projection.json"
    write_operator_projection(root / "atlas-market-catalog.json", descriptors(symbols))
    write_operator_projection(projection, history.projection(symbols, time.time_ns() // 1_000_000))
    stopped, changed, book = asyncio.Event(), asyncio.Event(), QuoteBook()
    task = None
    try:
        if prepare:
            return
        for sig in (signal.SIGINT, signal.SIGTERM):
            asyncio.get_running_loop().add_signal_handler(sig, stopped.set)
        task = asyncio.create_task(watch_quotes(book, symbols, changed, stopped))
        while not stopped.is_set():
            now = time.time_ns() // 1_000_000
            history.sample(book, symbols, now)
            write_operator_projection(projection, history.projection(symbols, now))
            try:
                await asyncio.wait_for(stopped.wait(), timeout=2)
            except TimeoutError:
                pass
    finally:
        if task:
            task.cancel()
            await asyncio.gather(task, return_exceptions=True)
        history.close()
        lock.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=DEFAULT_ROOT)
    parser.add_argument("--prepare", action="store_true")
    args = parser.parse_args()
    asyncio.run(run(args.root, args.prepare))


if __name__ == "__main__":
    main()
