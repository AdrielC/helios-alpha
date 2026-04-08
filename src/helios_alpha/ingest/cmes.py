from __future__ import annotations

import json
from datetime import date
from pathlib import Path

import polars as pl

from helios_alpha.config import load_settings
from helios_alpha.utils.http import get_json
from helios_alpha.utils.time import daterange_chunks, parse_iso_z


def _pick_analysis(cme: dict) -> dict | None:
    analyses = cme.get("cmeAnalyses") or []
    if not analyses:
        return None
    accurate = [a for a in analyses if a.get("isMostAccurate")]
    return accurate[0] if accurate else analyses[0]


def _earth_arrival_window(
    enlil: dict | None,
) -> tuple[object, object, bool, bool, bool, bool]:
    """Return (t_start, t_end, earth_in_impact_list, glancing, enlil_gb, enlil_minor)."""
    if not enlil:
        return None, None, False, False, False, False
    shock = enlil.get("estimatedShockArrivalTime")
    impacts = enlil.get("impactList") or []
    earth_times: list[object] = []
    earth_listed = False
    is_glancing = False
    for imp in impacts:
        loc = str(imp.get("location") or "").lower()
        if loc == "earth":
            earth_listed = True
            at = imp.get("arrivalTime")
            if at:
                earth_times.append(parse_iso_z(at) or at)
            if imp.get("isGlancingBlow"):
                is_glancing = True
    t_min = min(earth_times) if earth_times else None
    t_max = max(earth_times) if earth_times else None
    egb = bool(enlil.get("isEarthGB"))
    emi = bool(enlil.get("isEarthMinorImpact"))
    if shock and t_min is None:
        st = parse_iso_z(shock)
        return st, st, earth_listed, is_glancing, egb, emi
    return t_min, t_max, earth_listed, is_glancing, egb, emi


def _earth_directed_heuristic(
    half_angle: float | None, longitude: float | None, cme_type: str | None
) -> bool:
    """Disk-proxy for 'Earth-directed' when ENLIL Earth impacts are missing."""
    ct = (cme_type or "").upper()
    wide = half_angle is not None and half_angle >= 35
    earth_limb = longitude is not None and abs(longitude) <= 45
    haloish = ct in {"O", "C", "R"} and wide
    return bool(haloish or (wide and earth_limb))


def _donki_cme(start: date, end: date, api_key: str) -> list[dict]:
    url = "https://api.nasa.gov/DONKI/CME"
    params = {
        "startDate": start.isoformat(),
        "endDate": end.isoformat(),
        "api_key": api_key,
    }
    data = get_json(url, params=params)
    return data if isinstance(data, list) else []


def fetch_cmes_range(start: date, end: date, api_key: str | None = None) -> pl.DataFrame:
    key = api_key or load_settings().nasa_api_key
    rows: list[dict] = []
    for a, b in daterange_chunks(start, end, max_days=30):
        rows.extend(_donki_cme(a, b, key))
    if not rows:
        return pl.DataFrame(
            schema={
                "cme_id": pl.Utf8,
                "start_time_utc": pl.Datetime(time_zone="UTC"),
                "speed_kms": pl.Float64,
                "half_angle_deg": pl.Float64,
                "longitude_deg": pl.Float64,
                "latitude_deg": pl.Float64,
                "cme_type": pl.Utf8,
                "earth_arrival_start_utc": pl.Datetime(time_zone="UTC"),
                "earth_arrival_end_utc": pl.Datetime(time_zone="UTC"),
                "enlil_earth_gb": pl.Boolean,
                "enlil_earth_minor_impact": pl.Boolean,
                "earth_impact_listed": pl.Boolean,
                "earth_impact_glancing": pl.Boolean,
                "earth_directed_heuristic": pl.Boolean,
                "linked_flare_ids": pl.Utf8,
            }
        )
    out = _cme_records_from_payload(rows)
    df = pl.DataFrame(out)
    df = df.unique(subset=["cme_id"], keep="last").sort("start_time_utc")
    return df


def _cme_records_from_payload(rows: list[dict]) -> list[dict]:
    out: list[dict] = []
    for cme in rows:
        aid = cme.get("activityID")
        analysis = _pick_analysis(cme)
        speed = analysis.get("speed") if analysis else None
        half = analysis.get("halfAngle") if analysis else None
        lon = analysis.get("longitude") if analysis else None
        lat = analysis.get("latitude") if analysis else None
        ctype = analysis.get("type") if analysis else None
        enlil = None
        if analysis:
            enlils = analysis.get("enlilList") or []
            enlil = enlils[0] if enlils else None
        t0, t1, earth_listed, glancing, egb, emi = _earth_arrival_window(enlil)
        half_f = float(half) if half is not None else None
        lon_f = float(lon) if lon is not None else None
        directed = _earth_directed_heuristic(half_f, lon_f, ctype)
        linked = cme.get("linkedEvents") or []
        flares = [x.get("activityID") for x in linked if "-FLR-" in str(x.get("activityID", ""))]
        out.append(
            {
                "cme_id": aid,
                "start_time_utc": parse_iso_z(cme.get("startTime")),
                "speed_kms": float(speed) if speed is not None else None,
                "half_angle_deg": half_f,
                "longitude_deg": lon_f,
                "latitude_deg": float(lat) if lat is not None else None,
                "cme_type": ctype,
                "earth_arrival_start_utc": t0,
                "earth_arrival_end_utc": t1,
                "enlil_earth_gb": egb,
                "enlil_earth_minor_impact": emi,
                "earth_impact_listed": earth_listed,
                "earth_impact_glancing": glancing,
                "earth_directed_heuristic": directed,
                "linked_flare_ids": ",".join(flares) if flares else None,
            }
        )
    return out


def save_cmes_parquet(df: pl.DataFrame, path: Path | None = None) -> Path:
    s = load_settings()
    path = path or (s.data_raw / "solar" / "cmes.parquet")
    path.parent.mkdir(parents=True, exist_ok=True)
    df.write_parquet(path)
    return path


def ingest_cmes_json(path: Path) -> pl.DataFrame:
    raw = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(raw, list):
        msg = "Expected JSON array from DONKI CME"
        raise ValueError(msg)
    out = _cme_records_from_payload(raw)
    df = pl.DataFrame(out)
    return df.unique(subset=["cme_id"], keep="last").sort("start_time_utc")