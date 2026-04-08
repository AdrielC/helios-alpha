from __future__ import annotations

from datetime import date, timedelta

import polars as pl


def trading_window_dates(center: date, before: int, after: int) -> list[date]:
    return [center + timedelta(days=k) for k in range(-before, after + 1)]


def flag_overlap(event_dates: list[date], min_gap_days: int = 3) -> pl.DataFrame:
    """Mark events that fall within min_gap_days of another (for cluster-robust tests)."""
    d = sorted({d for d in event_dates if d is not None})
    rows = []
    for i, di in enumerate(d):
        near = any(abs((di - dj).days) <= min_gap_days for j, dj in enumerate(d) if j != i)
        rows.append({"event_date_utc": di, "clustered": near})
    return pl.DataFrame(rows)
