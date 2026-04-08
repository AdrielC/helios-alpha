from __future__ import annotations

from datetime import date
from pathlib import Path

import pandas as pd
import polars as pl
import yfinance as yf

from helios_alpha.config import load_settings


def download_daily_prices(
    tickers: list[str],
    start: date,
    end: date,
    *,
    auto_adjust: bool = True,
) -> pl.DataFrame:
    """Download split-adjusted daily OHLCV via yfinance; long schema per ticker."""
    if not tickers:
        return pl.DataFrame(
            schema={
                "date": pl.Date,
                "ticker": pl.Utf8,
                "open": pl.Float64,
                "high": pl.Float64,
                "low": pl.Float64,
                "close": pl.Float64,
                "volume": pl.Float64,
            }
        )
    start_s = start.isoformat()
    end_s = end.isoformat()
    frames: list[pl.DataFrame] = []
    for t in tickers:
        df = yf.download(
            t,
            start=start_s,
            end=end_s,
            progress=False,
            auto_adjust=auto_adjust,
            threads=False,
        )
        if df is None or df.empty:
            continue
        if isinstance(df.columns, pd.MultiIndex):
            df.columns = df.columns.droplevel(1)
        df = df.reset_index()
        date_col = "Date" if "Date" in df.columns else df.columns[0]
        pldf = pl.from_pandas(df)
        rename_map = {
            date_col: "date",
            "Open": "open",
            "High": "high",
            "Low": "low",
            "Close": "close",
            "Volume": "volume",
        }
        pldf = pldf.rename({k: v for k, v in rename_map.items() if k in pldf.columns})
        pldf = pldf.with_columns(
            pl.col("date").cast(pl.Date),
            pl.lit(t).alias("ticker"),
        )
        cols = ["date", "ticker", "open", "high", "low", "close", "volume"]
        frames.append(pldf.select([c for c in cols if c in pldf.columns]))
    if not frames:
        return pl.DataFrame(
            schema={
                "date": pl.Date,
                "ticker": pl.Utf8,
                "open": pl.Float64,
                "high": pl.Float64,
                "low": pl.Float64,
                "close": pl.Float64,
                "volume": pl.Float64,
            }
        )
    out = pl.concat(frames, how="vertical_relaxed")
    return out.sort(["ticker", "date"])


def save_prices_parquet(df: pl.DataFrame, path: Path | None = None) -> Path:
    s = load_settings()
    path = path or (s.data_raw / "market" / "daily_prices.parquet")
    path.parent.mkdir(parents=True, exist_ok=True)
    df.write_parquet(path)
    return path


def load_prices(path: Path | None = None) -> pl.DataFrame:
    s = load_settings()
    path = path or (s.data_raw / "market" / "daily_prices.parquet")
    if not path.exists():
        return pl.DataFrame(
            schema={
                "date": pl.Date,
                "ticker": pl.Utf8,
                "open": pl.Float64,
                "high": pl.Float64,
                "low": pl.Float64,
                "close": pl.Float64,
                "volume": pl.Float64,
            }
        )
    return pl.read_parquet(path)


def with_returns(df: pl.DataFrame) -> pl.DataFrame:
    return df.sort(["ticker", "date"]).with_columns(
        (pl.col("close") / pl.col("close").shift(1).over("ticker") - 1.0).alias("ret")
    )
