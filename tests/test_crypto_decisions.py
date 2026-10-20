import asyncio
import json
import stat
from types import SimpleNamespace
from unittest.mock import AsyncMock, Mock

import pytest

from helios_alpha.control.store import canonical
from helios_alpha.crypto import decisions

NOW = 1_800_000_000_000
IDENTITY = "a" * 64
FALLBACK = {"provider": "local", "veto_new_risk": True}


@pytest.fixture
def review(tmp_path, monkeypatch):
    root = tmp_path / "private"
    root.mkdir(mode=0o700)
    executable = tmp_path / "test-decision"
    executable.write_text("test fixture; never executed")
    executable.chmod(0o700)
    harness = SimpleNamespace(
        root=root,
        config={"enabled": True, "executable": str(executable)},
        event={"observed_ms": NOW - 1000, "symbol": "BTC-USD"},
        clock=NOW,
        receipt_changes={},
        probability=0.1,
        after_response=None,
        store=SimpleNamespace(command=Mock(return_value={"send_allowed": True})),
    )
    monkeypatch.setattr(decisions.time, "time_ns", lambda: harness.clock * 1_000_000)

    async def communicate(wire):
        request = json.loads(wire)
        harness.request = request
        receipt = {
            "schema_version": 1,
            "request_id": request["request_id"],
            "effective_at_ms": request["effective_at_ms"],
            "available_at_ms": request["available_at_ms"],
            "received_at_ms": NOW,
            "outcome": {
                "Ok": {
                    "answers": {
                        "hold_off": {"type": "predicate", "probability": harness.probability}
                    }
                }
            },
            **harness.receipt_changes,
        }
        if harness.after_response is not None:
            harness.clock = harness.after_response
        return canonical({"Ok": {"receipt": receipt}}).encode(), None

    harness.process = SimpleNamespace(
        returncode=0,
        communicate=AsyncMock(side_effect=communicate),
        kill=Mock(),
        wait=AsyncMock(),
    )
    harness.spawn = AsyncMock(return_value=harness.process)
    monkeypatch.setattr(asyncio, "create_subprocess_exec", harness.spawn)

    async def call():
        return await decisions.review_event(
            root,
            harness.store,
            harness.config,
            harness.event,
            fallback_veto=True,
            identity=IDENTITY,
            expires_ms=NOW + 60_000,
        )

    harness.call = call
    return harness


@pytest.mark.parametrize("executable", [None, "relative/path", "/does/not/exist"])
def test_invalid_executable_does_not_consume_decision_budget(review, executable):
    review.config["executable"] = executable
    assert asyncio.run(review.call()) == FALLBACK
    review.store.command.assert_not_called()
    review.spawn.assert_not_called()


def test_oversized_event_does_not_consume_decision_budget(review):
    review.event["title"] = "x" * 4096
    assert asyncio.run(review.call()) == FALLBACK
    review.store.command.assert_not_called()
    review.spawn.assert_not_called()


def test_reservation_denial_never_launches_model(review):
    review.store.command.return_value = {"send_allowed": False}
    assert asyncio.run(review.call()) == FALLBACK
    review.store.command.assert_called_once()
    review.spawn.assert_not_called()


def test_valid_review_is_private_atomic_and_reused_without_another_reservation(review):
    expected = {"provider": "openai", "veto_new_risk": False}
    assert asyncio.run(review.call()) == expected
    path = review.root / "decision-cache" / (IDENTITY + ".json")
    assert stat.S_IMODE(path.stat().st_mode) == 0o600
    assert not list(path.parent.glob(".decision-*"))
    assert asyncio.run(review.call()) == expected
    review.store.command.assert_called_once()
    review.spawn.assert_called_once()


@pytest.mark.parametrize(
    "changes",
    [
        {"schema_version": 2},
        {"request_id": "b" * 64},
        {"effective_at_ms": NOW - 2000},
        {"available_at_ms": NOW - 1},
        {"received_at_ms": NOW + 1},
        {"received_at_ms": NOW - 1},
    ],
)
def test_unbound_or_impossible_receipts_cannot_allow_new_risk(review, changes):
    review.receipt_changes = changes
    assert asyncio.run(review.call()) == FALLBACK
    assert not list((review.root / "decision-cache").glob("*.json"))


@pytest.mark.parametrize("probability", [True, "0.1", -0.1, 1.1, None])
def test_invalid_predicate_cannot_allow_new_risk(review, probability):
    review.probability = probability
    assert asyncio.run(review.call()) == FALLBACK


def test_answer_arriving_after_its_deadline_is_not_cached(review):
    review.after_response = NOW + 8000
    assert asyncio.run(review.call()) == FALLBACK
    assert not list((review.root / "decision-cache").glob("*.json"))


@pytest.mark.parametrize(
    "contents",
    [
        "{",
        canonical({"expires_ms": NOW + 60_000, "result": {"veto_new_risk": False}}),
        canonical(
            {
                "schema_version": 1,
                "request_id": IDENTITY,
                "expires_ms": NOW + 60_000,
                "result": {"provider": "openai", "veto_new_risk": "false"},
            }
        ),
    ],
)
def test_invalid_cache_holds_without_resending_or_crashing(review, contents):
    cache = review.root / "decision-cache"
    cache.mkdir(mode=0o700)
    path = cache / (IDENTITY + ".json")
    path.write_text(contents)
    path.chmod(0o600)
    assert asyncio.run(review.call()) == FALLBACK
    review.store.command.assert_not_called()
    review.spawn.assert_not_called()


def test_interrupted_model_is_killed_and_reaped(review):
    async def check():
        entered = asyncio.Event()
        never = asyncio.Event()

        async def hang(_):
            entered.set()
            await never.wait()

        review.process.communicate.side_effect = hang
        review.process.returncode = None
        review.process.kill.side_effect = lambda: setattr(review.process, "returncode", -9)
        task = asyncio.create_task(review.call())
        await asyncio.wait_for(entered.wait(), 1)
        task.cancel()
        with pytest.raises(asyncio.CancelledError):
            await task
        review.process.kill.assert_called_once()
        review.process.wait.assert_awaited_once()
        assert not list((review.root / "decision-cache").glob("*.json"))

    asyncio.run(check())


def test_model_timeout_keeps_the_local_veto_and_reaps_process(review):
    review.process.communicate.side_effect = TimeoutError
    review.process.returncode = None
    review.process.kill.side_effect = lambda: setattr(review.process, "returncode", -9)
    assert asyncio.run(review.call()) == FALLBACK
    review.process.kill.assert_called_once()
    review.process.wait.assert_awaited_once()
