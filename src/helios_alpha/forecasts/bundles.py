from __future__ import annotations

import hashlib
import json
from dataclasses import dataclass
from pathlib import Path
from typing import Any


class ForecastBundleError(ValueError):
    """A forecast bundle does not satisfy its versioned observation contract."""


@dataclass(frozen=True)
class ForecastInput:
    series_id: str
    role: str
    required: bool
    max_age_seconds: int
    source_ids: tuple[str, ...]


@dataclass(frozen=True)
class ForecastBundle:
    schema_version: int
    bundle_version: int
    definition_sha256: str
    id: str
    label: str
    thesis: str
    horizon: str
    state: str
    strategy_ids: tuple[str, ...]
    series_ids: tuple[str, ...]
    shared_series_ids: tuple[str, ...]
    input_contract: tuple[ForecastInput, ...]

    def operator_contract(self) -> dict[str, Any]:
        return {
            "schemaVersion": self.schema_version,
            "bundleVersion": self.bundle_version,
            "definitionSha256": self.definition_sha256,
            "id": self.id,
            "label": self.label,
            "thesis": self.thesis,
            "horizon": self.horizon,
            "state": self.state,
            "strategyIds": list(self.strategy_ids),
            "seriesIds": list(self.series_ids),
            "sharedSeriesIds": list(self.shared_series_ids),
            "inputContract": [
                {
                    "seriesId": item.series_id,
                    "role": item.role,
                    "required": item.required,
                    "maxAgeSeconds": item.max_age_seconds,
                    "sourceIds": list(item.source_ids),
                }
                for item in self.input_contract
            ],
        }


def _text(value: object, field: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise ForecastBundleError(f"{field} must not be empty")
    return value


def _text_list(value: object, field: str) -> tuple[str, ...]:
    if not isinstance(value, list):
        raise ForecastBundleError(f"{field} must be an array")
    result = tuple(_text(item, field) for item in value)
    if len(set(result)) != len(result):
        raise ForecastBundleError(f"{field} contains duplicates")
    return result


def load_forecast_bundle(path: Path) -> ForecastBundle:
    raw = path.read_bytes()
    try:
        payload = json.loads(raw)
    except json.JSONDecodeError as error:
        raise ForecastBundleError("forecast bundle is not valid JSON") from error
    if not isinstance(payload, dict):
        raise ForecastBundleError("forecast bundle root must be an object")
    schema_version = payload.get("schemaVersion")
    bundle_version = payload.get("bundleVersion")
    if not isinstance(schema_version, int) or schema_version != 1:
        raise ForecastBundleError("unsupported forecast bundle schema")
    if not isinstance(bundle_version, int) or bundle_version <= 0:
        raise ForecastBundleError("bundleVersion must be positive")
    state = _text(payload.get("state"), "state")
    if state not in {"monitoring", "eligible", "blocked"}:
        raise ForecastBundleError("invalid forecast state")
    series_ids = _text_list(payload.get("seriesIds"), "seriesIds")
    shared_series_ids = _text_list(payload.get("sharedSeriesIds"), "sharedSeriesIds")
    if not set(shared_series_ids).issubset(series_ids):
        raise ForecastBundleError("sharedSeriesIds must be a subset of seriesIds")
    raw_inputs = payload.get("inputContract")
    if not isinstance(raw_inputs, list) or not raw_inputs:
        raise ForecastBundleError("inputContract must be a non-empty array")
    inputs = []
    for index, raw_input in enumerate(raw_inputs):
        if not isinstance(raw_input, dict):
            raise ForecastBundleError(f"inputContract[{index}] must be an object")
        max_age = raw_input.get("maxAgeSeconds")
        if not isinstance(max_age, int) or max_age <= 0:
            raise ForecastBundleError(f"inputContract[{index}].maxAgeSeconds must be positive")
        required = raw_input.get("required")
        if not isinstance(required, bool):
            raise ForecastBundleError(f"inputContract[{index}].required must be boolean")
        inputs.append(
            ForecastInput(
                series_id=_text(raw_input.get("seriesId"), "seriesId"),
                role=_text(raw_input.get("role"), "role"),
                required=required,
                max_age_seconds=max_age,
                source_ids=_text_list(raw_input.get("sourceIds"), "sourceIds"),
            )
        )
    if tuple(item.series_id for item in inputs) != series_ids:
        raise ForecastBundleError("seriesIds must exactly match inputContract order")
    if not any(item.required for item in inputs):
        raise ForecastBundleError("at least one forecast input must be required")
    return ForecastBundle(
        schema_version=schema_version,
        bundle_version=bundle_version,
        definition_sha256=hashlib.sha256(raw).hexdigest(),
        id=_text(payload.get("id"), "id"),
        label=_text(payload.get("label"), "label"),
        thesis=_text(payload.get("thesis"), "thesis"),
        horizon=_text(payload.get("horizon"), "horizon"),
        state=state,
        strategy_ids=_text_list(payload.get("strategyIds"), "strategyIds"),
        series_ids=series_ids,
        shared_series_ids=shared_series_ids,
        input_contract=tuple(inputs),
    )


def load_space_weather_bundle() -> ForecastBundle:
    root = Path(__file__).resolve().parents[3]
    return load_forecast_bundle(root / "config" / "forecasts" / "space-weather-impact-v1.json")
