import asyncio
import json
import random
from datetime import UTC, datetime

import pytest

from helios_alpha.crypto.orderbook import Level2Feed, PriceSide, price_micros

NOW = 1_800_000_000_000


def snapshot(symbol="BTC-USD"):
    return {
        "type": "snapshot",
        "product_id": symbol,
        "bids": [["100", "2"], ["99", "3"]],
        "asks": [["101", "2"], ["103", "3"]],
    }


def update(changes=None, at=NOW, symbol="BTC-USD"):
    return {
        "type": "l2update",
        "product_id": symbol,
        "time": datetime.fromtimestamp(at / 1000, UTC).isoformat(),
        "changes": [["buy", "100", "1"]] if changes is None else changes,
    }


def test_snapshot_needs_a_fresh_update_and_sizes_are_absolute():
    feed = Level2Feed(["BTC-USD"])
    assert feed.apply(snapshot(), NOW) is None
    first = feed.apply(update(), NOW)
    assert (first["bid_micros"], first["ask_micros"]) == (100_000_000, 101_000_000)
    assert first["sequence_scope"] == "websocket_session"
    assert first["channel"] == "level2_batch"
    second = feed.apply(update([["buy", "100", "0"], ["sell", "101", "0"]]), NOW)
    assert (second["bid_micros"], second["ask_micros"]) == (99_000_000, 103_000_000)
    assert second["sequence"] == first["sequence"] + 1


def test_batch_is_validated_after_all_changes():
    feed = Level2Feed(["BTC-USD"])
    feed.apply(snapshot(), NOW)
    quote = feed.apply(update([["buy", "102", "1"], ["sell", "101", "0"]]), NOW)
    assert (quote["bid_micros"], quote["ask_micros"]) == (102_000_000, 103_000_000)


@pytest.mark.parametrize(
    "changes",
    [
        [],
        [["buy", "100", "-1"]],
        [["buy", "NaN", "1"]],
        [["sell", "101", "Infinity"]],
        [["invalid", "100", "1"]],
        [["buy", "100"]],
        [["buy", 100, "1"]],
        [["buy", "100.0000001", "1"]],
        [["buy", "104", "1"]],
        [["buy", "100", "0"], ["buy", "99", "0"]],
    ],
)
def test_invalid_batch_invalidates_all_books_until_new_snapshots(changes):
    feed = Level2Feed(["BTC-USD", "ETH-USD"])
    feed.apply(snapshot(), NOW)
    feed.apply(snapshot("ETH-USD"), NOW)
    with pytest.raises(ValueError, match="fresh snapshots"):
        feed.apply(update(changes), NOW)
    assert not feed.books
    with pytest.raises(ValueError):
        feed.apply(update(), NOW)
    feed.apply(snapshot(), NOW)
    assert feed.apply(update(), NOW)["bid_micros"] == 100_000_000


@pytest.mark.parametrize("stamp", [NOW - 5001, NOW + 251])
def test_stale_and_future_updates_force_new_snapshots(stamp):
    feed = Level2Feed(["BTC-USD"])
    feed.apply(snapshot(), NOW)
    with pytest.raises(ValueError):
        feed.apply(update(at=stamp), NOW)
    assert not feed.books


def test_clock_regression_is_not_silently_skipped():
    feed = Level2Feed(["BTC-USD"])
    feed.apply(snapshot(), NOW)
    feed.apply(update(at=NOW + 2), NOW + 2)
    with pytest.raises(ValueError):
        feed.apply(update(at=NOW + 1), NOW + 3)
    assert not feed.books


def test_small_clock_skew_preserves_local_availability_and_heartbeat_cannot_refresh_it():
    feed = Level2Feed(["BTC-USD"])
    feed.apply(snapshot(), NOW)
    quote = feed.apply(update(at=NOW + 14), NOW)
    assert quote["observed_ms"] == NOW
    assert quote["exchange_ms"] == NOW + 14
    assert quote["received_ms"] == NOW
    assert feed.apply({"type": "heartbeat", "product_id": "BTC-USD"}, NOW + 1000) is None
    assert feed.sequence == 1


def test_snapshot_shape_duplicates_and_capacity_fail_closed(monkeypatch):
    from helios_alpha.crypto import orderbook

    monkeypatch.setattr(orderbook, "MAX_LEVELS_PER_SIDE", 2)
    feed = Level2Feed(["BTC-USD"])
    with pytest.raises(ValueError):
        feed.apply({**snapshot(), "bids": [["100", "1"], ["100.00", "2"]]}, NOW)
    feed.apply(snapshot(), NOW)
    with pytest.raises(ValueError):
        feed.apply(update([["buy", "98", "1"]]), NOW)
    assert not feed.books
    feed.apply(snapshot(), NOW)
    monkeypatch.setattr(orderbook, "MAX_CHANGES", 1)
    with pytest.raises(ValueError):
        feed.apply(update([["buy", "99", "1"], ["buy", "100", "1"]]), NOW)
    assert not feed.books


def test_far_book_levels_have_bounded_fixed_point_precision():
    assert price_micros("10000000.00000000") == 10_000_000_000_000
    for value in ("0", "99999999999999", "1e20", "0.000000001", True, "-1"):
        with pytest.raises(ValueError):
            price_micros(value)


def test_random_absolute_updates_match_a_simple_book_model():
    rng = random.Random(20261020)
    feed = Level2Feed(["BTC-USD"])
    feed.apply(snapshot(), NOW)
    bids, asks = {99, 100}, {101, 103}
    # Independent set/min/max oracle exercises heap insertion, deletion, and reinsertion.
    for i in range(2000):
        side = rng.choice(["buy", "sell"])
        levels = bids if side == "buy" else asks
        price = rng.randrange(50, 101) if side == "buy" else rng.randrange(101, 152)
        present = rng.choice([True, False])
        if not present and levels == {price}:
            present = True
        (levels.add if present else levels.discard)(price)
        quote = feed.apply(
            update([[side, str(price), "7.0" if present else "0"]], NOW + i), NOW + i
        )
        assert quote["bid_micros"] == max(bids) * 1_000_000
        assert quote["ask_micros"] == min(asks) * 1_000_000


def test_far_level_churn_cannot_grow_the_heap_without_bound():
    side = PriceSide([["100", "1"]], bids=True)
    for _ in range(10_000):
        side.change(90_000_000, True)
        side.change(90_000_000, False)
        assert side.best() == 100_000_000
        assert len(side.heap) <= 2 * len(side.prices) + 1024


def test_reconnect_discards_history_and_requires_a_new_snapshot(monkeypatch):
    pytest.importorskip("websockets")
    from helios_alpha.crypto import streaming

    monkeypatch.setattr(streaming.time, "time_ns", lambda: NOW * 1_000_000)

    async def check():
        book = streaming.QuoteBook()
        stopped, changed = asyncio.Event(), asyncio.Event()
        generation = 0
        sent = []
        recovered = []
        messages = [
            [snapshot(), update(), update([["buy", "100", "-1"]])],
            [
                {**snapshot(), "bids": [["102", "1"]], "asks": [["103", "1"]]},
                update([["buy", "102", "2"]]),
            ],
        ]

        class Stream:
            def __init__(self, number):
                self.number = number
                self.items = iter(messages[number])

            async def __aenter__(self):
                assert not book.latest and not book.history
                return self

            async def __aexit__(self, *_):
                if self.number == 1:
                    recovered.append((book.latest.copy(), dict(book.history)))

            async def send(self, wire):
                sent.append(json.loads(wire))

            async def recv(self):
                item = next(self.items)
                if self.number == 1 and item["type"] == "l2update":
                    stopped.set()
                await asyncio.sleep(0)
                return json.dumps(item)

        def connect(_url, **kwargs):
            nonlocal generation
            assert kwargs["proxy"] is None
            assert kwargs["max_queue"] == 16
            stream = Stream(generation)
            generation += 1
            return stream

        monkeypatch.setattr(streaming, "connect", connect)
        await asyncio.wait_for(streaming.watch_quotes(book, ["BTC-USD"], changed, stopped), 5)
        assert generation == 2
        assert book.protocol_resets == 1 and book.snapshots == 2
        assert all(item["channels"] == ["level2_batch", "heartbeat"] for item in sent)
        assert recovered[0][0]["BTC-USD"]["bid_micros"] == 102_000_000
        assert len(recovered[0][1]["BTC-USD"]) == 1
        assert not book.latest and not book.history

    asyncio.run(check())


def test_cancelled_feed_cannot_leave_quotes_available(monkeypatch):
    pytest.importorskip("websockets")
    from helios_alpha.crypto import streaming

    monkeypatch.setattr(streaming.time, "time_ns", lambda: NOW * 1_000_000)

    async def check():
        book = streaming.QuoteBook()
        stopped, changed = asyncio.Event(), asyncio.Event()
        messages = iter([snapshot(), update()])

        class Stream:
            async def __aenter__(self):
                return self

            async def __aexit__(self, *_):
                pass

            async def send(self, _):
                pass

            async def recv(self):
                item = next(messages, None)
                if item is None:
                    await asyncio.Event().wait()
                return json.dumps(item)

        monkeypatch.setattr(streaming, "connect", lambda *_, **__: Stream())
        task = asyncio.create_task(streaming.watch_quotes(book, ["BTC-USD"], changed, stopped))
        try:
            await asyncio.wait_for(changed.wait(), 1)
            assert book.quotes(["BTC-USD"], NOW, 1000)
        finally:
            task.cancel()
            with pytest.raises(asyncio.CancelledError):
                await task
        assert not book.connected and not book.latest and not book.history
        assert book.available_quotes(["BTC-USD"], NOW, 1000) == []

    asyncio.run(check())
