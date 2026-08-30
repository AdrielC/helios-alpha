from __future__ import annotations

import math
import re
from pathlib import Path
from typing import Any

import polars as pl
import yaml
from pydantic import BaseModel


class SSIWeights(BaseModel):
    flare: float = 0.4
    cme_speed: float = 0.2
    earth_directed: float = 0.15
    proton_flux: float = 0.15
    kp_prior: float = 0.1


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
    if not math.isfinite(mult) or mult <= 0.0:
        msg = f"invalid flare multiplier: {mult}"
        raise ValueError(msg)
    base = {"A": 1e-8, "B": 1e-7, "C": 1e-6, "M": 1e-5, "X": 1e-4}[letter]
    x = base * mult
    # A1 maps to 0, X1 to 0.8, and X10 to 1 on a log-flux scale.
    ratio = (math.log10(x) - math.log10(1e-8)) / (
        math.log10(1e-3) - math.log10(1e-8)
    )
    return max(0.0, min(1.0, ratio))


def _finite(value: float, label: str) -> float:
    parsed = float(value)
    if not math.isfinite(parsed):
        msg = f"{label} must be finite"
        raise ValueError(msg)
    return parsed


def _norm_speed(v: float | None, floors: SSIFloorsCaps) -> float:
    if v is None:
        return 0.0
    x = max(floors.speed_floor_kms, min(floors.speed_cap_kms, _finite(v, "CME speed")))
    return (x - floors.speed_floor_kms) / (floors.speed_cap_kms - floors.speed_floor_kms)


def _norm_proton(v: float | None, floors: SSIFloorsCaps) -> float:
    if v is None:
        return 0.0
    x = max(floors.proton_floor, min(floors.proton_cap, _finite(v, "proton flux")))
    lo = math.log10(floors.proton_floor)
    hi = math.log10(floors.proton_cap)
    return (math.log10(x) - lo) / (hi - lo)


def _norm_kp_prior(v: float | None) -> float:
    if v is None:
        return 0.0
    return min(1.0, max(0.0, _finite(v, "prior Kp") / 9.0))


def _validate_config(weights: SSIWeights, floors: SSIFloorsCaps, bands: SSIBands) -> None:
    weight_values = list(weights.model_dump().values())
    if any(not math.isfinite(value) or value < 0.0 for value in weight_values):
        msg = "SSI weights must be finite and non-negative"
        raise ValueError(msg)
    if not math.isclose(math.fsum(weight_values), 1.0, rel_tol=0.0, abs_tol=1e-12):
        msg = "SSI weights must sum to one"
        raise ValueError(msg)
    floor_values = list(floors.model_dump().values())
    if any(not math.isfinite(value) or value <= 0.0 for value in floor_values):
        msg = "SSI floors and caps must be finite and positive"
        raise ValueError(msg)
    if not (
        floors.speed_floor_kms < floors.speed_cap_kms
        and floors.proton_floor < floors.proton_cap
    ):
        msg = "SSI caps must exceed their floors"
        raise ValueError(msg)
    if not (
        0.0 <= bands.watch < bands.warning < bands.oh_no <= 1.0
        and all(math.isfinite(value) for value in bands.model_dump().values())
    ):
        msg = "SSI bands must be finite, ordered, and inside [0, 1]"
        raise ValueError(msg)


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
    _validate_config(w, fc, thr)

    def score_row(r: dict[str, Any]) -> dict[str, float | str]:
        flare_s = _flare_class_score(r.get("class_type"))
        speed_s = _norm_speed(r.get("speed_kms"), fc)
        strict = r.get("earth_directed_strict")
        earth = bool(strict if strict is not None else r.get("earth_directed"))
        earth_f = 1.0 if earth else 0.0
        prot = _norm_proton(r.get("proton_flux_ge10_prior_24h"), fc)
        kp = _norm_kp_prior(r.get("kp_estimated_max_prior_day"))
        components = [
            w.flare * flare_s,
            w.cme_speed * speed_s,
            w.earth_directed * earth_f,
            w.proton_flux * prot,
            w.kp_prior * kp,
        ]
        ssi = math.fsum(components)
        if not math.isfinite(ssi) or not 0.0 <= ssi <= 1.0 + 1e-12:
            msg = f"SSI arithmetic escaped [0, 1]: {ssi}"
            raise ValueError(msg)
        ssi = min(1.0, max(0.0, ssi))
        missing = [
            name
            for name, value in [
                ("class_type", r.get("class_type")),
                ("speed_kms", r.get("speed_kms")),
                ("earth_directed", strict if strict is not None else r.get("earth_directed")),
                ("proton_flux_prior_24h", r.get("proton_flux_ge10_prior_24h")),
                ("kp_prior", r.get("kp_estimated_max_prior_day")),
            ]
            if value is None
        ]
        band = "calm"
        if ssi >= thr.oh_no:
            band = "oh_no"
        elif ssi >= thr.warning:
            band = "warning"
        elif ssi >= thr.watch:
            band = "watch"
        return {
            "ssi": float(ssi),
            "ssi_band": band,
            "ssi_complete": not missing,
            "ssi_missing_inputs": ",".join(missing),
        }

    rows = df.to_dicts()
    scored = [dict(**r, **score_row(r)) for r in rows]
    return pl.DataFrame(scored)
