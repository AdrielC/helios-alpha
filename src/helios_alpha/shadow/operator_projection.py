from __future__ import annotations

import json
import math
import os
import tempfile
from collections import defaultdict
from datetime import datetime
from pathlib import Path
from typing import Any

from helios_alpha.shadow.journal import ShadowJournal
from helios_alpha.shadow.models import ShadowContractError, ShadowObservation

_VALUE_FIELDS = {
    "goes-xray-flux": "fluxWattsPerSquareMeter",
    "goes-proton-flux-ge10": "fluxPfu",
    "donki-cme-analysis": "speedKms",
    "l1-solar-wind-speed": "speedKms",
    "l1-imf-bz-gsm": "bzGsmNt",
    "planetary-kp": "estimatedKp",
}
_EVENT_SERIES = {"donki-flare-events"}


def _time(value: datetime) -> str:
    return value.isoformat().replace("+00:00", "Z")


def _scalar_value(observation: ShadowObservation) -> float | None:
    series_id = observation.payload.get("seriesId")
    if series_id in _EVENT_SERIES:
        return 1.0
    field = _VALUE_FIELDS.get(series_id)
    if field is None:
        return None
    raw = observation.payload.get(field)
    if raw is None:
        return None
    if isinstance(raw, bool) or not isinstance(raw, (int, float)):
        raise ShadowContractError(f"{series_id}.{field} must be numeric")
    value = float(raw)
    if not math.isfinite(value):
        raise ShadowContractError(f"{series_id}.{field} must be finite")
    return value


def build_operator_projection(
    journal: ShadowJournal,
    projection_id: str = "scientific-shadow",
    max_observations: int = 100_000,
    max_points_per_series: int = 5_000,
) -> dict[str, Any]:
    if not projection_id.strip():
        raise ShadowContractError("projection_id must not be empty")
    if not 1 <= max_points_per_series <= 100_000:
        raise ShadowContractError("invalid per-series point limit")
    observed_at = journal.latest_observed_at()
    observations = journal.latest_observations(max_observations)
    if observed_at is None or not observations:
        raise ShadowContractError("operator projection requires at least one observation")

    by_series: dict[str, dict[str, tuple[int, dict[str, Any]]]] = defaultdict(dict)
    for observation in observations:
        series_id = observation.payload.get("seriesId")
        if not isinstance(series_id, str) or not series_id:
            raise ShadowContractError("shadow payload seriesId must not be empty")
        if observation.payload.get("active") is False:
            continue
        value = _scalar_value(observation)
        if value is None:
            continue
        timestamp = _time(observation.event_time)
        by_series[series_id][timestamp] = (
            observation.sequence,
            {
                "kind": "scalar",
                "timestamp": timestamp,
                "availableAt": _time(observation.available_at),
                "value": value,
            },
        )

    series = []
    for series_id in sorted(by_series):
        ordered = sorted(
            by_series[series_id].values(),
            key=lambda item: (item[1]["timestamp"], item[0]),
        )
        points = [point for _, point in ordered[-max_points_per_series:]]
        if points:
            series.append({"id": series_id, "points": points})
    if not series:
        raise ShadowContractError("operator projection contains no scalar series")
    return {
        "schemaVersion": 1,
        "projectionId": projection_id,
        "sequence": max(observation.sequence for observation in observations),
        "observedAt": _time(observed_at),
        "series": series,
    }


def write_operator_projection(path: Path, projection: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    encoded = json.dumps(
        projection,
        allow_nan=False,
        ensure_ascii=False,
        separators=(",", ":"),
        sort_keys=True,
    ).encode("utf-8")
    descriptor, temporary = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    try:
        os.fchmod(descriptor, 0o600)
        with os.fdopen(descriptor, "wb") as stream:
            stream.write(encoded)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
        directory = os.open(path.parent, os.O_RDONLY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    except Exception:
        try:
            os.unlink(temporary)
        except FileNotFoundError:
            pass
        raise
