from __future__ import annotations

import json
from datetime import date
from pathlib import Path

import polars as pl

from helios_alpha.config import load_settings
from helios_alpha.utils.http import get_json
from helios_alpha.utils.time import daterange_chunks, parse_iso_z


def _donki_flr(start: date, end: date, api_key: str) -> list[dict]:
    url = "https://api.nasa.gov/DONKI/FLR"
    params = {
        "startDate": start.isoformat(),
        "endDate": end.isoformat(),
        "api_key": api_key,
    }
    data = get_json(url, params=params)
    return data if isinstance(data, list) else []


def fetch_flares_range(start: date, end: date, api_key: str | None = None) -> pl.DataFrame:
    key = api_key or load_settings().nasa_api_key
    rows: list[dict] = []
    for a, b in daterange_chunks(start, end, max_days=30):
        rows.extend(_donki_flr(a, b, key))
    if not rows:
        return pl.DataFrame(
            schema={
                "flare_id": pl.Utf8,
                "peak_time_utc": pl.Datetime(time_zone="UTC"),
                "begin_time_utc": pl.Datetime(time_zone="UTC"),
                "end_time_utc": pl.Datetime(time_zone="UTC"),
                "class_type": pl.Utf8,
                "active_region_num": pl.Int64,
                "linked_cme_ids": pl.Utf8,
            }
        )
    out: list[dict] = []
    for r in rows:
        linked = r.get("linkedEvents") or []
        cme_ids = [x.get("activityID") for x in linked if "-CME-" in str(x.get("activityID", ""))]
        ar = r.get("activeRegionNum")
        out.append(
            {
                "flare_id": r.get("flrID"),
                "peak_time_utc": parse_iso_z(r.get("peakTime")),
                "begin_time_utc": parse_iso_z(r.get("beginTime")),
                "end_time_utc": parse_iso_z(r.get("endTime")),
                "class_type": r.get("classType"),
                "active_region_num": int(ar) if ar is not None else None,
                "linked_cme_ids": ",".join(cme_ids) if cme_ids else None,
            }
        )
    df = pl.DataFrame(out)
    df = df.unique(subset=["flare_id"], keep="last").sort("peak_time_utc")
    return df


def save_flares_parquet(df: pl.DataFrame, path: Path | None = None) -> Path:
    s = load_settings()
    path = path or (s.data_raw / "solar" / "flares.parquet")
    path.parent.mkdir(parents=True, exist_ok=True)
    df.write_parquet(path)
    return path


def ingest_flares_json(path: Path) -> pl.DataFrame:
    raw = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(raw, list):
        msg = "Expected JSON array from DONKI FLR"
        raise ValueError(msg)
    rows = []
    for r in raw:
        linked = r.get("linkedEvents") or []
        cme_ids = [x.get("activityID") for x in linked if "-CME-" in str(x.get("activityID", ""))]
        ar = r.get("activeRegionNum")
        rows.append(
            {
                "flare_id": r.get("flrID"),
                "peak_time_utc": parse_iso_z(r.get("peakTime")),
                "begin_time_utc": parse_iso_z(r.get("beginTime")),
                "end_time_utc": parse_iso_z(r.get("endTime")),
                "class_type": r.get("classType"),
                "active_region_num": int(ar) if ar is not None else None,
                "linked_cme_ids": ",".join(cme_ids) if cme_ids else None,
            }
        )
    df = pl.DataFrame(rows)
    return df.unique(subset=["flare_id"], keep="last").sort("peak_time_utc")


def flare_peak_trading_date(df: pl.DataFrame) -> pl.DataFrame:
    """NYSE calendar day for event anchor (UTC date is fine for first pass)."""
    return df.with_columns(
        pl.col("peak_time_utc").dt.replace_time_zone(None).dt.date().alias("event_date_utc")
    )
