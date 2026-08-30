from __future__ import annotations

from datetime import UTC, datetime, timedelta

import pandas as pd
import polars as pl
import pytest

from helios_alpha.frames import FRAME_SCHEMA_VERSION, HelioFrame, HelioFrameError
from helios_alpha.sources import SourceEnvelope, SourceIdentity, SourcePhase


def _frame() -> HelioFrame:
    base = datetime(2026, 1, 1, tzinfo=UTC)
    identity = SourceIdentity("fixture", "market", "trades")
    records = [
        SourceEnvelope(
            identity=identity,
            partition="BTC-USD",
            offset=offset,
            event_time=base + timedelta(seconds=offset),
            available_at=base + timedelta(seconds=offset, milliseconds=1),
            observed_at=base + timedelta(seconds=offset, milliseconds=2),
            phase=SourcePhase.BACKFILL,
            payload={"price": 100.0 + offset},
        )
        for offset in (1, 2, 3)
    ]
    return HelioFrame.from_envelopes(
        records,
        event_type="trade",
        instrument="BTC-USD",
        value_field="price",
        unit="USD",
    )


def test_polars_frame_is_authoritative_and_as_of_uses_availability() -> None:
    frame = _frame()
    cut = frame.data["available_at"][1]
    selected = frame.as_of(cut)
    assert selected.data["offset"].to_list() == [1, 2]


def test_pandas_accessor_is_thin_and_preserves_schema_version() -> None:
    pdf = _frame().to_pandas()
    assert isinstance(pdf, pd.DataFrame)
    pdf.helio.validate()
    assert pdf.helio.to_polars()["offset"].to_list() == [1, 2, 3]
    selected = pdf.helio.as_of(pdf["available_at"].iloc[1])
    assert selected.attrs["helios.schema_version"] == FRAME_SCHEMA_VERSION


def test_frame_rejects_offset_gap() -> None:
    frame = _frame().data.filter(pl.col("offset") != 2)
    with pytest.raises(HelioFrameError, match="contiguous"):
        HelioFrame(frame)
