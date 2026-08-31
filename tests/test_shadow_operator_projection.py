from __future__ import annotations

import hashlib
import json
from datetime import UTC, datetime

from helios_alpha.shadow import RetrievedJson, ShadowCandidate, ShadowJournal
from helios_alpha.shadow.operator_projection import (
    build_operator_projection,
    write_operator_projection,
)


def at(minute: int) -> datetime:
    return datetime(2026, 8, 31, 12, minute, tzinfo=UTC)


def document(minute: int, label: str) -> RetrievedJson:
    return RetrievedJson(
        source_url="https://example.test/feed.json",
        observed_at=at(minute),
        payload={"snapshot": label},
        body_sha256=hashlib.sha256(label.encode()).hexdigest(),
    )


def xray(value: float, available_at: datetime) -> ShadowCandidate:
    return ShadowCandidate(
        natural_key="xray-1",
        event_time=at(0),
        available_at=available_at,
        payload={
            "seriesId": "goes-xray-flux",
            "fluxWattsPerSquareMeter": value,
        },
    )


def test_projection_uses_latest_revision_and_writes_atomically(tmp_path) -> None:
    with ShadowJournal(tmp_path / "shadow.sqlite3") as journal:
        journal.ingest("xray-v1", document(1, "initial"), [xray(1e-6, at(1))])
        journal.ingest("xray-v1", document(2, "revision"), [xray(2e-6, at(2))])
        projection = build_operator_projection(journal, projection_id="shadow-test")

    assert projection["projectionId"] == "shadow-test"
    assert projection["sequence"] == 2
    assert projection["observedAt"] == "2026-08-31T12:02:00Z"
    assert projection["series"] == [
        {
            "id": "goes-xray-flux",
            "points": [
                {
                    "kind": "scalar",
                    "timestamp": "2026-08-31T12:00:00Z",
                    "availableAt": "2026-08-31T12:02:00Z",
                    "value": 2e-6,
                }
            ],
        }
    ]

    target = tmp_path / "operator" / "projection.json"
    write_operator_projection(target, projection)
    assert json.loads(target.read_text(encoding="utf-8")) == projection
    assert target.stat().st_mode & 0o777 == 0o600
