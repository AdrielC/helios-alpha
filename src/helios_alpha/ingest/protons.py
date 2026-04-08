from __future__ import annotations

from pathlib import Path

import polars as pl

from helios_alpha.config import load_settings
from helios_alpha.timekeeping import Clock, SystemClock
from helios_alpha.utils.http import get_json
from helios_alpha.utils.time import parse_iso_z


def fetch_goes_integral_protons_7d() -> pl.DataFrame:
    url = "https://services.swpc.noaa.gov/json/goes/primary/integral-protons-7-day.json"
    rows = get_json(url)
    if not isinstance(rows, list):
        return pl.DataFrame(
            schema={
                "time_utc": pl.Datetime(time_zone="UTC"),
                "energy": pl.Utf8,
                "flux": pl.Float64,
                "satellite": pl.Int64,
            }
        )
    out = []
    for r in rows:
        tt = r.get("time_tag")
        dt = parse_iso_z(tt) if tt else None
        if dt is None:
            continue
        out.append(
            {
                "time_utc": dt,
                "energy": str(r.get("energy") or ""),
                "flux": float(r["flux"]) if r.get("flux") is not None else None,
                "satellite": int(r["satellite"]) if r.get("satellite") is not None else None,
            }
        )
    return pl.DataFrame(out).sort(["time_utc", "energy"])


def pivot_ge10(df: pl.DataFrame) -> pl.DataFrame:
    ge10 = df.filter(pl.col("energy").str.contains("10 MeV"))
    return ge10.rename({"flux": "proton_flux_ge10"}).select(["time_utc", "proton_flux_ge10"])


def append_proton_snapshots(new: pl.DataFrame, master_path: Path | None = None) -> Path:
    s = load_settings()
    path = master_path or (s.data_raw / "solar" / "protons_ge10.parquet")
    path.parent.mkdir(parents=True, exist_ok=True)
    new = pivot_ge10(new)
    if path.exists():
        old = pl.read_parquet(path)
        combined = pl.concat([old, new], how="vertical_relaxed").unique(
            subset=["time_utc"], keep="last"
        )
    else:
        combined = new.unique(subset=["time_utc"], keep="last")
    combined = combined.sort("time_utc")
    combined.write_parquet(path)
    return path


def ingest_protons_refresh(clock: Clock | None = None) -> pl.DataFrame:
    _ = clock or SystemClock()
    raw = fetch_goes_integral_protons_7d()
    append_proton_snapshots(raw)
    return pivot_ge10(raw)


def max_proton_since(
    protons: pl.DataFrame, since_utc: object, until_utc: object | None = None
) -> float | None:
    if protons.is_empty():
        return None
    q = protons.filter(pl.col("time_utc") >= since_utc)
    if until_utc is not None:
        q = q.filter(pl.col("time_utc") <= until_utc)
    if q.is_empty():
        return None
    return float(q["proton_flux_ge10"].max())
