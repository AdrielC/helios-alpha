"""One source model for historical replay, restart catch-up, and live tails.

Adapters own transport. This module owns causal time, exact per-partition offsets, bounded replay,
and the handoff contract. A provider that cannot resume strictly after a checkpoint is not allowed
to claim a gap-free backfill-to-live stream.
"""

from __future__ import annotations

from abc import ABC, abstractmethod
from collections.abc import AsyncIterator, Iterator, Mapping, Sequence
from dataclasses import dataclass, field
from datetime import datetime
from enum import StrEnum
from typing import Generic, TypeVar

T = TypeVar("T")


class SourceError(RuntimeError):
    """Base source contract failure."""


class SourceContinuityError(SourceError):
    """A source prefix contained a gap or availability regression."""


class SourcePhase(StrEnum):
    BACKFILL = "backfill"
    LIVE = "live"


@dataclass(frozen=True)
class SourceIdentity:
    provider: str
    dataset: str
    schema: str

    @property
    def key(self) -> str:
        return f"{self.provider}:{self.dataset}:{self.schema}"


@dataclass(frozen=True)
class SourceCapabilities:
    backfill: bool
    live: bool
    resumable: bool
    rewindable: bool
    atomic_handoff: bool = False


@dataclass(frozen=True)
class SourceEnvelope(Generic[T]):
    identity: SourceIdentity
    partition: str
    offset: int
    event_time: datetime
    available_at: datetime
    observed_at: datetime
    phase: SourcePhase
    payload: T

    def __post_init__(self) -> None:
        for name, value in (
            ("event_time", self.event_time),
            ("available_at", self.available_at),
            ("observed_at", self.observed_at),
        ):
            if value.tzinfo is None or value.utcoffset() is None:
                raise SourceError(f"{name} must be timezone-aware")
        if self.offset < 0:
            raise SourceError("offset must be non-negative")
        if self.observed_at < self.available_at:
            raise SourceError("observed_at must not precede available_at")


@dataclass(frozen=True)
class SourceCursor:
    identity: SourceIdentity
    positions: Mapping[str, int] = field(default_factory=dict)
    availability: Mapping[str, datetime] = field(default_factory=dict)

    def position(self, partition: str) -> int | None:
        return self.positions.get(partition)


@dataclass(frozen=True)
class BackfillRequest:
    start_available_at: datetime | None
    end_available_at: datetime
    as_of: datetime
    cursor: SourceCursor

    def __post_init__(self) -> None:
        for name, value in (
            ("start_available_at", self.start_available_at),
            ("end_available_at", self.end_available_at),
            ("as_of", self.as_of),
        ):
            if value is not None and (value.tzinfo is None or value.utcoffset() is None):
                raise SourceError(f"{name} must be timezone-aware")
        if self.end_available_at > self.as_of:
            raise SourceError("backfill end must not exceed its causal as_of cut")
        if self.start_available_at is not None and self.start_available_at > self.end_available_at:
            raise SourceError("backfill start must not exceed end")


class _CursorTracker:
    def __init__(self, cursor: SourceCursor) -> None:
        self.identity = cursor.identity
        self.positions = dict(cursor.positions)
        self.availability = dict(cursor.availability)

    @property
    def cursor(self) -> SourceCursor:
        return SourceCursor(self.identity, dict(self.positions), dict(self.availability))

    def accept(self, record: SourceEnvelope[T]) -> bool:
        if record.identity != self.identity:
            raise SourceContinuityError(
                f"foreign record {record.identity.key} on source {self.identity.key}"
            )
        previous = self.positions.get(record.partition)
        if previous is not None:
            if record.offset <= previous:
                return False
            if record.offset != previous + 1:
                raise SourceContinuityError(
                    f"partition {record.partition} jumped from {previous} to {record.offset}"
                )
        previous_available = self.availability.get(record.partition)
        if previous_available is not None and record.available_at < previous_available:
            raise SourceContinuityError(
                f"partition {record.partition} availability regressed from "
                f"{previous_available.isoformat()} to {record.available_at.isoformat()}"
            )
        self.positions[record.partition] = record.offset
        self.availability[record.partition] = record.available_at
        return True


class HelioSource(ABC, Generic[T]):
    """Master source interface specialized by market, weather, or scientific adapters.

    `replay` and `stitched` apply the same continuity checks. `stitched` is gap-free only when the
    adapter advertises resumability and its `live` method resumes strictly after the supplied
    cursor. Providers with a true subscribe-first fence may override `stitched` and advertise
    `atomic_handoff=True`.
    """

    @property
    @abstractmethod
    def identity(self) -> SourceIdentity:
        raise NotImplementedError

    @property
    @abstractmethod
    def capabilities(self) -> SourceCapabilities:
        raise NotImplementedError

    @abstractmethod
    def backfill(self, request: BackfillRequest) -> Iterator[SourceEnvelope[T]]:
        raise NotImplementedError

    @abstractmethod
    async def live(self, cursor: SourceCursor) -> AsyncIterator[SourceEnvelope[T]]:
        raise NotImplementedError
        yield  # pragma: no cover

    @abstractmethod
    def rewind(self, available_at: datetime) -> SourceCursor:
        """Return the prefix immediately before `available_at`."""
        raise NotImplementedError

    def replay(self, request: BackfillRequest) -> Iterator[SourceEnvelope[T]]:
        if not self.capabilities.backfill:
            raise SourceError(f"{self.identity.key} does not support backfill")
        if request.cursor.identity != self.identity:
            raise SourceError("cursor belongs to a different source")
        tracker = _CursorTracker(request.cursor)
        for record in self.backfill(request):
            self._validate_backfill_record(request, record)
            if tracker.accept(record):
                yield record

    async def stitched(self, request: BackfillRequest) -> AsyncIterator[SourceEnvelope[T]]:
        if (
            not self.capabilities.backfill
            or not self.capabilities.live
            or not self.capabilities.resumable
        ):
            raise SourceError(f"{self.identity.key} cannot prove a gap-free live handoff")
        if request.cursor.identity != self.identity:
            raise SourceError("cursor belongs to a different source")
        tracker = _CursorTracker(request.cursor)
        for record in self.backfill(request):
            self._validate_backfill_record(request, record)
            if tracker.accept(record):
                yield record
        async for record in self.live(tracker.cursor):
            if record.phase is not SourcePhase.LIVE:
                raise SourceError("live tail emitted a non-live record")
            if tracker.accept(record):
                yield record

    @staticmethod
    def _validate_backfill_record(
        request: BackfillRequest, record: SourceEnvelope[T]
    ) -> None:
        if record.phase is not SourcePhase.BACKFILL:
            raise SourceError("backfill emitted a non-backfill record")
        if record.available_at > request.as_of:
            raise SourceError("source emitted information after the replay as_of cut")
        if record.available_at > request.end_available_at:
            raise SourceError("source emitted information after the requested backfill end")
        if (
            request.start_available_at is not None
            and record.available_at < request.start_available_at
        ):
            raise SourceError("source emitted information before the requested backfill start")


class InMemorySource(HelioSource[T]):
    """Deterministic adapter used to prove replay, rewind, and live handoff semantics."""

    def __init__(self, identity: SourceIdentity, records: Sequence[SourceEnvelope[T]]) -> None:
        self._identity = identity
        self._records = tuple(
            sorted(records, key=lambda row: (row.available_at, row.partition, row.offset))
        )

    @property
    def identity(self) -> SourceIdentity:
        return self._identity

    @property
    def capabilities(self) -> SourceCapabilities:
        return SourceCapabilities(True, True, True, True, True)

    def backfill(self, request: BackfillRequest) -> Iterator[SourceEnvelope[T]]:
        for record in self._records:
            if record.available_at > request.end_available_at:
                continue
            if (
                request.start_available_at is not None
                and record.available_at < request.start_available_at
            ):
                continue
            yield SourceEnvelope(
                identity=record.identity,
                partition=record.partition,
                offset=record.offset,
                event_time=record.event_time,
                available_at=record.available_at,
                observed_at=record.observed_at,
                phase=SourcePhase.BACKFILL,
                payload=record.payload,
            )

    async def live(self, cursor: SourceCursor) -> AsyncIterator[SourceEnvelope[T]]:
        for record in self._records:
            previous = cursor.position(record.partition)
            if previous is not None and record.offset <= previous:
                continue
            yield SourceEnvelope(
                identity=record.identity,
                partition=record.partition,
                offset=record.offset,
                event_time=record.event_time,
                available_at=record.available_at,
                observed_at=record.observed_at,
                phase=SourcePhase.LIVE,
                payload=record.payload,
            )

    def rewind(self, available_at: datetime) -> SourceCursor:
        tracker = _CursorTracker(SourceCursor(self.identity))
        for record in self._records:
            if record.available_at >= available_at:
                break
            tracker.accept(record)
        return tracker.cursor
