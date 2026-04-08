"""
Explicit time source for reproducible backtests.

Library code must not call datetime.now() or date.today(). Production/live ingest
passes a SystemClock; tests and historical replays pass FrozenClock (or a custom Clock).
"""

from __future__ import annotations

from datetime import UTC, date, datetime, timedelta
from typing import Protocol, runtime_checkable


@runtime_checkable
class Clock(Protocol):
    """Source of "current" time for an ingest or simulation run."""

    def now_utc(self) -> datetime:
        """Wall-clock instant in UTC (aware)."""

    def today_utc(self) -> date:
        """Calendar date in UTC corresponding to the run's notion of "today"."""


class SystemClock:
    """Live clock: use only from production entrypoints (Hydra main, operators)."""

    def now_utc(self) -> datetime:
        return datetime.now(UTC)

    def today_utc(self) -> date:
        return self.now_utc().date()


class FrozenClock:
    """Fixed instant for backtests, CI, and deterministic replays."""

    def __init__(self, at: datetime) -> None:
        if at.tzinfo is None:
            at = at.replace(tzinfo=UTC)
        self._at = at.astimezone(UTC)

    def now_utc(self) -> datetime:
        return self._at

    def today_utc(self) -> date:
        return self._at.date()


def utc_today_offset(clock: Clock, days: int) -> date:
    return clock.today_utc() + timedelta(days=days)
