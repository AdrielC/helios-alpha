import asyncio
from types import SimpleNamespace
from unittest.mock import AsyncMock, Mock

import pytest

pytest.importorskip("nats")
pytest.importorskip("websockets")
pytest.importorskip("defusedxml")

from helios_alpha.crypto import fast_runtime

NOW = 1_800_000_000_000


@pytest.mark.parametrize(
    "evidence_fails,btc_stale,entry_attempted,inputs_fail,missing_support,unusual_event",
    [
        (False, False, False, False, False, True),
        (True, False, False, False, False, True),
        (False, True, False, False, False, True),
        (False, False, True, False, False, True),
        (False, False, False, True, False, True),
        (False, False, False, False, True, True),
        (False, False, False, False, True, False),
    ],
)
def test_pending_entry_review_does_not_block_another_assets_exit(
    tmp_path,
    monkeypatch,
    evidence_fails,
    btc_stale,
    entry_attempted,
    inputs_fail,
    missing_support,
    unusual_event,
):
    root = tmp_path / "private"
    root.mkdir(mode=0o700)
    symbols = ["BTC-USD", "ETH-USD"]
    news = {
        "news_id": "a" * 64,
        "observed_ms": NOW - 1000,
        "published_ms": NOW - 1000,
        "eligible": True,
        "title": "Synthetic test event",
        "source": "synthetic",
    }
    config = {
        "schema_version": 1,
        "mode": "paper",
        "symbols": symbols,
        "news": {"poll_seconds": 30, "poll_seconds_by_source": {"synthetic": 15}},
        "strategy_path": str(root / "unused.py"),
        "strategy_config_path": str(root / "unused.json"),
        "paper": {"fee_bps": 95, "slippage_bps": 5},
        "decisions": {"enabled": True},
    }
    settings = {
        "policy": {"revision": 1, "limits": {"max_quote_age_ms": 5000}},
        "execution": {"paused": False},
        "paper_costs": config["paper"],
    }
    paper = {"positions": {"ETH-USD": 1_000_000}, "cursor": 0}
    store = SimpleNamespace(
        command=Mock(return_value={"signal_deadline_version": 1}),
        settings=Mock(return_value=settings),
        status=Mock(return_value={"paper": paper}),
        heartbeat=Mock(),
    )
    quotes = [
        {"symbol": symbol, "bid_micros": 100_000_000, "ask_micros": 100_001_000, "observed_ms": NOW}
        for symbol in symbols
    ]
    book = fast_runtime.QuoteBook()
    book.connected = True
    book.connections = 1
    for q in quotes:
        at = NOW - 6000 if btc_stale and q["symbol"] == "BTC-USD" else NOW
        book.accept({**q, "observed_ms": at, "received_ms": at, "sequence": 1})
    if evidence_fails:

        def unavailable_evidence(_):
            raise OSError("synthetic evidence storage failure")

        monkeypatch.setattr(fast_runtime, "NewsEvidence", unavailable_evidence)

    def propose(context):
        if context["symbol"] == "BTC-USD":
            result = {
                "action": "enter",
                "allocation_bps": 1000,
                "reason": "synthetic_entry",
                "unusual_event": unusual_event,
                "news_id": news["news_id"],
            }
            if missing_support:
                result["evidence_news_ids"] = [news["news_id"], "b" * 64]
            return result
        return {
            "action": "exit",
            "allocation_bps": 0,
            "reason": "synthetic_exit",
            "unusual_event": False,
        }

    strategy = SimpleNamespace(fingerprint="b" * 64, propose=propose)
    journal = SimpleNamespace(
        pending_entries=lambda: [],
        recent_published=lambda *_: [],
        entry_attempted=lambda _: entry_attempted,
        close=Mock(),
    )
    connection = SimpleNamespace(close=AsyncMock())
    monkeypatch.setattr(
        fast_runtime,
        "read_private_json",
        lambda path: (
            config if path.name == "runtime.json" else {"url": "unused", "token": "fixture"}
        ),
    )
    monkeypatch.setattr(
        fast_runtime, "configured_feeds", lambda _: [SimpleNamespace(id="synthetic")]
    )
    monkeypatch.setattr(fast_runtime, "poll_feed", AsyncMock())
    monkeypatch.setattr(fast_runtime, "PrivateStrategy", lambda *_: strategy)
    monkeypatch.setattr(
        fast_runtime,
        "DecisionSnapshots",
        lambda *_: SimpleNamespace(capture=lambda *_: "c" * 64, close=Mock()),
    )
    if inputs_fail:

        def unavailable_inputs(*_):
            raise OSError("synthetic decision archive failure")

        monkeypatch.setattr(fast_runtime, "DecisionSnapshots", unavailable_inputs)
    monkeypatch.setattr(fast_runtime, "GolemControlStore", lambda _: store)
    monkeypatch.setattr(fast_runtime, "NewsJournal", lambda _: journal)
    monkeypatch.setattr(fast_runtime, "QuoteBook", lambda: book)
    monkeypatch.setattr(fast_runtime, "now_ms", lambda: NOW)
    monkeypatch.setattr(fast_runtime.nats, "connect", AsyncMock(return_value=connection))
    monkeypatch.setattr(fast_runtime, "ensure_stream", AsyncMock())

    async def check():
        monkeypatch.setattr(asyncio.get_running_loop(), "add_signal_handler", Mock())
        reviewing = asyncio.Event()
        cancelled = asyncio.Event()
        exit_published = asyncio.Event()
        published = []
        stop = None

        async def hang_review(*_, **__):
            reviewing.set()
            try:
                await asyncio.Event().wait()
            finally:
                cancelled.set()

        async def news_task(_journal, _js, arrived, stopped, _metrics):
            nonlocal stop
            stop = stopped
            arrived.append(news)
            await stopped.wait()

        async def quotes_task(_book, _symbols, _changed, stopped):
            await stopped.wait()

        async def publish(event):
            published.append(event)
            if event["context"]["action"] == "exit":
                assert reviewing.is_set() == (
                    not any(
                        (evidence_fails, btc_stale, entry_attempted, inputs_fail, missing_support)
                    )
                )
                assert not cancelled.is_set()
                exit_published.set()
                stop.set()
            # Give the pending model task a turn without waiting for its result.
            await asyncio.sleep(0)
            return SimpleNamespace(seq=len(published))

        transport = SimpleNamespace(js=object(), publish=publish, drain=AsyncMock(return_value=[]))
        monkeypatch.setattr(
            fast_runtime.PaperTransport, "connect", AsyncMock(return_value=transport)
        )
        monkeypatch.setattr(fast_runtime, "review_event", hang_review)
        monkeypatch.setattr(fast_runtime, "watch_quotes", quotes_task)
        monkeypatch.setattr(fast_runtime, "publish_news", news_task)
        await asyncio.wait_for(fast_runtime.run(root), 2)
        assert exit_published.is_set()
        assert cancelled.is_set() == (
            not any((evidence_fails, btc_stale, entry_attempted, inputs_fail, missing_support))
        )
        assert len([e for e in published if e["context"]["action"] == "exit"]) == 1
        assert not any(e["context"]["action"] == "enter" for e in published)
        connection.close.assert_awaited_once()

    asyncio.run(check())
    assert fast_runtime.poll_feed.call_args.args[-1] == 15
