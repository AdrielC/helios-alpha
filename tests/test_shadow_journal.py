from __future__ import annotations

import hashlib
from datetime import UTC, datetime, timedelta

import pytest

from helios_alpha.shadow import RetrievedJson, ShadowCandidate, ShadowJournal
from helios_alpha.shadow.models import ShadowContractError


def at(minute: int) -> datetime:
    return datetime(2026, 8, 31, 12, minute, tzinfo=UTC)


def document(observed_at: datetime, payload: str) -> RetrievedJson:
    return RetrievedJson(
        source_url="https://example.test/feed.json",
        observed_at=observed_at,
        payload={"snapshot": payload},
        body_sha256=hashlib.sha256(payload.encode()).hexdigest(),
    )


def candidate(value: float, available_at: datetime | None = None) -> ShadowCandidate:
    return ShadowCandidate(
        natural_key="sample-1",
        event_time=at(0),
        available_at=available_at,
        payload={"value": value},
    )


def test_journal_atomically_checkpoints_append_only_revisions(tmp_path) -> None:
    with ShadowJournal(tmp_path / "shadow.sqlite3") as journal:
        first = journal.ingest("feed-v1", document(at(2), "one"), [candidate(1.0, at(1))])
        assert [(item.source_offset, item.revision) for item in first] == [(1, 1)]
        assert first[0].available_at == at(1)

        assert journal.ingest(
            "feed-v1", document(at(2), "one"), [candidate(1.0, at(1))]
        ) == []
        assert journal.ingest(
            "feed-v1", document(at(3), "same-values"), [candidate(1.0, at(1))]
        ) == []

        changed = journal.ingest(
            "feed-v1", document(at(4), "two"), [candidate(2.0, at(4))]
        )
        reverted = journal.ingest(
            "feed-v1", document(at(5), "three"), [candidate(1.0, at(5))]
        )

        assert (changed[0].source_offset, changed[0].revision) == (2, 2)
        assert (reverted[0].source_offset, reverted[0].revision) == (3, 3)
        assert [item.source_offset for item in journal.observations_after("feed-v1", 0)] == [
            1,
            2,
            3,
        ]
        checkpoint = journal.checkpoint("feed-v1")
        assert checkpoint is not None
        assert checkpoint.last_offset == 3
        assert checkpoint.snapshot_id == document(at(5), "three").snapshot_id


def test_journal_rejects_future_publication_and_duplicate_snapshot_keys(tmp_path) -> None:
    with ShadowJournal(tmp_path / "shadow.sqlite3") as journal:
        with pytest.raises(ShadowContractError, match="must not exceed"):
            journal.ingest(
                "feed-v1",
                document(at(2), "future"),
                [candidate(1.0, at(3))],
            )
        assert journal.checkpoint("feed-v1") is None

        duplicate = ShadowCandidate(
            natural_key="sample-1",
            event_time=at(0) + timedelta(seconds=1),
            payload={"value": 2.0},
        )
        with pytest.raises(ShadowContractError, match="duplicate natural keys"):
            journal.ingest(
                "feed-v1",
                document(at(2), "duplicates"),
                [candidate(1.0), duplicate],
            )
        assert journal.checkpoint("feed-v1") is None


def test_observation_payload_rejects_non_finite_numbers() -> None:
    with pytest.raises(ShadowContractError, match="finite canonical JSON"):
        candidate(float("nan"))
