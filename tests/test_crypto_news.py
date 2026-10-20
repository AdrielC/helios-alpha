import asyncio
from datetime import UTC, datetime
from types import SimpleNamespace
from unittest.mock import AsyncMock

import pytest

pytest.importorskip("nats")
pytest.importorskip("websockets")
pytest.importorskip("defusedxml")

from defusedxml.common import DTDForbidden
from nats.errors import TimeoutError as NatsTimeout

from helios_alpha.control.store import canonical
from helios_alpha.crypto.decisions import review_event
from helios_alpha.crypto.fast_runtime import NEWS_STREAM, fresh_news, publish_news, retry_delay
from helios_alpha.crypto.news import FEEDS, Feed, NewsJournal, parse_feed, polling_intervals
from helios_alpha.crypto.paper_transport import PaperTransport, event_envelope
from helios_alpha.crypto.runtime import parse_event
from helios_alpha.crypto.streaming import QuoteBook, parse_ticker

NOW = 1_800_000_000_000
FEED = Feed("test", "https://example.com/feed", "example.com", "test")


def rss(stamp=NOW, path="/new", title="Synthetic test headline"):
    published = datetime.fromtimestamp(stamp / 1000, UTC).isoformat()
    return (
        f"<rss><channel><item><title>{title}</title>"
        f"<link>https://example.com{path}</link>"
        f"<pubDate>{published}</pubDate></item></channel></rss>"
    ).encode()


def make_journal(tmp_path):
    root = tmp_path / "private"
    root.mkdir(mode=0o700)
    return root, NewsJournal(root)


def test_startup_backlog_edits_and_restart_never_create_fresh_news(tmp_path):
    root, journal = make_journal(tmp_path)
    first = parse_feed(FEED, rss())
    assert journal.ingest(FEED, first, NOW, {})["baseline"]
    assert not journal.pending()
    journal.close()
    journal = NewsJournal(root)
    changed = parse_feed(FEED, rss(NOW + 1000, title="Updated headline"))
    assert changed[0]["news_id"] == first[0]["news_id"]
    assert journal.ingest(FEED, changed, NOW + 1000, {})["new_items"] == 0
    assert not journal.pending()
    new = parse_feed(FEED, rss(NOW + 1000, path="/next"))
    assert journal.ingest(FEED, new, NOW + 1001, {"etag": '"v2"'})["fresh_items"] == 1
    frozen = journal.pending()[0]
    assert frozen["observed_ms"] == NOW + 1001
    journal.close()
    journal = NewsJournal(root)
    assert journal.pending() == [frozen]
    assert journal.headers(FEED) == {"If-None-Match": '"v2"'}
    journal.delivered(frozen["news_id"])
    assert not journal.pending()
    journal.close()


def test_restart_recovers_only_acknowledged_fresh_news_with_original_clocks(tmp_path):
    root, journal = make_journal(tmp_path)
    journal.ingest(FEED, parse_feed(FEED, rss(path="/baseline")), NOW, {})
    journal.ingest(FEED, parse_feed(FEED, rss()), NOW, {})
    original = journal.pending()[0]
    assert not journal.recent_published(NOW, {FEED.id})
    journal.delivered(original["news_id"])
    journal.close()
    journal = NewsJournal(root)
    assert journal.recent_published(NOW + 1000, {FEED.id}) == [original]
    assert not journal.pending()
    assert not journal.recent_published(NOW + 1000, {"disabled-source"})
    assert not journal.recent_published(NOW - 1, {FEED.id})
    assert not journal.recent_published(NOW + 120_001, {FEED.id})
    journal.close()


def test_restart_does_not_extend_a_recently_received_old_headline(tmp_path):
    _, journal = make_journal(tmp_path)
    journal.ingest(FEED, parse_feed(FEED, rss(path="/baseline")), NOW, {})
    journal.ingest(FEED, parse_feed(FEED, rss(NOW - 119_999)), NOW, {})
    original = journal.pending()[0]
    journal.delivered(original["news_id"])
    assert journal.recent_published(NOW, {FEED.id}) == [original]
    assert not journal.recent_published(NOW + 2, {FEED.id})
    journal.close()


def test_recovered_news_is_bounded_and_ordered_by_original_receipt(tmp_path, monkeypatch):
    from helios_alpha.crypto import news

    monkeypatch.setattr(news, "MAX_ACTIVE_NEWS", 2)
    _, journal = make_journal(tmp_path)
    journal.ingest(FEED, parse_feed(FEED, rss(path="/baseline")), NOW, {})
    originals = []
    for i in range(3):
        journal.ingest(FEED, parse_feed(FEED, rss(NOW + i, path=f"/story-{i}")), NOW + i, {})
        item = journal.pending()[0]
        originals.append(item)
        journal.delivered(item["news_id"])
    assert journal.recent_published(NOW + 3, {FEED.id}) == originals[-2:]
    journal.close()


@pytest.mark.parametrize(
    "changes",
    [{"observed_ms": True}, {"observed_ms": str(NOW)}, {"published_ms": None}, {"eligible": 1}],
)
def test_malformed_news_clocks_cannot_be_recovered(changes):
    item = {"eligible": True, "observed_ms": NOW, "published_ms": NOW}
    assert not fresh_news([{**item, **changes}], NOW)


@pytest.mark.parametrize("offset", [-1, 120001, 86400000, 10**12])
def test_future_or_old_headline_cannot_enter(tmp_path, offset):
    _, journal = make_journal(tmp_path)
    journal.ingest(FEED, parse_feed(FEED, rss(path="/baseline")), NOW, {})
    items = parse_feed(FEED, rss(NOW - offset))
    assert journal.ingest(FEED, items, NOW, {})["fresh_items"] == 0
    assert not journal.pending()
    journal.close()


def test_undated_and_edited_atom_items_remain_ineligible():
    atom = b"""<feed xmlns="http://www.w3.org/2005/Atom"><entry>
      <title>Edited old story</title><link href="https://example.com/story"/>
      <updated>2027-01-15T08:00:00Z</updated></entry></feed>"""
    item = parse_feed(FEED, atom)[0]
    assert item["published_ms"] is None
    assert not fresh_news([{**item, "eligible": True, "observed_ms": NOW}], NOW)


@pytest.mark.parametrize(
    "link",
    [
        "http://example.com/a",
        "https://evil.example/a",
        "https://example.com:9999/a",
        "https://user:pass@example.com/a",
        "https://example.com:bad/a",
        "https://127.0.0.1/a",
    ],
)
def test_news_does_not_admit_unexpected_origins(link):
    raw = rss().replace(b"https://example.com/new", link.encode())
    assert parse_feed(FEED, raw) == []


def test_xml_external_entities_are_rejected():
    with pytest.raises(DTDForbidden):
        parse_feed(FEED, b'<!DOCTYPE rss [<!ENTITY x SYSTEM "file:///etc/passwd">]><rss/>')


def test_entry_outbox_is_immutable_and_survives_unknown_publish(tmp_path):
    root, journal = make_journal(tmp_path)
    first = {"event_id": "original", "target_micros": 20}
    assert not journal.entry_attempted("story-symbol")
    assert journal.claim_entry("story-symbol", first)
    assert journal.entry_attempted("story-symbol")
    assert not journal.claim_entry("story-symbol", {"event_id": "new", "target_micros": 900})
    journal.close()
    journal = NewsJournal(root)
    assert journal.entry_attempted("story-symbol")
    assert journal.pending_entries() == [("story-symbol", first)]
    journal.entry_completed("story-symbol")
    assert journal.entry_attempted("story-symbol")
    assert not journal.pending_entries()
    assert not journal.claim_entry("story-symbol", first)
    journal.close()


def test_ambiguous_news_publication_reuses_exact_envelope(tmp_path):
    _, journal = make_journal(tmp_path)
    journal.ingest(FEED, parse_feed(FEED, rss(path="/baseline")), NOW, {})
    journal.ingest(FEED, parse_feed(FEED, rss()), NOW, {})

    async def check():
        stopped = asyncio.Event()
        calls = []
        metrics = {"news_published": 0}

        class JS:
            async def publish(self, subject, body, headers):
                calls.append((subject, body, headers))
                if len(calls) == 1:
                    # Broker may have accepted the write; the ACK was lost.
                    raise TimeoutError()
                stopped.set()
                return SimpleNamespace(stream=NEWS_STREAM)

        await publish_news(journal, JS(), [], stopped, metrics)
        assert len(calls) == 2 and calls[0] == calls[1]
        assert not journal.pending()
        assert metrics["news_published"] == 1

    asyncio.run(check())
    journal.close()


def ticker(at=NOW, seq=1, bid="100.00", ask="100.01"):
    return {
        "type": "ticker",
        "product_id": "BTC-USD",
        "time": datetime.fromtimestamp(at / 1000, UTC).isoformat(),
        "sequence": seq,
        "best_bid": bid,
        "best_ask": ask,
    }


@pytest.mark.parametrize(
    "change",
    [
        {"best_bid": "NaN"},
        {"best_ask": "0"},
        {"best_bid": "101"},
        {"sequence": True},
        {"time": "2027-01-15T08:00:00"},
        {"time": "invalid"},
    ],
)
def test_malformed_quotes_are_rejected(change):
    with pytest.raises((ValueError, KeyError)):
        parse_ticker({**ticker(), **change}, NOW)


@pytest.mark.parametrize("offset", [-251, 10001])
def test_stale_or_future_quotes_are_rejected(offset):
    with pytest.raises(ValueError):
        parse_ticker(ticker(at=NOW - offset), NOW)


def test_small_measured_venue_clock_skew_preserves_causal_local_availability():
    quote = parse_ticker(ticker(at=NOW + 5), NOW)
    assert quote["observed_ms"] == NOW
    assert quote["exchange_ms"] == NOW + 5


def test_quote_history_is_bounded_and_resets_after_disconnect():
    book = QuoteBook()
    book.connected = True
    for i in range(1000):
        assert book.accept(parse_ticker(ticker(NOW + i * 1000, i), NOW + i * 1000))
    assert len(book.history["BTC-USD"]) == 360
    assert not book.accept(parse_ticker(ticker(NOW + 999000, 998), NOW + 999000))
    assert not book.accept(parse_ticker(ticker(NOW + 998000, 1001), NOW + 999000))
    assert len(book.quotes(["BTC-USD"], NOW + 999000, 1000)) == 1
    with pytest.raises(ValueError):
        book.quotes(["BTC-USD"], NOW + 1001000, 1000)
    book.reset()
    assert not book.history and not book.latest
    with pytest.raises(ValueError):
        book.quotes(["BTC-USD"], NOW + 999000, 1000)


def test_fresh_assets_remain_available_when_another_quote_is_stale():
    book = QuoteBook()
    book.connected = True
    book.accept(parse_ticker(ticker(), NOW))
    book.accept(parse_ticker({**ticker(NOW - 6000), "product_id": "ETH-USD"}, NOW - 6000))
    assert [q["symbol"] for q in book.available_quotes(["BTC-USD", "ETH-USD"], NOW, 5000)] == [
        "BTC-USD"
    ]
    with pytest.raises(ValueError):
        book.quotes(["BTC-USD", "ETH-USD"], NOW, 5000)
    book.reset()
    assert not book.available_quotes(["BTC-USD", "ETH-USD"], NOW, 5000)


def test_retry_after_is_respected_and_bounded():
    assert retry_delay({"retry-after": "120"}, NOW, 30) == 120
    assert retry_delay({"retry-after": "99999"}, NOW, 30) == 3600
    assert retry_delay({"retry-after": "garbage"}, NOW, 30) == 30
    assert retry_delay({"retry-after": "1"}, NOW, 30) == 30


def test_per_source_polling_preserves_the_default_for_other_feeds():
    selected = [feed for feed in FEEDS if feed.id in {"decrypt", "coindesk", "sec-press"}]
    assert polling_intervals(
        {"poll_seconds": 30, "poll_seconds_by_source": {"decrypt": 15, "coindesk": 20}},
        selected,
    ) == {"decrypt": 15, "coindesk": 20, "sec-press": 30}
    assert polling_intervals({"poll_seconds": 300}, [FEED]) == {"test": 300}


@pytest.mark.parametrize(
    "config",
    [
        {},
        {"poll_seconds": True},
        {"poll_seconds": 14},
        {"poll_seconds": 301},
        {"poll_seconds": 30, "poll_seconds_by_source": []},
        {"poll_seconds": 30, "poll_seconds_by_source": {"disabled": 15}},
        {"poll_seconds": 30, "poll_seconds_by_source": {"test": 0}},
        {"poll_seconds": 30, "poll_seconds_by_source": {"test": True}},
        {"poll_seconds": 30, "poll_seconds_by_source": {"test": 15.0}},
        {"poll_seconds": 30, "poll_seconds_by_source": {"test": 301}},
    ],
)
def test_invalid_source_polling_cannot_start_network_tasks(config):
    with pytest.raises(ValueError, match="polling intervals"):
        polling_intervals(config, [FEED])


def test_decrypt_baseline_is_not_a_new_signal_and_other_origins_remain_rejected(tmp_path):
    feed = next(f for f in FEEDS if f.id == "decrypt")
    _, journal = make_journal(tmp_path)
    baseline = parse_feed(
        feed, rss(path="/baseline").replace(b"https://example.com", b"https://decrypt.co")
    )
    assert baseline[0]["synthetic"] is False
    assert journal.ingest(feed, baseline, NOW, {})["baseline"]
    assert journal.pending() == []
    assert parse_feed(feed, rss()) == []
    fresh = parse_feed(feed, rss(NOW + 1000).replace(b"https://example.com", b"https://decrypt.co"))
    assert journal.ingest(feed, fresh, NOW + 2000, {})["fresh_items"] == 1
    assert journal.pending()[0]["published_ms"] == NOW + 1000
    assert journal.pending()[0]["observed_ms"] == NOW + 2000
    journal.close()


def test_polling_keeps_cache_age_separate_from_news_freshness_and_backs_off(tmp_path, monkeypatch):
    from helios_alpha.crypto import fast_runtime

    _, journal = make_journal(tmp_path)
    journal.ingest(FEED, parse_feed(FEED, rss(path="/baseline")), NOW, {})
    clock = [NOW + 1000]
    monkeypatch.setattr(fast_runtime, "now_ms", lambda: clock[0])
    responses = [
        (200, {"age": "600", "etag": '"fresh"'}, rss(NOW)),
        (304, {"age": "invalid"}, b""),
        (429, {"retry-after": "120", "age": "0"}, b""),
    ]
    requests = []
    snapshots = []

    async def fetch(_client, _feed, headers):
        requests.append(headers)
        return responses.pop(0)

    monkeypatch.setattr(fast_runtime, "fetch_feed", fetch)

    async def check():
        stopped = asyncio.Event()
        health = {}

        async def sleep(_stopped, delay):
            snapshots.append((dict(health[FEED.id]), delay))
            clock[0] += delay * 1000
            if not responses:
                stopped.set()

        monkeypatch.setattr(fast_runtime, "sleep_or_stop", sleep)
        await fast_runtime.poll_feed(object(), FEED, journal, health, stopped, 15)

    asyncio.run(check())
    assert [delay for _, delay in snapshots] == [15, 15, 120]
    assert requests == [{}, {"If-None-Match": '"fresh"'}, {"If-None-Match": '"fresh"'}]
    assert [s["response_cache_age_seconds"] for s, _ in snapshots] == [600, None, 0]
    assert [s["consecutive_failures"] for s, _ in snapshots] == [0, 0, 1]
    assert all(s["poll_seconds"] == 15 for s, _ in snapshots)
    assert snapshots[-1][0]["next_poll_ms"] == NOW + 151_000
    original = journal.pending()[0]
    assert original["observed_ms"] == NOW + 1000
    assert original["published_ms"] == NOW
    assert not fresh_news([original], NOW + 120_001)
    journal.close()


def envelope():
    return event_envelope(
        available_ms=NOW,
        policy_revision=1,
        quote={
            "symbol": "BTC-USD",
            "bid_micros": 100000000,
            "ask_micros": 100010000,
            "observed_ms": NOW,
        },
        portfolio_quotes=[],
        target_micros=10000000,
        strategy_fingerprint="synthetic",
        decision_provider="test",
        context={"news_id": "synthetic", "observation_only": False},
    )


def test_news_provenance_is_part_of_immutable_event_digest():
    event = envelope()
    assert parse_event(canonical(event).encode()) == event
    event["context"]["news_id"] = "substituted"
    with pytest.raises(ValueError, match="digest"):
        parse_event(canonical(event).encode())


def test_crash_after_paper_effect_before_ack_does_not_repeat_effect():
    async def check():
        state = {"paper": {"source_id": "test", "cursor": 0}}
        effects = []

        class Store:
            def status(self, actor):
                return state

            def policy(self, actor):
                return {"revision": 1, "limits": {"symbol_exposure_micros": 100000000}}

            def command(self, actor, method, payload):
                effects.append(payload)
                state["paper"]["cursor"] = 1
                return {"outcome": "simulated_fill", "cursor": 1}

        class Message:
            data = canonical(envelope()).encode()
            metadata = SimpleNamespace(sequence=SimpleNamespace(stream=1))
            ack_calls = 0
            nak_calls = 0

            async def in_progress(self):
                pass

            async def nak(self, delay):
                self.nak_calls += 1

            async def ack_sync(self, timeout):
                self.ack_calls += 1
                if self.ack_calls == 1:
                    raise TimeoutError("crash before acknowledgment")

        msg = Message()

        class Subscription:
            deliveries = 0

            async def fetch(self, *args, **kwargs):
                self.deliveries += 1
                if self.deliveries <= 2:
                    return [msg]
                raise NatsTimeout()

        transport = PaperTransport()
        transport.store = Store()
        transport.source_id = "test"
        transport.subscription = Subscription()
        with pytest.raises(TimeoutError):
            await transport.drain()
        assert await transport.drain() == []
        assert len(effects) == 1 and msg.ack_calls == 2
        assert msg.nak_calls == 1

    asyncio.run(check())


def test_unavailable_model_holds_new_risk_without_paid_request(tmp_path, monkeypatch):
    root = tmp_path / "private"
    root.mkdir(mode=0o700)
    store = SimpleNamespace(command=lambda *args: {"send_allowed": False})
    process = AsyncMock()
    monkeypatch.setattr(asyncio, "create_subprocess_exec", process)

    async def check():
        event = {"observed_ms": 1}
        assert (await review_event(root, store, {"enabled": False}, event, fallback_veto=True))[
            "veto_new_risk"
        ]
        assert (
            await review_event(
                root,
                store,
                {"enabled": True},
                event,
                fallback_veto=True,
                expires_ms=10**15,
            )
        )["veto_new_risk"]

    asyncio.run(check())
    process.assert_not_called()
