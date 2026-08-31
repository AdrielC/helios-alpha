from __future__ import annotations

import hashlib
from pathlib import Path


def test_operator_demo_fingerprints_exact_imported_manifests() -> None:
    root = Path(__file__).resolve().parents[1]
    source = (root / "apps/operator/src/operations/time-series-port.ts").read_text(
        encoding="utf-8"
    )
    for name in ("space-weather-impact-v1.json", "execution-evidence-v1.json"):
        raw = (root / "config/forecasts" / name).read_bytes()
        assert hashlib.sha256(raw).hexdigest() in source
