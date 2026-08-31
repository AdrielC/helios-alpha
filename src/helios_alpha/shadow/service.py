from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime

from helios_alpha.shadow.adapters import JsonTransport, ShadowFeed
from helios_alpha.shadow.journal import ShadowJournal
from helios_alpha.shadow.models import ShadowContractError


@dataclass(frozen=True)
class PollReceipt:
    source_id: str
    snapshot_id: str
    observed_at: datetime
    normalized: int
    considered: int
    inserted: int
    last_offset: int


class ShadowIngestionService:
    def __init__(self, journal: ShadowJournal, transport: JsonTransport) -> None:
        self._journal = journal
        self._transport = transport

    def poll(self, feed: ShadowFeed, today: datetime) -> PollReceipt:
        document, candidates = feed.fetch(self._transport, today.date())
        checkpoint = self._journal.checkpoint(feed.source_id)
        if checkpoint and document.observed_at < checkpoint.observed_at:
            raise ShadowContractError("source observation clock regressed")

        if checkpoint and checkpoint.max_event_time:
            event_cutoff = checkpoint.max_event_time - feed.revision_lookback
            availability_cutoff = checkpoint.observed_at - feed.revision_lookback
        else:
            event_cutoff = document.observed_at - feed.initial_lookback
            availability_cutoff = event_cutoff
        considered = [
            candidate
            for candidate in candidates
            if candidate.event_time >= event_cutoff
            or (
                candidate.available_at is not None
                and candidate.available_at >= availability_cutoff
            )
        ]
        inserted = self._journal.ingest(feed.source_id, document, considered)
        next_checkpoint = self._journal.checkpoint(feed.source_id)
        if next_checkpoint is None:
            raise ShadowContractError("journal did not advance source checkpoint")
        return PollReceipt(
            source_id=feed.source_id,
            snapshot_id=document.snapshot_id,
            observed_at=document.observed_at,
            normalized=len(candidates),
            considered=len(considered),
            inserted=len(inserted),
            last_offset=next_checkpoint.last_offset,
        )
