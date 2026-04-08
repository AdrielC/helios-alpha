"""
Time utilities: **Pendulum-first** for parsing, construction, and timezone-aware instants.

We still use stdlib ``datetime.date`` at boundaries where Polars/pandas/yfinance expect it.
Convert at edges with ``to_pandas_timestamp`` / ``pendulum.instance``.

See: https://pendulum.eustace.io/docs/#pandas-integration
"""

from __future__ import annotations

from datetime import date, datetime, timedelta
from typing import TypeAlias

import pandas as pd
import pendulum

PendulumDT: TypeAlias = pendulum.DateTime
PendulumDate: TypeAlias = pendulum.Date


def parse_iso_z(s: str | None) -> PendulumDT | None:
    """Parse ISO timestamps from APIs; returns UTC ``pendulum.DateTime``."""
    if not s:
        return None
    s = s.strip()
    if s.endswith("Z"):
        s = s[:-1] + "+00:00"
    return pendulum.parse(s, tz="UTC")


def parse_iso_to_pendulum(s: str | None) -> PendulumDT | None:
    return parse_iso_z(s)


def parse_date_iso(s: str) -> date:
    """Parse ``YYYY-MM-DD`` (Hydra, CLI) to a calendar ``date``."""
    return pendulum.parse(s, tz="UTC").date()


def utc_date(d: datetime | PendulumDT) -> date:
    return pendulum.instance(d).in_timezone("UTC").date()


def utc_datetime(
    year: int,
    month: int,
    day: int,
    hour: int = 0,
    minute: int = 0,
    second: int = 0,
    microsecond: int = 0,
) -> PendulumDT:
    """UTC instant (prefer this over ``datetime(..., tzinfo=UTC)``)."""
    return pendulum.datetime(year, month, day, hour, minute, second, microsecond, tz="UTC")


def start_of_utc_day(d: date) -> PendulumDT:
    return pendulum.datetime(d.year, d.month, d.day, 0, 0, 0, tz="UTC")


def start_of_next_utc_day(d: date) -> PendulumDT:
    return start_of_utc_day(d + timedelta(days=1))


def from_unix_epoch_seconds(epoch_s: float) -> PendulumDT:
    return pendulum.from_timestamp(epoch_s, tz="UTC")


def from_unix_epoch_ms(epoch_ms: float) -> PendulumDT:
    return pendulum.from_timestamp(epoch_ms / 1000.0, tz="UTC")


def to_pandas_timestamp(dt: PendulumDT | datetime) -> pd.Timestamp:
    p = pendulum.instance(dt).in_timezone("UTC")
    return pd.Timestamp(
        year=p.year,
        month=p.month,
        day=p.day,
        hour=p.hour,
        minute=p.minute,
        second=p.second,
        microsecond=p.microsecond,
        tz="UTC",
    )


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


def in_utc(dt: datetime | PendulumDT | None) -> PendulumDT | None:
    if dt is None:
        return None
    return pendulum.instance(dt).in_timezone("UTC")
