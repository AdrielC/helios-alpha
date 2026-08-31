"""Export finite, versioned venue schedules for fail-closed execution services."""

from __future__ import annotations

import argparse
import hashlib
import json
from datetime import UTC, date, datetime, timedelta
from pathlib import Path
from typing import Any

import exchange_calendars as xc
import pandas as pd

SCHEMA_VERSION = 1
UNIX_EPOCH_DATE = date(1970, 1, 1)


def _epoch_seconds(value: pd.Timestamp) -> int:
    timestamp = value
    if timestamp.tzinfo is None:
        timestamp = timestamp.tz_localize("UTC")
    else:
        timestamp = timestamp.tz_convert("UTC")
    return int(timestamp.timestamp())


def _day_index(value: pd.Timestamp) -> int:
    return (value.date() - UNIX_EPOCH_DATE).days


def build_venue_schedule_manifest(
    exchange: str,
    start: date,
    end: date,
    *,
    generated_at: datetime,
) -> dict[str, Any]:
    """Build a deterministic manifest over a closed civil-date request range.

    ``generated_at`` is required so callers, tests, and deployment tooling control provenance
    rather than silently embedding the wall clock.
    """

    if end < start:
        msg = "venue schedule end must not precede start"
        raise ValueError(msg)
    if generated_at.tzinfo is None:
        msg = "generated_at must be timezone-aware"
        raise ValueError(msg)
    calendar = xc.get_calendar(exchange)
    start_ts = pd.Timestamp(start)
    end_ts = pd.Timestamp(end)
    labels = calendar.sessions_in_range(start_ts, end_ts)
    if labels.empty:
        msg = "venue schedule range contains no sessions"
        raise ValueError(msg)

    sessions: list[dict[str, Any]] = []
    for label in labels:
        open_at = calendar.session_open(label)
        close_at = calendar.session_close(label)
        break_start = calendar.session_break_start(label)
        break_end = calendar.session_break_end(label)
        breaks: list[dict[str, int]] = []
        if not pd.isna(break_start) and not pd.isna(break_end):
            breaks.append(
                {
                    "start_utc": _epoch_seconds(break_start),
                    "end_utc": _epoch_seconds(break_end),
                }
            )
        sessions.append(
            {
                "label": _day_index(label),
                "open_utc": _epoch_seconds(open_at),
                "close_utc": _epoch_seconds(close_at),
                "breaks": breaks,
            }
        )

    valid_from = datetime(start.year, start.month, start.day, tzinfo=UTC)
    valid_until_day = end + timedelta(days=1)
    valid_until = datetime(
        valid_until_day.year,
        valid_until_day.month,
        valid_until_day.day,
        tzinfo=UTC,
    )
    payload = {
        "schema_version": SCHEMA_VERSION,
        "venue": exchange,
        "timezone": str(calendar.tz),
        "source": "exchange_calendars",
        "source_version": xc.__version__,
        "generated_at_utc": int(generated_at.astimezone(UTC).timestamp()),
        "valid_from_utc": int(valid_from.timestamp()),
        "valid_until_utc": int(valid_until.timestamp()),
        "sessions": sessions,
    }
    canonical = json.dumps(payload, sort_keys=True, separators=(",", ":")).encode()
    source_sha256 = hashlib.sha256(canonical).hexdigest()
    return {
        "metadata": {
            key: value for key, value in payload.items() if key != "sessions"
        }
        | {"source_sha256": source_sha256},
        "sessions": sessions,
    }


def _parse_instant(value: str) -> datetime:
    parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
    if parsed.tzinfo is None:
        msg = "generated-at must include a UTC offset"
        raise ValueError(msg)
    return parsed


def main(argv: list[str] | None = None) -> int:
    """Export a content-hashed venue schedule for an execution service."""

    parser = argparse.ArgumentParser(description=main.__doc__)
    parser.add_argument("--exchange", required=True)
    parser.add_argument("--start", required=True, type=date.fromisoformat)
    parser.add_argument("--end", required=True, type=date.fromisoformat)
    parser.add_argument("--generated-at", required=True, type=_parse_instant)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args(argv)
    manifest = build_venue_schedule_manifest(
        args.exchange,
        args.start,
        args.end,
        generated_at=args.generated_at,
    )
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
