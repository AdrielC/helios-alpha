"""Bounded Coinbase level2_batch reconstruction, valid only within one WebSocket session."""

from __future__ import annotations

import heapq
import re
from datetime import datetime

from .market import SYMBOLS

MAX_LEVELS_PER_SIDE = 100_000
MAX_CHANGES = 20_000
MAX_FRAME_BYTES = 8 * 1024 * 1024
MAX_EVENT_AGE_MS = 5000
PRICE = re.compile(r"([0-9]{1,14})(?:\.([0-9]{1,18}))?\Z")
SIZE = re.compile(r"[0-9]{1,24}(?:\.[0-9]{1,18})?\Z")


def price_micros(value):
    if not isinstance(value, str) or (match := PRICE.fullmatch(value)) is None:
        raise ValueError("invalid order-book price")
    fraction = (match[2] or "").rstrip("0")
    if len(fraction) > 6:
        raise ValueError("order-book price exceeds fixed-point precision")
    price = int(match[1]) * 1_000_000 + int(fraction.ljust(6, "0"))
    if not 0 < price <= 2**64 - 1:
        raise ValueError("order-book price exceeds bound")
    return price


def has_size(value):
    if not isinstance(value, str) or SIZE.fullmatch(value) is None:
        raise ValueError("invalid order-book size")
    return any(character not in "0." for character in value)


def event_time(value, received_ms):
    stamp = datetime.fromisoformat(value.replace("Z", "+00:00"))
    if stamp.utcoffset() is None:
        raise ValueError("order-book timestamp lacks timezone")
    exchange_ms = int(stamp.timestamp() * 1000)
    if not -250 <= received_ms - exchange_ms <= MAX_EVENT_AGE_MS:
        raise ValueError("order-book event is stale or from the future")
    return exchange_ms


class PriceSide:
    """Presence is sufficient for BBO. Sizes are absolute, never additive deltas."""

    def __init__(self, rows, *, bids):
        if not isinstance(rows, list) or not 0 < len(rows) <= MAX_LEVELS_PER_SIDE:
            raise ValueError("order-book side exceeds bound")
        self.sign = -1 if bids else 1
        self.prices = set()
        for row in rows:
            if not isinstance(row, list) or len(row) != 2 or not has_size(row[1]):
                raise ValueError("invalid order-book snapshot level")
            price = price_micros(row[0])
            if price in self.prices:
                raise ValueError("duplicate order-book snapshot level")
            self.prices.add(price)
        self.heap = [price * self.sign for price in self.prices]
        heapq.heapify(self.heap)

    def change(self, price, present):
        if present:
            if price not in self.prices:
                self.prices.add(price)
                heapq.heappush(self.heap, price * self.sign)
        else:
            self.prices.discard(price)

    def best(self):
        if not 0 < len(self.prices) <= MAX_LEVELS_PER_SIDE:
            raise ValueError("order-book side empty or beyond capacity")
        # Remove stale heap entries without scanning the whole book on each update.
        while self.heap and self.heap[0] * self.sign not in self.prices:
            heapq.heappop(self.heap)
        if len(self.heap) > 2 * len(self.prices) + 1024:
            self.heap = [price * self.sign for price in self.prices]
            heapq.heapify(self.heap)
        return self.heap[0] * self.sign


class Level2Feed:
    def __init__(self, symbols):
        if not symbols or len(set(symbols)) != len(symbols) or not set(symbols) <= SYMBOLS:
            raise ValueError("invalid order-book symbols")
        self.symbols = set(symbols)
        self.reset()

    def reset(self):
        self.books = {}
        self.last_exchange_ms = {}
        self.sequence = 0

    def apply(self, message, received_ms):
        """Any invalid book message invalidates every book until fresh snapshots arrive."""
        try:
            return self._apply(message, received_ms)
        except (ValueError, TypeError, KeyError, AttributeError, IndexError, OverflowError):
            self.reset()
            raise ValueError("invalid order-book stream; fresh snapshots required") from None

    def _apply(self, message, received_ms):
        if type(received_ms) is not int or received_ms < 0:
            raise ValueError("invalid receipt timestamp")
        kind = message.get("type")
        if kind not in ("snapshot", "l2update"):
            return None
        symbol = message["product_id"]
        if symbol not in self.symbols:
            raise ValueError("unexpected order-book symbol")
        if kind == "snapshot":
            if symbol in self.books:
                raise ValueError("unexpected replacement snapshot")
            bids = PriceSide(message["bids"], bids=True)
            asks = PriceSide(message["asks"], bids=False)
            if bids.best() > asks.best():
                raise ValueError("crossed order-book snapshot")
            self.books[symbol] = (bids, asks)
            # A snapshot need not carry an exchange clock. Publish only after a fresh update.
            return None
        if symbol not in self.books:
            raise ValueError("update arrived before snapshot")
        exchange_ms = event_time(message["time"], received_ms)
        if exchange_ms < self.last_exchange_ms.get(symbol, exchange_ms):
            raise ValueError("order-book clock regressed")
        changes = message["changes"]
        if not isinstance(changes, list) or not 0 < len(changes) <= MAX_CHANGES:
            raise ValueError("invalid order-book batch size")
        parsed = []
        for change in changes:
            if not isinstance(change, list) or len(change) != 3 or change[0] not in ("buy", "sell"):
                raise ValueError("invalid order-book change")
            parsed.append((change[0], price_micros(change[1]), has_size(change[2])))
        bids, asks = self.books[symbol]
        for side, price, present in parsed:
            (bids if side == "buy" else asks).change(price, present)
        bid, ask = bids.best(), asks.best()
        if bid > ask:
            raise ValueError("crossed order-book update")
        self.last_exchange_ms[symbol] = exchange_ms
        self.sequence += 1
        return {
            "symbol": symbol,
            "bid_micros": bid,
            "ask_micros": ask,
            "observed_ms": min(exchange_ms, received_ms),
            "exchange_ms": exchange_ms,
            "received_ms": received_ms,
            "sequence": self.sequence,
            "sequence_scope": "websocket_session",
            "channel": "level2_batch",
        }
