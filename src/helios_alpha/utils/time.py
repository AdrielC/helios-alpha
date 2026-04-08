from __future__ import annotations

from datetime import date, datetime, timedelta
from typing import TypeAlias

import pendulum

# Prefer Pendulum for parsing and calendar math; stdlib `date` still OK for API boundaries.
PendulumDT: TypeAlias = pendulum.DateTime
PendulumDate: TypeAlias = pendulum.Date


def parse_iso_z(s: str | None) -> datetime | None:
    """Parse ISO timestamps from APIs; returns timezone-aware UTC datetime (Pendulum DateTime)."""
    if not s:
        return None
    s = s.strip()
    if s.endswith("Z"):
        s = s[:-1] + "+00:00"
    return pendulum.parse(s, tz="UTC")


def parse_iso_to_pendulum(s: str | None) -> PendulumDT | None:
    if not s:
        return None
    s = s.strip()
    if s.endswith("Z"):
        s = s[:-1] + "+00:00"
    return pendulum.parse(s, tz="UTC")


def utc_date(d: datetime) -> date:
    return pendulum.instance(d).in_timezone("UTC").date()


def daterange_chunks(start: date, end: date, max_days: int) -> list[tuple[date, date]]:
    if start > end:
        return []
    out: list[tuple[date, date]] = []
    cur = start
    while cur <= end:
        chunk_end = min(cur + timedelta(days=max_days - 1), end)
        out.append((cur, chunk_end))
        cur = chunk_end + timedelta(days=1)
    return out


def pendulum_date(d: date | str | PendulumDate) -> PendulumDate:
    if isinstance(d, pendulum.Date):
        return d
    if isinstance(d, str):
        return pendulum.parse(d).date()
    return pendulum.date(d.year, d.month, d.day)


def pendulum_to_date(d: PendulumDate | date) -> date:
    if isinstance(d, date) and not isinstance(d, pendulum.Date):
        return d
    return date(d.year, d.month, d.day)
