from __future__ import annotations

import argparse
import json
import sys
import time
from dataclasses import asdict
from datetime import UTC, datetime
from pathlib import Path

from helios_alpha.shadow.adapters import DEFAULT_SHADOW_FEEDS, HttpxJsonTransport
from helios_alpha.shadow.journal import ShadowJournal
from helios_alpha.shadow.operator_projection import (
    build_operator_projection,
    write_operator_projection,
)
from helios_alpha.shadow.service import ShadowIngestionService
from helios_alpha.timekeeping import SystemClock


def parser() -> argparse.ArgumentParser:
    command = argparse.ArgumentParser(description="Run point-in-time NOAA/NASA shadow ingestion")
    command.add_argument(
        "--journal",
        type=Path,
        default=Path("data/shadow/space-weather.sqlite3"),
        help="durable append-only SQLite journal",
    )
    command.add_argument(
        "--operator-projection",
        type=Path,
        help="atomically write a generic helio-operatord projection after every cycle",
    )
    command.add_argument(
        "--projection-id",
        default="scientific-shadow",
        help="stable identity used for monotonic operator projection updates",
    )
    command.add_argument(
        "--max-points-per-series",
        type=int,
        default=5_000,
        help="bounded number of current points exported for each series",
    )
    command.add_argument(
        "--feed",
        action="append",
        choices=[feed.source_id for feed in DEFAULT_SHADOW_FEEDS],
        help="poll only the selected feed; repeat for more than one",
    )
    command.add_argument(
        "--interval-seconds",
        type=float,
        default=0.0,
        help="poll forever at this interval; zero runs one cycle",
    )
    return command


def _json_receipt(value: object) -> str:
    return json.dumps(value, sort_keys=True, default=lambda item: item.isoformat())


def main() -> None:
    args = parser().parse_args()
    if args.interval_seconds < 0:
        raise SystemExit("--interval-seconds must not be negative")
    selected = set(args.feed or [])
    feeds = [feed for feed in DEFAULT_SHADOW_FEEDS if not selected or feed.source_id in selected]

    clock = SystemClock()
    with ShadowJournal(args.journal) as journal, HttpxJsonTransport(clock) as transport:
        service = ShadowIngestionService(journal, transport)
        while True:
            failures = 0
            now = clock.now_utc()
            cycle_time = datetime.fromtimestamp(now.timestamp(), tz=UTC)
            for feed in feeds:
                try:
                    receipt = service.poll(feed, cycle_time)
                    print(_json_receipt({"status": "ok", **asdict(receipt)}), flush=True)
                except Exception as error:
                    failures += 1
                    print(
                        _json_receipt(
                            {
                                "status": "error",
                                "source_id": feed.source_id,
                                "error": str(error),
                            }
                        ),
                        file=sys.stderr,
                        flush=True,
                    )
            if args.operator_projection:
                projection = build_operator_projection(
                    journal,
                    projection_id=args.projection_id,
                    max_points_per_series=args.max_points_per_series,
                )
                write_operator_projection(args.operator_projection, projection)
                print(
                    _json_receipt(
                        {
                            "status": "projection",
                            "projection_id": projection["projectionId"],
                            "sequence": projection["sequence"],
                            "series": len(projection["series"]),
                            "path": str(args.operator_projection),
                        }
                    ),
                    flush=True,
                )
            if args.interval_seconds == 0:
                raise SystemExit(1 if failures else 0)
            time.sleep(args.interval_seconds)


if __name__ == "__main__":
    main()
