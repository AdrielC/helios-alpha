import asyncio
import hashlib
import json
import threading
from dataclasses import replace
from types import SimpleNamespace
from unittest.mock import AsyncMock

import pytest

pytest.importorskip("nats")

from nats.errors import TimeoutError as NatsTimeout
from nats.js.api import ConsumerConfig
from nats.js.errors import NotFoundError

from helios_alpha.control.store import canonical
from helios_alpha.crypto import paper_transport
from helios_alpha.crypto.paper_transport import PaperTransport, event_envelope
from helios_alpha.crypto.runtime import parse_event

NOW = 1_800_000_000_000


def message():
    event = event_envelope(
        available_ms=NOW,
        policy_revision=1,
        quote={
            "symbol": "BTC-USD",
            "bid_micros": 100_000_000,
            "ask_micros": 100_010_000,
            "observed_ms": NOW,
        },
        portfolio_quotes=[],
        target_micros=10_000_000,
        strategy_fingerprint="synthetic",
        decision_provider="test",
        context={"observation_only": False},
    )
    return SimpleNamespace(
        data=canonical(event).encode(),
        metadata=SimpleNamespace(sequence=SimpleNamespace(stream=1)),
        ack_sync=AsyncMock(),
        nak=AsyncMock(),
        in_progress=AsyncMock(),
    )


class Store:
    def __init__(self):
        self.state = {"paper": {"source_id": "synthetic", "cursor": 0}}
        self.effects = 0

    def status(self, _):
        return self.state

    def policy(self, _):
        return {"revision": 1, "limits": {"symbol_exposure_micros": 100_000_000}}

    def command(self, _, method, payload):
        assert method == "paper_step"
        assert payload["expected_cursor"] == self.state["paper"]["cursor"]
        self.effects += 1
        self.state["paper"]["cursor"] = payload["source_sequence"]
        return {"outcome": "simulated_fill", "cursor": payload["source_sequence"]}


def transport(store, msg):
    result = PaperTransport()
    result.store = store
    result.source_id = "synthetic"
    result.subscription = SimpleNamespace(fetch=AsyncMock(side_effect=[[msg], NatsTimeout()]))
    return result


def test_transport_preserves_original_signal_after_delivery_delay():
    async def check():
        msg = message()
        event = json.loads(msg.data)
        event.pop("event_id")
        signal = {
            "input_digest": "a" * 64,
            "available_ms": NOW - 119_999,
            "expires_ms": NOW + 1,
        }
        event["signal"] = signal
        msg.data = canonical(event_envelope(**event)).encode()
        seen = []
        store = Store()
        command = store.command

        def capture(*args):
            seen.append(args[-1])
            return command(*args)

        store.command = capture
        await transport(store, msg).drain()
        assert seen[0]["signal"] == signal
        assert seen[0]["available_ms"] == NOW
        msg.ack_sync.assert_awaited_once()

    asyncio.run(check())


@pytest.mark.parametrize(
    "changes",
    [
        {"available_ms": True},
        {"available_ms": NOW + 1},
        {"expires_ms": NOW},
        {"expires_ms": NOW + 120_001},
        {"expires_ms": 2**64},
        {"input_digest": "not-a-digest"},
        {"unknown": 1},
    ],
)
def test_event_rejects_malformed_signal_validity(changes):
    event = json.loads(message().data)
    event.pop("event_id")
    event["signal"] = {
        "input_digest": "a" * 64,
        "available_ms": NOW,
        "expires_ms": NOW + 1000,
        **changes,
    }
    with pytest.raises(ValueError, match="signal"):
        event_envelope(**event)


def test_signal_deadline_is_bound_to_the_immutable_event_digest():
    event = json.loads(message().data)
    event.pop("event_id")
    event["signal"] = {
        "input_digest": "a" * 64,
        "available_ms": NOW,
        "expires_ms": NOW + 1000,
    }
    event = event_envelope(**event)
    event["signal"]["expires_ms"] += 1
    with pytest.raises(ValueError, match="digest"):
        parse_event(canonical(event).encode())
    # A legacy event is still decoded as legacy: no deadline is invented on recovery.
    legacy = parse_event(message().data)
    assert "signal" not in legacy
    body = {k: v for k, v in legacy.items() if k != "event_id"}
    assert legacy["event_id"] == hashlib.sha256(canonical(body).encode()).hexdigest()


def test_slow_commit_renews_lease_and_stops_after_ack(monkeypatch):
    monkeypatch.setattr(paper_transport, "LEASE_REFRESH_SECONDS", 0.01)

    async def check():
        msg = message()
        store = Store()
        started, finish = threading.Event(), threading.Event()
        command = store.command

        def slow_command(*args):
            started.set()
            assert finish.wait(2)
            return command(*args)

        store.command = slow_command
        task = asyncio.create_task(transport(store, msg).drain())
        try:
            assert await asyncio.to_thread(started.wait, 1)
            async with asyncio.timeout(1):
                while msg.in_progress.await_count < 2:
                    await asyncio.sleep(0.005)
            msg.ack_sync.assert_not_awaited()
            finish.set()
            assert len(await task) == 1
            count = msg.in_progress.await_count
            await asyncio.sleep(0.03)
            assert msg.in_progress.await_count == count
            msg.ack_sync.assert_awaited_once()
            msg.nak.assert_not_awaited()
        finally:
            finish.set()
            await asyncio.gather(task, return_exceptions=True)

    asyncio.run(check())


def test_canceled_unknown_commit_is_nacked_then_reconciled(monkeypatch):
    monkeypatch.setattr(paper_transport, "LEASE_REFRESH_SECONDS", 0.01)

    async def check():
        msg, store = message(), Store()
        started, finish, committed = threading.Event(), threading.Event(), threading.Event()
        command = store.command

        def uncertain_command(*args):
            started.set()
            assert finish.wait(2)
            result = command(*args)
            committed.set()
            return result

        store.command = uncertain_command
        connection = transport(store, msg)
        task = asyncio.create_task(connection.drain())
        try:
            assert await asyncio.to_thread(started.wait, 1)
            task.cancel()
            with pytest.raises(asyncio.CancelledError):
                await task
            msg.ack_sync.assert_not_awaited()
            msg.nak.assert_awaited_once_with(delay=paper_transport.REDELIVERY_DELAY_SECONDS)
            # Canceling to_thread does not stop an already-running Golem call.
            finish.set()
            assert await asyncio.to_thread(committed.wait, 1)
            count = msg.in_progress.await_count
            await asyncio.sleep(0.03)
            assert msg.in_progress.await_count == count
            connection.subscription.fetch = AsyncMock(side_effect=[[msg], NatsTimeout()])
            assert await connection.drain() == []
            assert store.effects == 1
            msg.ack_sync.assert_awaited_once()
        finally:
            finish.set()
            await asyncio.gather(task, return_exceptions=True)

    asyncio.run(check())


def test_reconciliation_error_survives_failed_nak():
    async def check():
        msg, store = message(), Store()
        store.state["paper"]["source_id"] = "different-stream"
        msg.nak.side_effect = ConnectionError("broker disconnected")
        with pytest.raises(ValueError, match="stream identity"):
            await transport(store, msg).drain()
        assert store.effects == 0
        msg.ack_sync.assert_not_awaited()
        msg.nak.assert_awaited_once()

    asyncio.run(check())


def broker(config, monkeypatch):
    js = SimpleNamespace(
        consumer_info=AsyncMock(return_value=SimpleNamespace(config=config)),
        add_consumer=AsyncMock(),
        pull_subscribe=AsyncMock(),
    )
    if config is None:
        js.consumer_info.side_effect = NotFoundError()
    connection = SimpleNamespace(
        jetstream=lambda **_: js,
        request=AsyncMock(
            return_value=SimpleNamespace(
                data=json.dumps({"created": "2026-10-20T01:00:00Z"}).encode()
            )
        ),
    )
    monkeypatch.setattr(paper_transport, "ensure_stream", AsyncMock())
    return connection, js


def existing_config():
    return ConsumerConfig(
        name="test-durable",
        durable_name="test-durable",
        filter_subject="test.>",
        max_ack_pending=1,
        ack_wait=90,
        max_deliver=-1,
        metadata={"preserve": "consumer settings"},
    )


def test_existing_consumer_changes_only_lease_without_reset(monkeypatch):
    async def check():
        before = existing_config()
        connection, js = broker(before, monkeypatch)
        await PaperTransport.connect(
            connection, Store(), stream="TEST", subject="test.>", durable="test-durable"
        )
        updated = js.add_consumer.call_args.kwargs["config"]
        assert updated == replace(before, ack_wait=paper_transport.ACK_WAIT_SECONDS)
        assert before.ack_wait == 90
        assert updated.metadata == before.metadata
        js.add_consumer.assert_awaited_once()
        assert js.pull_subscribe.call_args.kwargs["durable"] == "test-durable"

    asyncio.run(check())


def test_new_consumer_gets_short_lease_without_migration(monkeypatch):
    async def check():
        connection, js = broker(None, monkeypatch)
        await PaperTransport.connect(connection, Store())
        js.add_consumer.assert_not_awaited()
        assert js.pull_subscribe.call_args.kwargs["config"].ack_wait == 5

    asyncio.run(check())


@pytest.mark.parametrize(
    "changes",
    [
        {"filter_subject": "another.>"},
        {"filter_subjects": ["test.>", "another.>"]},
        {"ack_policy": "none"},
        {"deliver_policy": "new"},
        {"max_ack_pending": 2},
        {"backoff": [90]},
        {"deliver_subject": "push.inbox"},
    ],
)
def test_unexpected_consumer_is_not_rewritten(monkeypatch, changes):
    async def check():
        connection, js = broker(replace(existing_config(), **changes), monkeypatch)
        with pytest.raises(ValueError, match="reconciliation"):
            await PaperTransport.connect(
                connection, Store(), stream="TEST", subject="test.>", durable="test-durable"
            )
        js.add_consumer.assert_not_awaited()
        js.pull_subscribe.assert_not_awaited()

    asyncio.run(check())
