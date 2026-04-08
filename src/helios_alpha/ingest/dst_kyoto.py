"""
Kyoto Dst (hourly) via NASA ISWA mirror of WDC-Kyoto-style monthly files.

Format: one line per UTC day, 24 hourly Dst nT values (fixed-width columns).
Primary: https://iswa.gsfc.nasa.gov/iswa_data_tree/index/geomagnetic/Dst-realtime/WDC-Kyoto/dstYYMM.txt

This is suitable for historical backfill; for operational "final" Dst prefer
direct Kyoto WDC exports when you need authoritative provenance.
"""

from __future__ import annotations

import re
from calendar import monthrange
from datetime import UTC, date, datetime, timedelta
from pathlib import Path

import polars as pl

from helios_alpha.config import load_settings
from helios_alpha.utils.http import get_text

_ISWA_BASE = (
    "https://iswa.gsfc.nasa.gov/iswa_data_tree/index/geomagnetic/Dst-realtime/WDC-Kyoto"
)


def _month_file_url(year: int, month: int) -> str:
    yy = year % 100
    return f"{_ISWA_BASE}/dst{yy:02d}{month:02d}.txt"


def _parse_dst_month_lines(text: str, year: int, month: int) -> list[dict]:
    """
    Parse WDC-style Dst month file: lines like
    DST2401*01RRX020   0   4   5 ...
    """
    rows: list[dict] = []
    day_re = re.compile(r"^DST\d{4}\*(\d{2})")
    for line in text.splitlines():
        line = line.rstrip()
        if not line.strip():
            continue
        m = day_re.match(line)
        if not m:
            continue
        day = int(m.group(1))
        if day < 1 or day > monthrange(year, month)[1]:
            continue
        rest = line[m.end() :].strip()
        parts = rest.split()
        if not parts:
            continue
        # First token may be metadata (RRX020); skip non-numeric leading tokens
        nums: list[int] = []
        for p in parts:
            try:
                nums.append(int(p))
            except ValueError:
                continue
            if len(nums) >= 24:
                break
        if len(nums) < 24:
            continue
        for hour, dst in enumerate(nums[:24]):
            ts = datetime(year, month, day, hour, 0, 0, tzinfo=UTC)
            rows.append({"time_utc": ts, "dst_nT": float(dst)})
    return rows


def fetch_kyoto_dst_month(year: int, month: int) -> pl.DataFrame:
    url = _month_file_url(year, month)
    text = get_text(url)
    rows = _parse_dst_month_lines(text, year, month)
    if not rows:
        return pl.DataFrame(
            schema={"time_utc": pl.Datetime(time_zone="UTC"), "dst_nT": pl.Float64}
        )
    return pl.DataFrame(rows).sort("time_utc")


def fetch_kyoto_dst_range(start: date, end: date) -> pl.DataFrame:
    """Inclusive date range on UTC calendar."""
    if start > end:
        return pl.DataFrame(
            schema={"time_utc": pl.Datetime(time_zone="UTC"), "dst_nT": pl.Float64}
        )
    frames: list[pl.DataFrame] = []
    y, m = start.year, start.month
    while (y, m) <= (end.year, end.month):
        frames.append(fetch_kyoto_dst_month(y, m))
        if m == 12:
            y += 1
            m = 1
        else:
            m += 1
    if not frames:
        return pl.DataFrame(
            schema={"time_utc": pl.Datetime(time_zone="UTC"), "dst_nT": pl.Float64}
        )
    out = pl.concat(frames, how="vertical_relaxed")
    start_dt = datetime.combine(start, datetime.min.time()).replace(tzinfo=UTC)
    end_dt = datetime.combine(end + timedelta(days=1), datetime.min.time()).replace(tzinfo=UTC)
    return out.filter((pl.col("time_utc") >= start_dt) & (pl.col("time_utc") < end_dt))


def daily_dst_min(df_hourly: pl.DataFrame) -> pl.DataFrame:
    """Most negative Dst per UTC day (storm severity)."""
    if df_hourly.is_empty():
        return pl.DataFrame(schema={"date_utc": pl.Date, "dst_min_nT": pl.Float64})
    return (
        df_hourly.with_columns(pl.col("time_utc").dt.date().alias("date_utc"))
        .group_by("date_utc")
        .agg(pl.col("dst_nT").min().alias("dst_min_nT"))
        .sort("date_utc")
    )


def merge_dst_daily_master(new_daily: pl.DataFrame, master_path: Path | None = None) -> Path:
    s = load_settings()
    path = master_path or (s.data_raw / "geomagnetic" / "dst_daily.parquet")
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.exists():
        old = pl.read_parquet(path)
        combined = pl.concat([old, new_daily], how="vertical_relaxed")
        combined = combined.group_by("date_utc", maintain_order=True).agg(
            pl.col("dst_min_nT").min()
        )
    else:
        combined = new_daily
    combined.write_parquet(path)
    return path


def ingest_kyoto_dst_range(start: date, end: date) -> pl.DataFrame:
    hourly = fetch_kyoto_dst_range(start, end)
    daily = daily_dst_min(hourly)
    merge_dst_daily_master(daily)
    return daily


def load_dst_daily(path: Path | None = None) -> pl.DataFrame:
    s = load_settings()
    path = path or (s.data_raw / "geomagnetic" / "dst_daily.parquet")
    if not path.exists():
        return pl.DataFrame(schema={"date_utc": pl.Date, "dst_min_nT": pl.Float64})
    return pl.read_parquet(path)


def dst_stats_around_dates(
    dst_daily: pl.DataFrame,
    center_dates: list[date | None],
    before_days: int = 1,
    after_days: int = 2,
) -> pl.DataFrame:
    """Minimum Dst (most negative) in [center - before, center + after]."""
    rows = []
    for d in center_dates:
        if d is None:
            rows.append({"window_center": None, "dst_min_window_nT": None})
            continue
        start = d - timedelta(days=before_days)
        end = d + timedelta(days=after_days)
        sub = dst_daily.filter((pl.col("date_utc") >= start) & (pl.col("date_utc") <= end))
        if sub.is_empty():
            rows.append({"window_center": d, "dst_min_window_nT": None})
        else:
            rows.append(
                {
                    "window_center": d,
                    "dst_min_window_nT": float(sub["dst_min_nT"].min()),
                }
            )
    return pl.DataFrame(rows)
