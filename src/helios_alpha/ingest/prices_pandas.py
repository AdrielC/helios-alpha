"""
Pandas representation of daily OHLCV for calendar / session alignment.

Polars stays the default for bulk IO; convert at boundaries when you need
``CustomBusinessDay`` / ``CustomBusinessHour`` or ``exchange_calendars``.
"""

from __future__ import annotations

from datetime import date

import pandas as pd
import polars as pl


def daily_prices_to_pandas(df: pl.DataFrame) -> pd.DataFrame:
    if df.is_empty():
        return pd.DataFrame(columns=["date", "ticker", "open", "high", "low", "close", "volume"])
    pdf = df.to_pandas()
    if "date" in pdf.columns:
        pdf["date"] = pd.to_datetime(pdf["date"]).dt.normalize()
    return pdf


def filter_prices_as_of(pdf: pd.DataFrame, as_of: date) -> pd.DataFrame:
    if pdf.empty:
        return pdf
    cutoff = pd.Timestamp(as_of).normalize()
    return pdf.loc[pdf["date"] <= cutoff].copy()


def filter_polars_prices_as_of(df: pl.DataFrame, as_of: date) -> pl.DataFrame:
    if df.is_empty():
        return df
    return df.filter(pl.col("date") <= pl.lit(as_of))
