"""
Trading sessions and *custom business time* for US equities.

**Source of truth**: `exchange_calendars` (e.g. XNYS) — holidays and DST at open/close.
Pandas ``CustomBusinessDay`` / ``CustomBusinessHour`` are derived from that calendar.

XNYS regular hours in `exchange_calendars` use a single continuous RTH block (no lunch in
schedule). If a venue exposes ``break_start`` / ``break_end``, use
``session_break_start`` / ``session_break_end`` on the calendar for that day.
"""

from __future__ import annotations

from dataclasses import dataclass
from datetime import date
from pathlib import Path
from typing import Any

import exchange_calendars as xc
import pandas as pd
import yaml


@dataclass(frozen=True)
class TradingCalendar:
    """Thin wrapper around ``exchange_calendars`` with pandas offset helpers."""

    exchange: str = "XNYS"

    def __post_init__(self) -> None:
        object.__setattr__(self, "_cal", xc.get_calendar(self.exchange))

    @property
    def cal(self) -> xc.ExchangeCalendar:
        return self._cal  # type: ignore[no-any-return]

    def is_session(self, day: pd.Timestamp | str | date) -> bool:
        ts = pd.Timestamp(day).normalize()
        return bool(self.cal.is_session(ts))

    def session_open_utc(self, day: pd.Timestamp | str | date) -> pd.Timestamp:
        ts = pd.Timestamp(day).normalize()
        return self.cal.session_open(ts)

    def session_close_utc(self, day: pd.Timestamp | str | date) -> pd.Timestamp:
        ts = pd.Timestamp(day).normalize()
        return self.cal.session_close(ts)

    def session_break_start_utc(self, day: pd.Timestamp | str | date) -> pd.Timestamp | None:
        ts = pd.Timestamp(day).normalize()
        if not self.cal.is_session(ts):
            return None
        bs = self.cal.session_break_start(ts)
        return None if pd.isna(bs) else bs

    def session_break_end_utc(self, day: pd.Timestamp | str | date) -> pd.Timestamp | None:
        ts = pd.Timestamp(day).normalize()
        if not self.cal.is_session(ts):
            return None
        be = self.cal.session_break_end(ts)
        return None if pd.isna(be) else be

    def next_session(self, day: pd.Timestamp | str | date) -> pd.Timestamp:
        ts = pd.Timestamp(day).normalize()
        return self.cal.next_session(ts)

    def previous_session(self, day: pd.Timestamp | str | date) -> pd.Timestamp:
        ts = pd.Timestamp(day).normalize()
        return self.cal.previous_session(ts)

    def sessions_in_range(
        self, start: pd.Timestamp | str | date, end: pd.Timestamp | str | date
    ) -> pd.DatetimeIndex:
        a = pd.Timestamp(start).normalize()
        b = pd.Timestamp(end).normalize()
        return self.cal.sessions_in_range(a, b)

    def custom_business_day(self) -> pd.offsets.CustomBusinessDay:
        return pd.offsets.CustomBusinessDay(calendar=self.cal)

    def custom_business_hour_regular(self) -> pd.offsets.CustomBusinessHour:
        """
        Single continuous RTH block in **exchange local** time (America/New_York for XNYS).

        Does not split around lunch unless you model breaks separately (see
        ``session_break_*_utc``).
        """
        if self.exchange != "XNYS":
            msg = f"Hour window defaults are for XNYS, not {self.exchange}"
            raise ValueError(msg)
        return pd.offsets.CustomBusinessHour(
            start="09:30",
            end="16:00",
            weekmask="Mon Tue Wed Thu Fri",
            calendar=self.cal,
        )

    def shift_sessions(self, day: pd.Timestamp | str | date, periods: int) -> pd.Timestamp:
        """``periods`` > 0: forward N **sessions** from session label ``day``."""
        off = periods * self.custom_business_day()
        return pd.Timestamp(day).normalize() + off

    def align_to_trading_day(self, ts: pd.Timestamp | date) -> pd.Timestamp:
        """Session **date** label for the session containing instant ``ts`` (UTC-aware)."""
        t = pd.Timestamp(ts)
        if t.tzinfo is None:
            t = t.tz_localize("UTC")
        else:
            t = t.tz_convert("UTC")
        return self.cal.minute_to_session(t)

    def session_offset_date(self, session_label: date | pd.Timestamp, periods: int) -> date:
        """Return calendar date of the session ``periods`` steps from ``session_label``."""
        end_ts = self.shift_sessions(session_label, periods)
        return end_ts.date()


def load_trading_calendar_config(path: Path | None = None) -> TradingCalendar:
    from helios_alpha.config import load_settings

    s = load_settings()
    path = path or (s.repo_root / "config" / "markets" / "xnys.yaml")
    raw: dict[str, Any] = yaml.safe_load(path.read_text(encoding="utf-8"))
    ex = str(raw.get("exchange", "XNYS"))
    return TradingCalendar(exchange=ex)
