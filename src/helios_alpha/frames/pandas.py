"""Thin pandas adapter. Polars remains authoritative."""

from __future__ import annotations

from datetime import datetime

import pandas as pd
import polars as pl

from helios_alpha.frames.core import HelioFrame


@pd.api.extensions.register_dataframe_accessor("helio")
class HelioAccessor:
    def __init__(self, pandas_obj: pd.DataFrame) -> None:
        self._obj = pandas_obj

    def to_polars(self) -> pl.DataFrame:
        return HelioFrame.from_pandas(self._obj).data

    def validate(self, *, require_contiguous: bool = True) -> None:
        HelioFrame.from_pandas(self._obj).validate(require_contiguous=require_contiguous)

    def as_of(self, cut: datetime) -> pd.DataFrame:
        return HelioFrame.from_pandas(self._obj).as_of(cut).to_pandas()
