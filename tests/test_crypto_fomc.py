import asyncio
import hashlib
from datetime import UTC, datetime

import httpx
import pytest

pytest.importorskip("nats")
pytest.importorskip("websockets")
pytest.importorskip("defusedxml")

from helios_alpha.crypto import fast_runtime
from helios_alpha.crypto.fomc import (
    MAX_STATEMENT_BYTES,
    TITLE,
    fetch_statement,
    needs_source_facts,
    parse_statement,
)
from helios_alpha.crypto.news import FEEDS, NewsJournal, parse_feed

NOW = int(datetime(2026, 9, 16, 18, tzinfo=UTC).timestamp() * 1000)
FEED = FEEDS[0]
URL = "https://www.federalreserve.gov/newsevents/pressreleases/monetary20260916a.htm"
LOWER = (
    "The Committee decided to lower the target range for the federal funds rate "
    "by 1/4 percentage point to 3-1/4 to 3-1/2 percent."
)
MAINTAIN = (
    "The Committee decided to maintain the target range for the federal funds rate "
    "at 3-1/2 to 3-3/4 percent."
)


def document(decision=LOWER, *, extra="", date="September 16, 2026"):
    return (
        f'<html><p class="article__time">{date}</p><h3 class="title">{TITLE}</h3>'
        f"<p>{decision}</p>{extra}</html>"
    ).encode()


def rss(url=URL, title=TITLE):
    return (
        f"<rss><channel><item><title>{title}</title><link>{url}</link>"
        "<pubDate>Wed, 16 Sep 2026 18:00:00 GMT</pubDate></item></channel></rss>"
    ).encode()


def item():
    return {
        **parse_feed(FEED, rss())[0], "eligible": True, "observed_ms": NOW + 2, "synthetic": True,
    }


@pytest.fixture
def journal(tmp_path):
    root = tmp_path / "private"
    root.mkdir(mode=0o700)
    store = NewsJournal(root)
    store.ingest(FEED, parse_feed(FEED, rss(URL.replace("0916", "0729"))), NOW, {})
    store.ingest(FEED, parse_feed(FEED, rss()), NOW + 2, {})
    yield root, store
    store.close()


@pytest.mark.parametrize(
    "decision,expected",
    [
        (LOWER, ("lower", 325, 350, -25)),
        (MAINTAIN, ("maintain", 350, 375, 0)),
        (
            "In support of its goals, the Committee decided to raise the target range "
            "for the federal funds rate by 1/2 percentage point to 4 to 4-1/4 percent.",
            ("raise", 400, 425, 50),
        ),
        (
            "The Committee decided to reduce the target range for the federal funds rate "
            "to 0 to 1/4 percent.",
            ("lower", 0, 25, None),
        ),
        (LOWER.replace("3-1/4", "3.25").replace("3-1/2", "3.50"), ("lower", 325, 350, -25)),
        (LOWER.replace("3-1/4", "3¼").replace("3-1/2", "3½"), ("lower", 325, 350, -25)),
        (LOWER.replace(" to ", "<strong> </strong>to "), ("lower", 325, 350, -25)),
    ],
)
def test_extracts_only_the_adopted_policy_action(decision, expected):
    raw = document(decision)
    facts = parse_statement(item(), raw, NOW + 1000)
    assert tuple(facts[key] for key in (
        "action", "target_lower_bps", "target_upper_bps", "stated_change_bps"
    )) == expected
    assert facts["fetched_ms"] == NOW + 1000
    assert facts["content_sha256"] == hashlib.sha256(raw).hexdigest()
    assert facts["market_surprise"] == "not_measured"


def test_dissenting_preference_does_not_override_the_adopted_action():
    raw = document(
        MAINTAIN,
        extra="<p>Voting against the action was a member who preferred to lower "
        "the target range for the federal funds rate by 1/4 percentage point.</p>"
        f"<script>{LOWER}</script>",
    )
    assert parse_statement(item(), raw, NOW + 1000)["action"] == "maintain"


@pytest.mark.parametrize(
    "raw",
    [
        document(extra=f"<p>{LOWER}</p>"),
        document(LOWER + " " + LOWER),
        document("Members discussed whether to lower rates."),
        document(LOWER.replace("decided", "might decide")),
        document(LOWER.replace("3-1/4 to 3-1/2", "4 to 3")),
        document(LOWER.replace("1/4 percentage", "0 percentage")),
        document(LOWER.replace("1/4 percentage", "1/0 percentage")),
        document(LOWER.replace("1/4 percentage", "1/3 percentage")),
        document(LOWER.replace("lower", "maintain")),
        document(date="July 29, 2026"),
        document() + b"x" * MAX_STATEMENT_BYTES,
    ],
)
def test_ambiguous_invalid_and_mismatched_pages_are_rejected(raw):
    with pytest.raises(ValueError):
        parse_statement(item(), raw, NOW + 1000)


@pytest.mark.parametrize(
    "changes",
    [
        {"source": "other"},
        {"source_category": "reporting"},
        {"title": "Minutes of the FOMC"},
        {"url": URL.replace("www.federalreserve.gov", "example.com")},
        {"url": URL + "?redirect=other"},
        {"url": URL.replace("a.htm", "a1.htm")},
        {"url": URL.replace("https://", "http://")},
        {"url": URL.replace("0916", "0729")},
        {"published_ms": NOW + 1_000_000},
        {"observed_ms": True},
    ],
)
def test_origin_identity_and_availability_are_bound(changes):
    with pytest.raises(ValueError):
        parse_statement({**item(), **changes}, document(), NOW + 1000)


def test_fetch_never_follows_redirects_or_accepts_oversized_responses():
    async def check():
        requests = []

        def redirect(request):
            requests.append(str(request.url))
            return httpx.Response(302, headers={"Location": "https://example.com/"})

        async with httpx.AsyncClient(
            transport=httpx.MockTransport(redirect), follow_redirects=True
        ) as client:
            with pytest.raises(ValueError, match="fetch"):
                await fetch_statement(client, item(), lambda: NOW + 1000)
        assert requests == [URL]
        async with httpx.AsyncClient(transport=httpx.MockTransport(
            lambda _: httpx.Response(
                200, content=b"x" * (MAX_STATEMENT_BYTES + 1), headers={"Content-Type": "text/html"}
            )
        )) as client:
            with pytest.raises(ValueError, match="bound"):
                await fetch_statement(client, item(), lambda: NOW + 1000)
        async with httpx.AsyncClient(transport=httpx.MockTransport(
            lambda _: httpx.Response(200, content=document(), headers={"Content-Type": "text/html"})
        )) as client:
            assert (await fetch_statement(client, item(), lambda: NOW + 1000))["action"] == "lower"

    asyncio.run(check())


def test_initial_feed_baseline_never_fetches_or_publishes_historical_statements(tmp_path):
    root = tmp_path / "private"
    root.mkdir(mode=0o700)
    store = NewsJournal(root)
    try:
        assert needs_source_facts(parse_feed(FEED, rss())[0])
        assert store.ingest(FEED, parse_feed(FEED, rss()), NOW + 2, {})["baseline"]
        assert store.pending_source_details() == store.pending() == []
    finally:
        store.close()


def test_staged_source_request_survives_restart_without_changing_intake_clocks(journal):
    root, store = journal
    original = store.pending_source_details()[0]
    assert not store.pending()
    restarted = NewsJournal(root)
    try:
        assert restarted.pending_source_details() == [original]
        restarted.ingest(
            FEED, parse_feed(FEED, rss(title="Edited statement title")), NOW + 500, {}
        )
        assert restarted.pending_source_details() == [original]
        facts = parse_statement(original, document(), NOW + 1000)
        assert restarted.finish_source_details(original["news_id"], facts, NOW + 1000)
        frozen = restarted.pending()[0]
        for key in ("news_id", "title", "url", "published_ms", "observed_ms"):
            assert frozen[key] == original[key]
        assert frozen["source_facts"] == facts and frozen["eligible"]
        assert not restarted.pending_source_details()
        assert not restarted.finish_source_details(original["news_id"], facts, NOW + 2000)
        assert restarted.pending() == [frozen]
        restarted.delivered(original["news_id"])
        assert restarted.recent_published(NOW + 2000, {FEED.id}) == [frozen]
    finally:
        restarted.close()


def test_late_details_cannot_rejuvenate_an_expired_statement(journal):
    _, store = journal
    original = store.pending_source_details()[0]
    with pytest.raises(ValueError, match="discarded"):
        store.finish_source_details(original["news_id"], None, NOW + 100)
    at = NOW + 120_001
    facts = parse_statement(original, document(), at)
    assert store.finish_source_details(original["news_id"], facts, at)
    assert not store.pending() and not store.pending_source_details()
    assert not store.recent_published(at, {FEED.id})


def test_mismatched_details_leave_the_original_request_pending(journal):
    _, store = journal
    original = store.pending_source_details()[0]
    facts = parse_statement(original, document(), NOW + 1000)
    with pytest.raises(ValueError, match="match staged"):
        store.finish_source_details(
            original["news_id"], {**facts, "source_url": URL.replace("a.htm", "b.htm")}, NOW + 1000
        )
    assert store.pending_source_details() == [original]
    assert not store.pending()


def test_worker_retries_with_the_original_timestamp_and_publishes_only_completed_facts(
    journal, monkeypatch
):
    _, store = journal
    original = store.pending_source_details()[0]
    clock, calls = [NOW + 10], []

    async def check():
        stop = asyncio.Event()
        metrics = {"source_details": {}}

        async def fetch(_, value, now):
            calls.append(value.copy())
            assert not store.pending()
            if len(calls) == 1:
                raise httpx.ReadTimeout("synthetic timeout")
            return parse_statement(value, document(), now())

        async def sleep(stopped, _):
            clock[0] += 250
            if len(calls) >= 2:
                stopped.set()
            await asyncio.sleep(0)

        monkeypatch.setattr(fast_runtime, "fetch_statement", fetch)
        monkeypatch.setattr(fast_runtime, "now_ms", lambda: clock[0])
        monkeypatch.setattr(fast_runtime, "sleep_or_stop", sleep)
        await asyncio.wait_for(fast_runtime.enrich_news(None, store, stop, metrics), 3)
        assert len(calls) == 2 and calls[0] == calls[1] == original
        assert store.pending()[0]["source_facts"]["action"] == "lower"
        assert store.pending()[0]["observed_ms"] == NOW + 2
        assert metrics["source_details"]["last_error_type"] is None

    asyncio.run(check())


def test_cancelled_fetch_keeps_the_durable_source_request(journal, monkeypatch):
    _, store = journal
    original = store.pending_source_details()[0]

    async def check():
        entered = asyncio.Event()

        async def fetch(*_):
            entered.set()
            await asyncio.Event().wait()

        monkeypatch.setattr(fast_runtime, "fetch_statement", fetch)
        monkeypatch.setattr(fast_runtime, "now_ms", lambda: NOW + 10)
        task = asyncio.create_task(
            fast_runtime.enrich_news(None, store, asyncio.Event(), {"source_details": {}})
        )
        await asyncio.wait_for(entered.wait(), 3)
        task.cancel()
        with pytest.raises(asyncio.CancelledError):
            await task
        assert store.pending_source_details() == [original]
        assert not store.pending()

    asyncio.run(check())
