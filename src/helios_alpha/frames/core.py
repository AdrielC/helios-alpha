"""Polars-authoritative point-in-time event frame."""

from __future__ import annotations

import json
from collections.abc import Iterable
from dataclasses import dataclass
from datetime import datetime
from typing import Any

import pandas as pd
import polars as pl

from helios_alpha.sources import SourceEnvelope

FRAME_SCHEMA_VERSION = "1"
KEY_COLUMNS = ("source", "partition", "offset")
TIME_ZONE = "UTC"


class HelioFrameError(ValueError):
    """Canonical frame invariant failure."""


def canonical_schema() -> dict[str, pl.DataType]:
    return {
        "source": pl.Utf8,
        "partition": pl.Utf8,
        "offset": pl.UInt64,
        "event_time": pl.Datetime("us", TIME_ZONE),
        "available_at": pl.Datetime("us", TIME_ZONE),
        "observed_at": pl.Datetime("us", TIME_ZONE),
        "event_type": pl.Utf8,
        "instrument": pl.Utf8,
        "value": pl.Float64,
        "unit": pl.Utf8,
        "payload_json": pl.Utf8,
    }


def _json_payload(payload: Any) -> str:
    if isinstance(payload, str):
        return payload
    return json.dumps(payload, sort_keys=True, separators=(",", ":"), default=str)


@dataclass(frozen=True)
class HelioFrame:
    """Validated Polars frame with availability-aware selection helpers."""

    data: pl.DataFrame

    def __post_init__(self) -> None:
        object.__setattr__(self, "data", self._normalize(self.data))
        self.validate()

    @classmethod
    def empty(cls) -> HelioFrame:
        return cls(pl.DataFrame(schema=canonical_schema()))

    @classmethod
    def from_envelopes(
        cls,
        records: Iterable[SourceEnvelope[Any]],
        *,
        event_type: str,
        instrument: str | None = None,
        value_field: str | None = None,
        unit: str | None = None,
    ) -> HelioFrame:
        rows: list[dict[str, Any]] = []
        for record in records:
            payload = record.payload
            value = None
            if value_field is not None and isinstance(payload, dict):
                candidate = payload.get(value_field)
                value = float(candidate) if candidate is not None else None
            rows.append(
                {
                    "source": record.identity.key,
                    "partition": record.partition,
                    "offset": record.offset,
                    "event_time": record.event_time,
                    "available_at": record.available_at,
                    "observed_at": record.observed_at,
                    "event_type": event_type,
                    "instrument": instrument,
                    "value": value,
                    "unit": unit,
                    "payload_json": _json_payload(payload),
                }
            )
        data = (
            pl.DataFrame(rows, schema=canonical_schema())
            if rows
            else pl.DataFrame(schema=canonical_schema())
        )
        return cls(data)

    @classmethod
    def from_pandas(cls, frame: pd.DataFrame) -> HelioFrame:
        return cls(pl.from_pandas(frame.reset_index(drop=True)))

    @staticmethod
    def _normalize(frame: pl.DataFrame) -> pl.DataFrame:
        missing = [column for column in canonical_schema() if column not in frame.columns]
        if missing:
            raise HelioFrameError(f"missing canonical columns: {', '.join(missing)}")
        expressions = [
            pl.col(name).cast(dtype, strict=False)
            for name, dtype in canonical_schema().items()
        ]
        return frame.select(expressions).sort(["available_at", "source", "partition", "offset"])

    def validate(self, *, require_contiguous: bool = True) -> None:
        if self.data.is_empty():
            return
        required = [
            "source",
            "partition",
            "offset",
            "event_time",
            "available_at",
            "observed_at",
            "event_type",
        ]
        null_counts = self.data.select(pl.col(required).null_count()).row(0)
        if any(count for count in null_counts):
            raise HelioFrameError("canonical identity and time columns may not be null")
        duplicates = self.data.group_by(list(KEY_COLUMNS)).len().filter(pl.col("len") > 1)
        if not duplicates.is_empty():
            raise HelioFrameError("duplicate source, partition, offset identity")
        if not self.data.filter(pl.col("observed_at") < pl.col("available_at")).is_empty():
            raise HelioFrameError("observed_at must not precede available_at")
        for group in self.data.partition_by(["source", "partition"], maintain_order=True):
            ordered = group.sort("offset")
            offsets = ordered["offset"].to_list()
            if require_contiguous and any(
                right != left + 1 for left, right in zip(offsets, offsets[1:])
            ):
                raise HelioFrameError("source offsets must be contiguous within each partition")
            availability = ordered["available_at"].to_list()
            if any(right < left for left, right in zip(availability, availability[1:])):
                raise HelioFrameError("availability must not regress within a source partition")

    def as_of(self, cut: datetime) -> HelioFrame:
        if cut.tzinfo is None or cut.utcoffset() is None:
            raise HelioFrameError("as_of cut must be timezone-aware")
        return HelioFrame(self.data.filter(pl.col("available_at") <= pl.lit(cut)))

    def between(self, start: datetime, end: datetime) -> HelioFrame:
        if start > end:
            raise HelioFrameError("start must not exceed end")
        return HelioFrame(
            self.data.filter(
                pl.col("available_at").is_between(
                    pl.lit(start), pl.lit(end), closed="both"
                )
            )
        )

    def to_pandas(self) -> pd.DataFrame:
        frame = self.data.to_pandas()
        frame.attrs["helios.schema_version"] = FRAME_SCHEMA_VERSION
        return frame
