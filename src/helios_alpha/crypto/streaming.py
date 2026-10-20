"""Public order-book reference quotes with causal timestamps and bounded history."""

from __future__ import annotations

import asyncio
import json
import time
from collections import deque
from datetime import datetime

from websockets.asyncio.client import connect

from .market import SYMBOLS, micros
from .orderbook import MAX_FRAME_BYTES, Level2Feed

WEBSOCKET = "wss://ws-feed.exchange.coinbase.com"


def parse_ticker(value: dict, received_ms: int) -> dict | None:
    if value.get("type") != "ticker" or value.get("product_id") not in SYMBOLS:
        return None
    stamp = datetime.fromisoformat(value["time"].replace("Z", "+00:00"))
    if stamp.utcoffset() is None:
        raise ValueError("quote timestamp lacks timezone")
    exchange_ms = int(stamp.timestamp() * 1000)
    # Small venue/host clock skew is measured in the feed. Preserve it explicitly;
    # local availability cannot occur after receipt. Large skew still fails closed.
    if not -250 <= received_ms - exchange_ms <= 10_000:
        raise ValueError("reference quote is stale or from the future")
    observed_ms = min(exchange_ms, received_ms)
    sequence = value["sequence"]
    if type(sequence) is not int or sequence < 0:
        raise ValueError("invalid quote sequence")
    bid, ask = micros(value["best_bid"]), micros(value["best_ask"])
    if bid > ask:
        raise ValueError("crossed reference quote")
    return dict(
        symbol=value["product_id"],
        bid_micros=bid,
        ask_micros=ask,
        observed_ms=observed_ms,
        exchange_ms=exchange_ms,
        received_ms=received_ms,
        sequence=sequence,
    )


class QuoteBook:
    def __init__(self):
        self.latest: dict[str, dict] = {}
        self.history: dict[str, deque] = {}
        self.connected = False
        self.connections = 0
        self.ticks = 0
        self.rejected = 0
        self.channel = "unconfigured"
        self.snapshots = 0
        self.protocol_resets = 0
        self.last_feed_error = None

    def reset(self):
        self.connected = False
        self.latest.clear()
        self.history.clear()

    def accept(self, quote: dict) -> bool:
        symbol = quote["symbol"]
        previous = self.latest.get(symbol)
        if previous and (
            quote["sequence"] <= previous["sequence"]
            or quote["observed_ms"] < previous["observed_ms"]
            or quote["received_ms"] < previous["received_ms"]
        ):
            self.rejected += 1
            return False
        self.latest[symbol] = quote
        history = self.history.setdefault(symbol, deque(maxlen=360))
        sample = (quote["received_ms"], (quote["bid_micros"] + quote["ask_micros"]) // 2)
        if not history or sample[0] // 1000 > history[-1][0] // 1000:
            history.append(sample)
        else:
            history[-1] = sample
        self.ticks += 1
        return True

    def quotes(self, symbols, now_ms: int, max_age_ms: int) -> list[dict]:
        if not self.connected:
            raise ValueError("reference feed disconnected")
        result = self.available_quotes(symbols, now_ms, max_age_ms)
        if len(result) != len(symbols):
            raise ValueError("reference quote unavailable or stale")
        return result

    def available_quotes(self, symbols, now_ms: int, max_age_ms: int) -> list[dict]:
        """Fresh assets can progress independently; the risk kernel checks held assets."""
        if not self.connected:
            return []
        result = []
        for symbol in symbols:
            q = self.latest.get(symbol)
            if q is None or not 0 <= now_ms - q["observed_ms"] <= max_age_ms:
                continue
            result.append(
                {key: q[key] for key in ("symbol", "bid_micros", "ask_micros", "observed_ms")}
            )
        return result

    def context(self, symbol: str, now_ms: int) -> dict:
        history = [list(point) for point in self.history.get(symbol, ()) if point[0] <= now_ms]
        return {"samples": history, "connected": self.connected}


async def watch_quotes(book: QuoteBook, symbols: list[str], changed: asyncio.Event, stopped):
    attempt = 0
    book.channel = "level2_batch"
    while not stopped.is_set():
        try:
            book.reset()
            depth = Level2Feed(symbols)
            # No broker credentials, environment proxies, or API calls are involved.
            async with connect(
                WEBSOCKET,
                proxy=None,
                open_timeout=8,
                ping_interval=20,
                ping_timeout=10,
                max_size=MAX_FRAME_BYTES,
                max_queue=16,
                compression=None,
            ) as stream:
                await stream.send(
                    json.dumps(
                        {
                            "type": "subscribe",
                            "product_ids": symbols,
                            "channels": ["level2_batch", "heartbeat"],
                        }
                    )
                )
                book.reset()
                book.connected = True
                book.connections += 1
                attempt = 0
                while not stopped.is_set():
                    raw = await asyncio.wait_for(stream.recv(), timeout=5)
                    value = json.loads(raw)
                    if value.get("type") == "error":
                        raise ValueError("reference subscription rejected")
                    quote = depth.apply(value, time.time_ns() // 1_000_000)
                    if value.get("type") == "snapshot":
                        book.snapshots += 1
                    if quote:
                        if not book.accept(quote):
                            raise ValueError("order-book quote order regressed")
                        changed.set()
                        book.last_feed_error = None
        except asyncio.CancelledError:
            book.reset()
            changed.set()
            raise
        except Exception as error:
            book.reset()
            book.rejected += 1
            book.protocol_resets += 1
            book.last_feed_error = type(error).__name__
            changed.set()
            attempt += 1
            try:
                await asyncio.wait_for(stopped.wait(), timeout=min(30, 2 ** min(attempt, 5)))
            except TimeoutError:
                pass
    book.reset()
