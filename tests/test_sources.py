from __future__ import annotations

import asyncio
from datetime import UTC, datetime, timedelta

import pytest

from helios_alpha.sources import (
    BackfillRequest,
    InMemorySource,
    SourceContinuityError,
    SourceCursor,
    SourceEnvelope,
    SourceError,
    SourceIdentity,
    SourcePhase,
)


def _fixture_records(offsets: tuple[int, ...] = (1, 2, 3)) -> tuple[SourceEnvelope[dict], ...]:
    identity = SourceIdentity("fixture", "events", "v1")
    base = datetime(2026, 1, 1, tzinfo=UTC)
    return tuple(
        SourceEnvelope(
            identity=identity,
            partition="p0",
            offset=offset,
            event_time=base + timedelta(seconds=offset - 1),
            available_at=base + timedelta(seconds=offset),
            observed_at=base + timedelta(seconds=offset, milliseconds=1),
            phase=SourcePhase.BACKFILL,
            payload={"value": offset},
        )
        for offset in offsets
    )


def test_rewind_and_stitched_handoff_share_one_exact_prefix() -> None:
    records = _fixture_records()
    source = InMemorySource(records[0].identity, records)
    cursor = source.rewind(records[1].available_at)
    assert cursor.position("p0") == 1
    request = BackfillRequest(
        start_available_at=None,
        end_available_at=records[1].available_at,
        as_of=records[1].available_at,
        cursor=SourceCursor(records[0].identity),
    )

    async def collect() -> list[SourceEnvelope[dict]]:
        return [record async for record in source.stitched(request)]

    stitched = asyncio.run(collect())
    assert [record.offset for record in stitched] == [1, 2, 3]
    assert [record.phase for record in stitched] == [
        SourcePhase.BACKFILL,
        SourcePhase.BACKFILL,
        SourcePhase.LIVE,
    ]


def test_gap_is_rejected_in_replay() -> None:
    records = _fixture_records((1, 3))
    source = InMemorySource(records[0].identity, records)
    request = BackfillRequest(
        start_available_at=None,
        end_available_at=records[-1].available_at,
        as_of=records[-1].available_at,
        cursor=SourceCursor(records[0].identity),
    )
    with pytest.raises(SourceContinuityError, match="jumped from 1 to 3"):
        list(source.replay(request))


def test_backfill_cut_cannot_include_future_information() -> None:
    records = _fixture_records()
    with pytest.raises(SourceError, match="must not exceed"):
        BackfillRequest(
            start_available_at=None,
            end_available_at=records[2].available_at,
            as_of=records[1].available_at,
            cursor=SourceCursor(records[0].identity),
        )


def test_backfill_request_rejects_naive_clocks() -> None:
    records = _fixture_records()
    with pytest.raises(SourceError, match="timezone-aware"):
        BackfillRequest(
            start_available_at=None,
            end_available_at=datetime(2026, 1, 1),
            as_of=datetime(2026, 1, 1),
            cursor=SourceCursor(records[0].identity),
        )


def test_replay_rejects_an_adapter_record_outside_the_requested_range() -> None:
    records = _fixture_records()

    class UnfencedSource(InMemorySource[dict]):
        def backfill(self, request: BackfillRequest):
            del request
            yield records[0]

    source = UnfencedSource(records[0].identity, records)
    request = BackfillRequest(
        start_available_at=records[1].available_at,
        end_available_at=records[2].available_at,
        as_of=records[2].available_at,
        cursor=SourceCursor(records[0].identity),
    )

    with pytest.raises(SourceError, match="before the requested backfill start"):
        list(source.replay(request))
