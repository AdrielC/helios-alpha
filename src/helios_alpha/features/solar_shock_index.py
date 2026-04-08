from __future__ import annotations

import math
import re
from pathlib import Path
from typing import Any

import polars as pl
import yaml
from pydantic import BaseModel


class SSIWeights(BaseModel):
    flare: float = 0.3
    cme_speed: float = 0.15
    earth_directed: float = 0.1
    proton_flux: float = 0.1
    kp_forecast: float = 0.1
    dst_severity: float = 0.25


class SSIFloorsCaps(BaseModel):
    speed_floor_kms: float = 200.0
    speed_cap_kms: float = 2000.0
    proton_floor: float = 0.1
    proton_cap: float = 10000.0


def _flare_class_score(class_type: str | None) -> float:
    if not class_type:
        return 0.0
    m = re.match(r"^([ABCMX])(\d+\.?\d*)$", class_type.strip().upper())
    if not m:
        return 0.0
    letter = m.group(1)
    mult = float(m.group(2))
    base = {"A": 1e-8, "B": 1e-7, "C": 1e-6, "M": 1e-5, "X": 1e-4}[letter]
    x = base * mult
    ratio = math.log10(x + 1e-12) / math.log10(1e-3)
    return max(0.0, min(1.0, ratio))


def _norm_speed(v: float | None, floors: SSIFloorsCaps) -> float:
    if v is None:
        return 0.0
    x = max(floors.speed_floor_kms, min(floors.speed_cap_kms, float(v)))
    return (x - floors.speed_floor_kms) / (floors.speed_cap_kms - floors.speed_floor_kms)


def _norm_proton(v: float | None, floors: SSIFloorsCaps) -> float:
    if v is None:
        return 0.0
    x = max(floors.proton_floor, min(floors.proton_cap, float(v)))
    lo = math.log10(floors.proton_floor)
    hi = math.log10(floors.proton_cap)
    return (math.log10(x) - lo) / (hi - lo)


def _norm_kp_prior(v: float | None) -> float:
    if v is None:
        return 0.0
    return min(1.0, float(v) / 9.0)


def _norm_dst_min_window(dst_min_nT: float | None, cap: float = 150.0) -> float:
    """dst_min is most negative Dst in window; map stronger storms toward 1.0."""
    if dst_min_nT is None:
        return 0.0
    v = float(dst_min_nT)
    if v >= 0:
        return 0.0
    return min(1.0, (-v) / cap)


def load_ssi_config(path: Path | None = None) -> tuple[SSIWeights, SSIFloorsCaps]:
    from helios_alpha.config import load_settings

    path = path or (load_settings().repo_root / "config" / "thresholds.yaml")
    raw = yaml.safe_load(path.read_text(encoding="utf-8"))
    ssi = raw.get("solar_shock_index", {})
    w = SSIWeights(**ssi.get("weights", {}))
    fc = SSIFloorsCaps(**ssi.get("floors_caps", {}))
    return w, fc


class SSIBands(BaseModel):
    watch: float = 0.35
    warning: float = 0.55
    oh_no: float = 0.75


def load_thresholds(path: Path | None = None) -> SSIBands:
    from helios_alpha.config import load_settings

    path = path or (load_settings().repo_root / "config" / "thresholds.yaml")
    raw = yaml.safe_load(path.read_text(encoding="utf-8"))
    ssi = raw.get("solar_shock_index", {})
    return SSIBands(**ssi.get("bands", {}))


def compute_ssi(df: pl.DataFrame, config_path: Path | None = None) -> pl.DataFrame:
    w, fc = load_ssi_config(config_path)
    thr = load_thresholds(config_path)

    def score_row(r: dict[str, Any]) -> dict[str, float | str]:
        flare_s = _flare_class_score(r.get("class_type"))
        speed_s = _norm_speed(r.get("speed_kms"), fc)
        strict = r.get("earth_directed_strict")
        earth = bool(strict if strict is not None else r.get("earth_directed"))
        earth_f = 1.0 if earth else 0.0
        prot = _norm_proton(r.get("proton_flux_ge10_max_post_flare"), fc)
        kp = _norm_kp_prior(r.get("kp_estimated_max_prior_day"))
        dst_s = _norm_dst_min_window(r.get("dst_min_nT_around_arrival"))
        ssi = (
            w.flare * flare_s
            + w.cme_speed * speed_s
            + w.earth_directed * earth_f
            + w.proton_flux * prot
            + w.kp_forecast * kp
            + w.dst_severity * dst_s
        )
        band = "calm"
        if ssi >= thr.oh_no:
            band = "oh_no"
        elif ssi >= thr.warning:
            band = "warning"
        elif ssi >= thr.watch:
            band = "watch"
        return {"ssi": float(ssi), "ssi_band": band}

    rows = df.to_dicts()
    scored = [dict(**r, **score_row(r)) for r in rows]
    return pl.DataFrame(scored)
