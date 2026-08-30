from __future__ import annotations

import math

import polars as pl
import pytest

from helios_alpha.features.solar_shock_index import _flare_class_score, compute_ssi


def row(**overrides: object) -> dict[str, object]:
    base: dict[str, object] = {
        "class_type": "X1.0",
        "speed_kms": 1200.0,
        "earth_directed_strict": True,
        "earth_directed": True,
        "proton_flux_ge10_max_post_flare": 10.0,
        "kp_estimated_max_prior_day": 4.0,
        "dst_min_nT_around_arrival": -25.0,
    }
    base.update(overrides)
    return base


def test_flare_score_uses_a_bounded_log_flux_scale() -> None:
    assert _flare_class_score("A1") == pytest.approx(0.0)
    assert _flare_class_score("M1") == pytest.approx(0.6)
    assert _flare_class_score("X1") == pytest.approx(0.8)
    assert _flare_class_score("X10") == pytest.approx(1.0)


def test_candidate_ssi_does_not_use_future_arrival_dst() -> None:
    scored = compute_ssi(
        pl.DataFrame(
            [
                row(dst_min_nT_around_arrival=-10.0),
                row(dst_min_nT_around_arrival=-500.0),
            ]
        )
    )
    assert scored["ssi"][0] == scored["ssi"][1]
    assert scored["ssi_complete"].to_list() == [True, True]


def test_missing_inputs_are_visible_in_output() -> None:
    scored = compute_ssi(pl.DataFrame([row(speed_kms=None)]))
    assert scored["ssi_complete"][0] is False
    assert "speed_kms" in scored["ssi_missing_inputs"][0]


def test_non_finite_feature_fails_closed() -> None:
    with pytest.raises(ValueError, match="CME speed must be finite"):
        compute_ssi(pl.DataFrame([row(speed_kms=math.inf)]))
