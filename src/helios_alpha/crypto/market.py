"""Public Coinbase reference data, kept separate from broker execution evidence."""

from __future__ import annotations

import json
import time
from datetime import UTC, datetime
from decimal import Decimal

import httpx

ORIGIN = "https://api.exchange.coinbase.com"
SYMBOLS = {"BTC-USD", "ETH-USD"}
HOUR = 3600


def micros(value) -> int:
    number = Decimal(str(value)) * 1_000_000
    if not number.is_finite() or number <= 0 or number != number.to_integral_value():
        raise ValueError("invalid reference price")
    result = int(number)
    if result > 1_000_000_000_000:
        raise ValueError("reference price exceeds bound")
    return result


async def get_json(client: httpx.AsyncClient, path: str, params=None):
    async with client.stream("GET", ORIGIN + path, params=params) as response:
        response.raise_for_status()
        body = bytearray()
        async for chunk in response.aiter_bytes():
            body.extend(chunk)
            if len(body) > 256 * 1024:
                raise ValueError("market response exceeds bound")
    return json.loads(body)


def parse_candles(rows: list, start: int, end: int) -> list[dict]:
    if not isinstance(rows, list) or len(rows) > 301:
        raise ValueError("invalid candle response")
    bars = {}
    for row in rows:
        if not isinstance(row, list) or len(row) != 6 or type(row[0]) is not int:
            raise ValueError("invalid candle row")
        at = row[0]
        if not start <= at < end:
            continue
        bar = dict(
            start_ms=at * 1000,
            duration_ms=HOUR * 1000,
            low_micros=micros(row[1]),
            high_micros=micros(row[2]),
            open_micros=micros(row[3]),
            close_micros=micros(row[4]),
        )
        if at % HOUR or not (bar["low_micros"] <= bar["open_micros"] <= bar["high_micros"]):
            raise ValueError("invalid candle range")
        if not bar["low_micros"] <= bar["close_micros"] <= bar["high_micros"]:
            raise ValueError("invalid candle close")
        if at in bars and bars[at] != bar:
            raise ValueError("conflicting candle revision")
        bars[at] = bar
    if set(bars) != set(range(start, end, HOUR)):
        raise ValueError("historical candle gap; no synthetic filling")
    return [bars[at] for at in sorted(bars)]


async def candles(client: httpx.AsyncClient, symbol: str, start: int, end: int):
    if symbol not in SYMBOLS or start % HOUR or end % HOUR or not 0 < end - start <= 300 * HOUR:
        raise ValueError("invalid candle request")
    params = {
        "granularity": HOUR,
        "start": datetime.fromtimestamp(start, UTC).isoformat(),
        "end": datetime.fromtimestamp(end, UTC).isoformat(),
    }
    rows = await get_json(client, f"/products/{symbol}/candles", params)
    return parse_candles(rows, start, end)


async def quote(client: httpx.AsyncClient, symbol: str):
    if symbol not in SYMBOLS:
        raise ValueError("unsupported symbol")
    value = await get_json(client, f"/products/{symbol}/ticker")
    stamp = datetime.fromisoformat(value["time"].replace("Z", "+00:00"))
    if stamp.utcoffset() is None:
        raise ValueError("market timestamp has no offset")
    observed_ms = int(stamp.timestamp() * 1000)
    if not 0 <= time.time_ns() // 1_000_000 - observed_ms <= 10_000:
        raise ValueError("reference quote is stale or from the future")
    result = dict(
        symbol=symbol,
        bid_micros=micros(value["bid"]),
        ask_micros=micros(value["ask"]),
        observed_ms=observed_ms,
    )
    if result["ask_micros"] < result["bid_micros"]:
        raise ValueError("crossed reference quote")
    return result
