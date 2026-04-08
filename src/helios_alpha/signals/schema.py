from __future__ import annotations

import uuid
from enum import StrEnum
from typing import Any

from pydantic import BaseModel, Field


class SignalKind(StrEnum):
    """Coarse action hint for subscribers (not an order)."""

    watch = "watch"
    warning = "warning"
    storm = "storm"
    clear = "clear"
    bar = "bar"
    custom = "custom"


class HeliosSignalV1(BaseModel):
    """
    Versioned broadcast envelope. JSON-serializable for ZMQ / Redis / NATS.

    Subscribers should dedupe on ``signal_id`` and respect ``causal_ts_utc``
    for replay / backtest alignment.
    """

    schema_version: str = Field(default="1")
    signal_id: str = Field(default_factory=lambda: str(uuid.uuid4()))
    emitted_at_utc: str = Field(
        ...,
        description="Set by SignalPublisher or test fixtures; not auto-filled here.",
    )
    causal_ts_utc: str | None = None
    source: str = Field(..., description="e.g. helios_alpha.ssi, helios_alpha.rules")
    kind: SignalKind
    topic_suffix: str = Field(
        default="default",
        description="Subtopic for ZMQ filter, e.g. ssi, execution.intent",
    )
    payload: dict[str, Any] = Field(default_factory=dict)
    requires_ack: bool = False

    def to_json_bytes(self) -> bytes:
        return self.model_dump_json().encode("utf-8")

    @classmethod
    def watch(
        cls,
        source: str,
        payload: dict[str, Any] | None = None,
        *,
        topic_suffix: str = "default",
    ) -> HeliosSignalV1:
        return cls(
            source=source,
            kind=SignalKind.watch,
            payload=payload or {},
            topic_suffix=topic_suffix,
        )

    @classmethod
    def warning(
        cls,
        source: str,
        payload: dict[str, Any] | None = None,
        *,
        topic_suffix: str = "default",
    ) -> HeliosSignalV1:
        return cls(
            source=source,
            kind=SignalKind.warning,
            payload=payload or {},
            topic_suffix=topic_suffix,
        )

    @classmethod
    def storm(
        cls,
        source: str,
        payload: dict[str, Any] | None = None,
        *,
        topic_suffix: str = "default",
    ) -> HeliosSignalV1:
        return cls(
            source=source,
            kind=SignalKind.storm,
            payload=payload or {},
            topic_suffix=topic_suffix,
        )

    @classmethod
    def clear(
        cls,
        source: str,
        payload: dict[str, Any] | None = None,
        *,
        topic_suffix: str = "default",
    ) -> HeliosSignalV1:
        return cls(
            source=source,
            kind=SignalKind.clear,
            payload=payload or {},
            topic_suffix=topic_suffix,
        )
