from __future__ import annotations

from datetime import date, timedelta
from pathlib import Path

import polars as pl

from helios_alpha.config import load_settings
from helios_alpha.timekeeping import Clock, SystemClock
from helios_alpha.utils.http import get_json
from helios_alpha.utils.time import parse_iso_z


def fetch_kp_1m_recent() -> pl.DataFrame:
    """Last ~7 days of 1-minute Kp estimates (NOAA SWPC)."""
    url = "https://services.swpc.noaa.gov/json/planetary_k_index_1m.json"
    rows = get_json(url)
    if not isinstance(rows, list):
        return pl.DataFrame(
            schema={
                "time_utc": pl.Datetime(time_zone="UTC"),
                "kp_index": pl.Int64,
                "estimated_kp": pl.Float64,
            }
        )
    out = []
    for r in rows:
        tt = r.get("time_tag")
        dt = parse_iso_z(tt + "Z") if tt and not str(tt).endswith("Z") else parse_iso_z(tt)
        if dt is None:
            continue
        out.append(
            {
                "time_utc": dt,
                "kp_index": int(r["kp_index"]) if r.get("kp_index") is not None else None,
                "estimated_kp": (
                    float(r["estimated_kp"]) if r.get("estimated_kp") is not None else None
                ),
            }
        )
    return pl.DataFrame(out).sort("time_utc")


def daily_kp_from_1m(df: pl.DataFrame) -> pl.DataFrame:
    """Aggregate to UTC calendar day: max estimated Kp and max discrete Kp index."""
    if df.is_empty():
        return pl.DataFrame(
            schema={
                "date_utc": pl.Date,
                "kp_estimated_max": pl.Float64,
                "kp_index_max": pl.Int64,
            }
        )
    return (
        df.with_columns(pl.col("time_utc").dt.convert_time_zone("UTC").dt.date().alias("date_utc"))
        .group_by("date_utc")
        .agg(
            pl.col("estimated_kp").max().alias("kp_estimated_max"),
            pl.col("kp_index").max().alias("kp_index_max"),
        )
        .sort("date_utc")
    )


def append_daily_kp_master(new_daily: pl.DataFrame, master_path: Path | None = None) -> Path:
    s = load_settings()
    path = master_path or (s.data_raw / "geomagnetic" / "kp_daily.parquet")
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.exists():
        old = pl.read_parquet(path)
        combined = pl.concat([old, new_daily], how="vertical_relaxed")
        combined = combined.group_by("date_utc", maintain_order=True).agg(
            pl.col("kp_estimated_max").max(),
            pl.col("kp_index_max").max(),
        )
    else:
        combined = new_daily
    combined.write_parquet(path)
    return path


def ingest_kp_daily_refresh(clock: Clock | None = None) -> pl.DataFrame:
    """
    Fetch recent 1m Kp (NOAA only retains ~7 days), aggregate to daily, merge master.

    ``clock`` controls reproducibility; defaults to system only when called from live ingest.
    """
    _ = clock or SystemClock()
    raw = fetch_kp_1m_recent()
    daily = daily_kp_from_1m(raw)
    append_daily_kp_master(daily)
    return daily


def load_kp_daily(path: Path | None = None) -> pl.DataFrame:
    s = load_settings()
    path = path or (s.data_raw / "geomagnetic" / "kp_daily.parquet")
    if not path.exists():
        return pl.DataFrame(
            schema={
                "date_utc": pl.Date,
                "kp_estimated_max": pl.Float64,
                "kp_index_max": pl.Int64,
            }
        )
    return pl.read_parquet(path)


def kp_stats_around_dates(
    kp_daily: pl.DataFrame,
    center_dates: list[date | None],
    before_days: int = 1,
    after_days: int = 2,
) -> pl.DataFrame:
    """For each center date, max Kp over [center-before, center+after]."""
    rows = []
    for d in center_dates:
        if d is None:
            rows.append(
                {
                    "window_center": None,
                    "kp_estimated_max_window": None,
                    "kp_index_max_window": None,
                }
            )
            continue
        start = d - timedelta(days=before_days)
        end = d + timedelta(days=after_days)
        sub = kp_daily.filter((pl.col("date_utc") >= start) & (pl.col("date_utc") <= end))
        if sub.is_empty():
            rows.append(
                {
                    "window_center": d,
                    "kp_estimated_max_window": None,
                    "kp_index_max_window": None,
                }
            )
        else:
            rows.append(
                {
                    "window_center": d,
                    "kp_estimated_max_window": float(sub["kp_estimated_max"].max()),
                    "kp_index_max_window": int(sub["kp_index_max"].max()),
                }
            )
    return pl.DataFrame(rows)


def placeholder_dst_note() -> str:
    return "See helios_alpha.ingest.dst_kyoto and DATA_SOURCES.md."
