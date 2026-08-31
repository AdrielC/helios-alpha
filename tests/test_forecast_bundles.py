from __future__ import annotations

import json

import pytest

from helios_alpha.forecasts import (
    ForecastBundleError,
    load_forecast_bundle,
    load_space_weather_bundle,
)


def test_space_weather_bundle_is_versioned_fingerprinted_and_complete() -> None:
    bundle = load_space_weather_bundle()

    assert bundle.schema_version == 1
    assert bundle.bundle_version == 1
    assert len(bundle.definition_sha256) == 64
    assert "donki-cme-analysis" in bundle.series_ids
    assert "l1-imf-bz-gsm" in bundle.series_ids
    assert "market-ohlc" in bundle.shared_series_ids
    assert all(item.max_age_seconds > 0 for item in bundle.input_contract)
    assert bundle.operator_contract()["definitionSha256"] == bundle.definition_sha256


def test_bundle_rejects_series_contract_drift(tmp_path) -> None:
    contract = load_space_weather_bundle().operator_contract()
    contract["seriesIds"][0], contract["seriesIds"][1] = (
        contract["seriesIds"][1],
        contract["seriesIds"][0],
    )
    path = tmp_path / "bad.json"
    path.write_text(json.dumps(contract), encoding="utf-8")

    with pytest.raises(ForecastBundleError, match="exactly match"):
        load_forecast_bundle(path)
