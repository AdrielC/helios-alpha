"""
Explicit time source for reproducible backtests.

Library code must not call ``pendulum.now`` or ``datetime.now`` except inside
``SystemClock``. Use ``FrozenClock`` for backtests and ``as_of`` dates on the pipeline.
"""

from __future__ import annotations

from datetime import date
from typing import Protocol, runtime_checkable

import pendulum


@runtime_checkable
class Clock(Protocol):
    """Source of "current" time for an ingest or simulation run."""

    def now_utc(self) -> pendulum.DateTime:
        """Wall-clock instant in UTC."""

    def today_utc(self) -> date:
        """UTC calendar date for the run's notion of "today"."""


class SystemClock:
    """Live clock: use only from production entrypoints (Hydra CLI, operators)."""

    def now_utc(self) -> pendulum.DateTime:
        return pendulum.now("UTC")

    def today_utc(self) -> date:
        return self.now_utc().date()


class FrozenClock:
    """Fixed instant for backtests, CI, and deterministic replays."""

    def __init__(self, at: pendulum.DateTime | str) -> None:
        if isinstance(at, str):
            p = pendulum.parse(at, tz="UTC")
        else:
            p = at.in_timezone("UTC")
        self._at = p

    def now_utc(self) -> pendulum.DateTime:
        return self._at

    def today_utc(self) -> date:
        return self._at.date()


def utc_today_offset(clock: Clock, days: int) -> date:
    d = clock.today_utc()
    nd = pendulum.date(d.year, d.month, d.day).add(days=days)
    return date(nd.year, nd.month, nd.day)
