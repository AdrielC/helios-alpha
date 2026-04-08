"""
Polygon.io aggregates (OHLCV) for US equities and ETFs.

Docs: https://polygon.io/docs/stocks/get_v2_aggs_ticker__stocksticker__range__multiplier___timespan___from___to

Set ``HELIOS_POLYGON_API_KEY`` (see ``helios_alpha.config.Settings``).
"""

from __future__ import annotations

from datetime import date

import polars as pl

from helios_alpha.config import load_settings
from helios_alpha.utils.http import get_json
from helios_alpha.utils.time import from_unix_epoch_ms


def fetch_daily_aggregates(
    ticker: str,
    start: date,
    end: date,
    *,
    api_key: str | None = None,
    base_url: str = "https://api.polygon.io",
    adjusted: bool = True,
) -> pl.DataFrame:
    """
    One row per session bar from Polygon v2 aggs range endpoint.

    ``ticker`` should be Polygon format (e.g. ``SPY``, ``I:SPX`` for indices if licensed).
    """
    key = api_key if api_key is not None else load_settings().polygon_api_key
    if not key:
        msg = "Polygon API key missing: set HELIOS_POLYGON_API_KEY or pass api_key="
        raise ValueError(msg)
    sym = ticker.lstrip("^").upper()
    mult = 1
    span = "day"
    url = (
        f"{base_url.rstrip('/')}/v2/aggs/ticker/{sym}/range/"
        f"{mult}/{span}/{start.isoformat()}/{end.isoformat()}"
    )
    data = get_json(url, params={"adjusted": str(adjusted).lower(), "sort": "asc", "apiKey": key})
    results = data.get("results") or []
    if not results:
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
    rows = []
    for r in results:
        t_ms = r.get("t")
        if t_ms is None:
            continue
        d = from_unix_epoch_ms(float(t_ms)).date()
        rows.append(
            {
                "date": d,
                "ticker": ticker,
                "open": float(r["o"]),
                "high": float(r["h"]),
                "low": float(r["l"]),
                "close": float(r["c"]),
                "volume": float(r.get("v") or 0),
            }
        )
    df = pl.DataFrame(rows)
    return df.sort(["ticker", "date"])


def download_daily_prices_polygon(
    tickers: list[str],
    start: date,
    end: date,
    *,
    api_key: str | None = None,
    base_url: str = "https://api.polygon.io",
) -> pl.DataFrame:
    frames: list[pl.DataFrame] = []
    for t in tickers:
        sym = t.strip()
        if not sym:
            continue
        frames.append(
            fetch_daily_aggregates(sym, start, end, api_key=api_key, base_url=base_url)
        )
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
    return pl.concat(frames, how="vertical_relaxed").sort(["ticker", "date"])
