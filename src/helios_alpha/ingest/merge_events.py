from __future__ import annotations

from datetime import UTC, date, datetime, timedelta
from pathlib import Path

import polars as pl

from helios_alpha.config import load_settings
from helios_alpha.ingest import flares as flares_mod
from helios_alpha.ingest import geomagnetic as geo_mod
from helios_alpha.ingest import protons as protons_mod


def _first_cme_id(linked: str | None) -> str | None:
    if not linked:
        return None
    parts = [p.strip() for p in linked.split(",") if p.strip()]
    return parts[0] if parts else None


def _arrival_mid(
    t0: datetime | None, t1: datetime | None, fallback: date | None
) -> date | None:
    if t0 is not None and t1 is not None:
        if isinstance(t0, datetime) and isinstance(t1, datetime):
            mid = t0 + (t1 - t0) / 2
            return mid.astimezone(UTC).date()
    if t0 is not None and isinstance(t0, datetime):
        return t0.astimezone(UTC).date()
    if t1 is not None and isinstance(t1, datetime):
        return t1.astimezone(UTC).date()
    return fallback


def _speed_to_days(speed_kms: float | None) -> float | None:
    if speed_kms is None or speed_kms <= 0:
        return None
    au_km = 1.496e8
    days = au_km / speed_kms / 86400.0
    return float(days)


def build_event_table(
    flares: pl.DataFrame,
    cmes: pl.DataFrame,
    kp_daily: pl.DataFrame,
    protons: pl.DataFrame | None = None,
) -> pl.DataFrame:
    fl = flares_mod.flare_peak_trading_date(flares)
    fl = fl.with_columns(
        pl.col("linked_cme_ids")
        .map_elements(_first_cme_id, return_dtype=pl.Utf8)
        .alias("primary_cme_id")
    )
    cm = cmes.rename(
        {
            "earth_arrival_start_utc": "cme_earth_arrival_start_utc",
            "earth_arrival_end_utc": "cme_earth_arrival_end_utc",
        }
    )
    joined = fl.join(cm, left_on="primary_cme_id", right_on="cme_id", how="left")
    joined = joined.with_columns(
        pl.col("primary_cme_id").is_not_null().alias("cme_detected"),
        (
            pl.coalesce(pl.col("enlil_earth_gb"), pl.lit(False))
            | pl.coalesce(pl.col("earth_impact_listed"), pl.lit(False))
            | pl.coalesce(pl.col("earth_directed_heuristic"), pl.lit(False))
        ).alias("earth_directed"),
    )
    kp_prior = kp_daily.rename(
        {
            "date_utc": "kp_prior_date",
            "kp_estimated_max": "kp_estimated_max_prior_day",
            "kp_index_max": "kp_index_max_prior_day",
        }
    )
    joined = joined.with_columns(
        pl.col("event_date_utc").dt.offset_by("-1d").alias("kp_prior_date")
    ).join(kp_prior, on="kp_prior_date", how="left")

    def row_kp_dst_proxy(r: dict) -> dict:
        peak: date = r["event_date_utc"]
        t0 = r.get("cme_earth_arrival_start_utc")
        t1 = r.get("cme_earth_arrival_end_utc")
        speed = r.get("speed_kms")
        est_days = _speed_to_days(speed) if speed is not None else None
        fallback_arrival = None
        if est_days is not None:
            fallback_arrival = peak + timedelta(days=int(round(est_days)))
        arrival_mid = _arrival_mid(t0, t1, fallback_arrival)
        center = arrival_mid or peak
        stats = geo_mod.kp_stats_around_dates(kp_daily, [center], before_days=1, after_days=2)
        kp_est = stats["kp_estimated_max_window"][0]
        kp_ix = stats["kp_index_max_window"][0]
        pmax = None
        if protons is not None and not protons.is_empty():
            since = datetime.combine(peak, datetime.min.time()).replace(tzinfo=UTC)
            until = None
            if center:
                end_d = center + timedelta(days=2)
                until = datetime.combine(end_d, datetime.min.time()).replace(tzinfo=UTC)
            pmax = protons_mod.max_proton_since(protons, since, until)
        return {
            "arrival_window_center_utc": center,
            "kp_estimated_max_around_arrival": kp_est,
            "kp_index_max_around_arrival": kp_ix,
            "proton_flux_ge10_max_post_flare": pmax,
        }

    # Polars map_elements per row is slow but fine for MVP sizes
    py_rows = joined.to_dicts()
    enriched = [dict(**r, **row_kp_dst_proxy(r)) for r in py_rows]
    out = pl.DataFrame(enriched)
    cols_preferred = [
        "flare_id",
        "peak_time_utc",
        "event_date_utc",
        "class_type",
        "cme_detected",
        "primary_cme_id",
        "earth_directed",
        "speed_kms",
        "cme_earth_arrival_start_utc",
        "cme_earth_arrival_end_utc",
        "arrival_window_center_utc",
        "kp_estimated_max_around_arrival",
        "kp_index_max_around_arrival",
        "kp_estimated_max_prior_day",
        "kp_index_max_prior_day",
        "proton_flux_ge10_max_post_flare",
        "enlil_earth_gb",
        "earth_impact_listed",
        "earth_directed_heuristic",
    ]
    present = [c for c in cols_preferred if c in out.columns]
    return out.select(present + [c for c in out.columns if c not in present])


def save_event_table(df: pl.DataFrame, path: Path | None = None) -> Path:
    s = load_settings()
    path = path or (s.data_processed / "events" / "flare_cme_events.parquet")
    path.parent.mkdir(parents=True, exist_ok=True)
    df.write_parquet(path)
    return path
