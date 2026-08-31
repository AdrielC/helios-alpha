from __future__ import annotations

import hashlib
import json
from dataclasses import dataclass, field
from datetime import UTC, datetime
from typing import Any


class ShadowContractError(ValueError):
    """A source document cannot satisfy the causal shadow contract."""


def require_utc(value: datetime, field_name: str) -> datetime:
    if value.tzinfo is None or value.utcoffset() is None:
        raise ShadowContractError(f"{field_name} must be timezone-aware")
    return value.astimezone(UTC)


def canonical_json(value: Any) -> str:
    try:
        return json.dumps(
            value,
            allow_nan=False,
            ensure_ascii=False,
            separators=(",", ":"),
            sort_keys=True,
        )
    except (TypeError, ValueError) as error:
        raise ShadowContractError(f"payload is not finite canonical JSON: {error}") from error


def sha256_text(value: str) -> str:
    return hashlib.sha256(value.encode("utf-8")).hexdigest()


@dataclass(frozen=True)
class RetrievedJson:
    source_url: str
    observed_at: datetime
    payload: Any
    body_sha256: str
    etag: str | None = None
    last_modified: str | None = None

    def __post_init__(self) -> None:
        object.__setattr__(self, "observed_at", require_utc(self.observed_at, "observed_at"))
        if not self.source_url.startswith("https://"):
            raise ShadowContractError("source_url must use HTTPS")
        if len(self.body_sha256) != 64 or any(
            character not in "0123456789abcdef" for character in self.body_sha256
        ):
            raise ShadowContractError("body_sha256 must be lowercase SHA-256")
        canonical_json(self.payload)

    @property
    def snapshot_id(self) -> str:
        identity = canonical_json(
            {
                "bodySha256": self.body_sha256,
                "observedAt": self.observed_at.isoformat(),
                "sourceUrl": self.source_url,
            }
        )
        return sha256_text(identity)


@dataclass(frozen=True)
class ShadowCandidate:
    natural_key: str
    event_time: datetime
    payload: dict[str, Any]
    available_at: datetime | None = None
    quality_flags: tuple[str, ...] = field(default_factory=tuple)

    def __post_init__(self) -> None:
        if not self.natural_key.strip():
            raise ShadowContractError("natural_key must not be empty")
        object.__setattr__(self, "event_time", require_utc(self.event_time, "event_time"))
        if self.available_at is not None:
            object.__setattr__(
                self,
                "available_at",
                require_utc(self.available_at, "available_at"),
            )
        canonical_json(self.payload)
        if any(not flag.strip() for flag in self.quality_flags):
            raise ShadowContractError("quality flags must not be empty")

    @property
    def payload_json(self) -> str:
        return canonical_json(self.payload)

    @property
    def payload_sha256(self) -> str:
        return sha256_text(self.payload_json)


@dataclass(frozen=True)
class ShadowObservation:
    sequence: int
    source_id: str
    source_offset: int
    natural_key: str
    revision: int
    event_time: datetime
    available_at: datetime
    observed_at: datetime
    payload: dict[str, Any]
    payload_sha256: str
    source_url: str
    snapshot_id: str
    quality_flags: tuple[str, ...]

    @property
    def event_id(self) -> str:
        return f"shadow:v1:{self.source_id}:{self.source_offset}:{self.payload_sha256}"


@dataclass(frozen=True)
class ShadowCheckpoint:
    source_id: str
    last_offset: int
    observed_at: datetime
    max_event_time: datetime | None
    snapshot_id: str
    body_sha256: str
