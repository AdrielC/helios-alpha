"""
OMNI hourly Dst (and related indices) from NASA SPDF CDF files.

Base path (when reachable): https://spdf.gsfc.nasa.gov/pub/data/omni/omni_cdaweb/hourly/

Some networks block SPDF; in that case use `dst_kyoto` (ISWA mirror) instead.
Variable names differ by dataset; we try common Dst fields after opening CDF.
"""

from __future__ import annotations

import tempfile
from datetime import date, timedelta
from pathlib import Path

import polars as pl

from helios_alpha.utils.http import get_bytes
from helios_alpha.utils.time import from_unix_epoch_ms, from_unix_epoch_seconds

try:
    from cdflib import cdf as cdf_mod
except ImportError:
    cdf_mod = None  # type: ignore[misc, assignment]

OMNI_HOURLY_TEMPLATE = (
    "https://spdf.gsfc.nasa.gov/pub/data/omni/omni_cdaweb/hourly/{year}/"
    "omni2_h0_mrg1hr_{year}{month:02d}{day:02d}_v01.cdf"
)

_DST_CANDIDATES = ("DST", "Dst", "SYM_H", "SYM-H_INDEX", "SYM_H_INDEX")


def _cdf_path_for_day(year: int, month: int, day: int) -> str:
    return OMNI_HOURLY_TEMPLATE.format(year=year, month=month, day=day)


def _read_dst_from_cdf_bytes(data: bytes) -> pl.DataFrame:
    if cdf_mod is None:
        return pl.DataFrame(
            schema={"time_utc": pl.Datetime(time_zone="UTC"), "dst_nT": pl.Float64}
        )
    with tempfile.NamedTemporaryFile(suffix=".cdf", delete=True) as f:
        f.write(data)
        f.flush()
        c = cdf_mod.CDF(f.name)
        info = c.cdf_info()
        zvars = list(info.get("zVariables") or [])
        dst_var = next((v for v in _DST_CANDIDATES if v in zvars), None)
        epoch_var = "Epoch" if "Epoch" in zvars else None
        if not dst_var or not epoch_var:
            return pl.DataFrame(
                schema={"time_utc": pl.Datetime(time_zone="UTC"), "dst_nT": pl.Float64}
            )
        epochs = c.varget(epoch_var)
        dst = c.varget(dst_var)
        c.close()
    # Epoch is often ms since 1970 in OMNI CDFs
    rows = []
    if epochs is None or dst is None:
        return pl.DataFrame(
            schema={"time_utc": pl.Datetime(time_zone="UTC"), "dst_nT": pl.Float64}
        )
    flat_e = epochs.flatten() if hasattr(epochs, "flatten") else epochs
    flat_d = dst.flatten() if hasattr(dst, "flatten") else dst
    n = min(len(flat_e), len(flat_d))
    for i in range(n):
        e = float(flat_e[i])
        # Heuristic: if value looks like Unix ms
        if e > 1e12:
            ts = from_unix_epoch_ms(e)
        elif e > 1e9:
            ts = from_unix_epoch_seconds(e)
        else:
            # CDF epoch — skip row if we cannot convert reliably
            continue
        rows.append({"time_utc": ts, "dst_nT": float(flat_d[i])})
    return pl.DataFrame(rows).sort("time_utc")


def fetch_omni_dst_day(year: int, month: int, day: int) -> pl.DataFrame:
    url = _cdf_path_for_day(year, month, day)
    try:
        data = get_bytes(url, retries=1)
    except OSError:
        return pl.DataFrame(
            schema={"time_utc": pl.Datetime(time_zone="UTC"), "dst_nT": pl.Float64}
        )
    return _read_dst_from_cdf_bytes(data)


def fetch_omni_dst_range(start: date, end: date) -> pl.DataFrame:
    if start > end or cdf_mod is None:
        return pl.DataFrame(
            schema={"time_utc": pl.Datetime(time_zone="UTC"), "dst_nT": pl.Float64}
        )
    frames: list[pl.DataFrame] = []
    d = start
    while d <= end:
        df = fetch_omni_dst_day(d.year, d.month, d.day)
        if not df.is_empty():
            frames.append(df)
        d += timedelta(days=1)
    if not frames:
        return pl.DataFrame(
            schema={"time_utc": pl.Datetime(time_zone="UTC"), "dst_nT": pl.Float64}
        )
    return pl.concat(frames, how="vertical_relaxed").unique(subset=["time_utc"], keep="last")


def merge_omni_dst_daily_from_hourly(hourly: pl.DataFrame, master_path: Path | None = None) -> Path:
    from helios_alpha.ingest.dst_kyoto import daily_dst_min, merge_dst_daily_master

    daily = daily_dst_min(hourly)
    return merge_dst_daily_master(daily, master_path)
