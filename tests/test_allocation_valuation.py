"""Broker reads and durable marks must remain bound, fresh and free of execution authority."""

import asyncio
import copy
import hashlib
import json
import threading
import uuid
from dataclasses import replace

import pytest

from helios_alpha.control.models import RiskLimits
from helios_alpha.control.store import ControlError, canonical
from helios_alpha.crypto import valuation_reconciler as v
from helios_alpha.crypto.account_reader import validate_account_response
from helios_alpha.crypto.order_recovery import RecoveryError

ACCOUNT = "a" * 64
ORDER = str(uuid.UUID(int=1, version=4))


def envelope(snapshot):
    return {"snapshot": snapshot, "snapshot_digest": v.digest(snapshot)}


def read_set(requests, now):
    account = {
        "schema_version": 1,
        "execution_authority": False,
        "account_key": ACCOUNT,
        "api_account_fingerprint": "b" * 64,
        "status": "active",
        "api_tradable": True,
        "buying_power_usd": "1000",
        "reported_fee_ratio": "0.0095",
        "reported_fee_bps": 95,
        "holdings": {
            "BTC": {"total_quantity": "1", "available_quantity": "0.1"},
            "ETH": {"total_quantity": "1", "available_quantity": "0.1"},
        },
        "read_started_ms": now,
        "read_completed_ms": now,
    }
    markets = []
    for request in requests:
        symbol = request["symbol"]
        price = "100000" if symbol == "BTC-USD" else "2000"
        markets.append(
            envelope(
                {
                    "schema_version": 1,
                    "execution_authority": False,
                    "account_key": ACCOUNT,
                    "api_account_fingerprint": "b" * 64,
                    "quantity": request["quantity"],
                    "rules": {
                        "symbol": symbol,
                        "observed_ms": now,
                        "asset_increment": "0.000000000000000001",
                        "quote_increment": "0.01",
                        "max_order_quantity": "10",
                        "minimum_order_micros": 1000000,
                        "status": "tradable",
                        "api_tradable": True,
                    },
                    "quote": {
                        "account_key": ACCOUNT,
                        "symbol": symbol,
                        "bid": price,
                        "ask": price,
                        "observed_ms": now,
                    },
                    "reported_fee_bps": 95,
                    "broker_quote_observed_ms": now,
                    "read_started_ms": now,
                    "read_completed_ms": now,
                }
            )
        )
    return validate_account_response(
        {
            "schema_version": 1,
            "availability": envelope(account),
            "markets": markets,
            "get_requests": 2 + 2 * len(markets),
        },
        ACCOUNT,
        requests=requests,
        batch=True,
        started=now,
        now=now,
    )


def settings():
    limits = RiskLimits(
        capital_micros=500_000_000,
        order_micros=20_000_000,
        daily_loss_micros=10_000_000,
        drawdown_micros=15_000_000,
        symbol_exposure_micros=250_000_000,
        max_daily_orders=7,
        max_spread_bps=20,
        max_quote_age_ms=5000,
        symbols=["BTC-USD", "ETH-USD"],
    ).model_dump()
    policy = {
        "schema_version": 1,
        "revision": 1,
        "limits": limits,
        "updated_ns": 0,
        "sha256": hashlib.sha256(json.dumps(limits, separators=(",", ":")).encode()).hexdigest(),
    }
    return {
        "schema_version": 1,
        "authority": "golem",
        "policy": policy,
        "client": {"id": "crypto-runtime", "capabilities": ["status:read", "risk:read"]},
        "execution": {"paused": False},
    }


class Control:
    def __init__(self):
        self.value = settings()
        self.calls = 0
        self.on_second = lambda: None

    def settings(self, actor):
        assert actor == "crypto-runtime"
        self.calls += 1
        if self.calls == 2:
            self.on_second()
        return copy.deepcopy(self.value)


class MemoryJournal:
    account_key = ACCOUNT

    def __init__(self, clock):
        self.clock = clock
        self.balance = {
            "execution_authority": False,
            "account_key": ACCOUNT,
            "ledger_digest": "b" * 64,
            "observed_ms": clock(),
            "cash_micros": 100_000_000,
            "cash_deficit_micros": 0,
            "quantities": {"BTC-USD": "0.001", "ETH-USD": "0.01"},
            "quantity_deficits": {},
            "available_cash_micros": 100_000_000,
            "available_quantities": {"BTC-USD": "0.001", "ETH-USD": "0.01"},
            "accounted_orders": 0,
            "unresolved_orders": False,
        }
        self.risk = None
        self.marks = []
        self.error = None
        self.scope = {"client_order_id": ORDER, "revision": 1, "intent_digest": "e" * 64}
        self.on_receipt = lambda risk: None

    def state(self):
        state = {
            "execution_authority": False,
            "account_key": ACCOUNT,
            "evaluated_ms": self.clock(),
            "balance": copy.deepcopy(self.balance),
            "balance_digest": "d" * 64 if self.balance is not None else None,
            "risk": copy.deepcopy(self.risk),
        }
        if state["risk"]:
            state["risk"]["current"] = (
                not self.balance["unresolved_orders"]
                and self.clock() < state["risk"]["valid_until_ms"]
            )
        return state

    def prepared(self, identity):
        assert identity == ORDER
        balance = copy.deepcopy(self.balance)
        balance.update(
            unresolved_orders=False,
            available_cash_micros=balance["cash_micros"],
            available_quantities=balance["quantities"],
        )
        return {
            "execution_authority": False,
            "scope": self.scope,
            "intent": {
                "client_order_id": ORDER,
                "account_key": ACCOUNT,
                "expires_ms": self.clock() + 2000,
            },
            "balance": balance,
            "balance_digest": "d" * 64,
        }

    def mark(self, payload, scope):
        self.marks.append(copy.deepcopy((payload, scope)))
        if self.error:
            raise self.error
        deadline = min(
            [payload["observed_ms"], *[q["quote"]["observed_ms"] for q in payload["quotes"]]]
        ) + min(payload["policy"]["limits"]["max_quote_age_ms"], 5000)
        if scope is not None:
            deadline = min(deadline, self.clock() + 2000)
        self.risk = {
            "current": True,
            "valuation_digest": "f" * 64,
            "balance_digest": payload["expected_balance_digest"],
            "source_digest": payload["source_digest"],
            "observed_ms": payload["observed_ms"],
            "valid_until_ms": deadline,
            "policy_revision": payload["policy"]["revision"],
            "policy_digest": payload["policy"]["sha256"],
            "day_utc": self.clock() // v.DAY_MS,
            "equity_micros": 100_000_000,
            "equity_deficit_micros": 0,
            "day_open_equity_micros": 100_000_000,
            "peak_equity_micros": 100_000_000,
            "gross_exposure_micros": 0,
            "symbol_exposure_micros": {},
            "daily_order_count": 0,
            "day_loss_latched": False,
            "drawdown_latched": False,
            "deficit_latched": False,
            "paused": payload["paused"],
            "fee_reserve_bps": payload["fee_reserve_bps"],
            "settlement_quantum_micros": 10000,
        }
        risk = copy.deepcopy(self.risk)
        self.on_receipt(risk)
        return {"execution_authority": False, "changed": True, "risk": risk}


@pytest.fixture
def context(monkeypatch):
    now = [1_000_000]

    def clock():
        return now[0]

    monkeypatch.setattr(v, "millis", clock)
    control, journal = Control(), MemoryJournal(clock)
    reads = []

    async def read(*, markets):
        reads.append(copy.deepcopy(markets))
        return read_set(markets, clock())

    fee = v.FeeBound(ACCOUNT, 100, "c" * 64, now[0] + 100_000)
    worker = v.ValuationWorker(control, journal, read, fee)
    return worker, control, journal, reads, now


def test_marks_all_owned_positions_without_adopting_broker_funds(context):
    worker, control, journal, reads, _ = context
    # Removing ETH from new-buy policy must not hide held ETH from equity.
    control.value["policy"]["limits"]["symbols"] = ["BTC-USD"]
    control.value["policy"]["sha256"] = hashlib.sha256(
        json.dumps(control.value["policy"]["limits"], separators=(",", ":")).encode()
    ).hexdigest()
    result = asyncio.run(worker.cycle())
    assert result["state"] == "marked" and result["execution_authority"] is False
    assert result["broker_requests"] == 6 and len(journal.marks) == 1
    assert reads == [
        [{"symbol": "BTC-USD", "quantity": "0.001"}, {"symbol": "ETH-USD", "quantity": "0.01"}]
    ]
    payload, scope = journal.marks[0]
    assert scope is None and len(payload["quotes"]) == 2
    assert journal.balance["cash_micros"] == 100_000_000
    assert payload["policy"] == control.value["policy"] and control.calls == 2
    assert result["risk"]["source_digest"] == payload["source_digest"]


@pytest.mark.parametrize("kind", ["allocation_uninitialized", "order_reconciliation_required"])
def test_missing_allocation_or_pending_order_never_launches_broker_reads(context, kind):
    worker, _, journal, reads, _ = context
    if kind == "allocation_uninitialized":
        journal.balance = None
    else:
        journal.balance["unresolved_orders"] = True
    assert asyncio.run(worker.cycle())["state"] == kind
    assert not reads and not journal.marks


def test_cash_only_allocation_still_requires_one_fresh_account_read(context):
    worker, _, journal, reads, _ = context
    journal.balance["quantities"] = journal.balance["available_quantities"] = {}
    result = asyncio.run(worker.cycle())
    assert result["broker_requests"] == 2 and reads == [[]]
    assert journal.marks[0][0]["quotes"] == []


@pytest.mark.parametrize("change", ["policy", "pause", "permission", "expiry"])
def test_settings_are_checked_again_before_the_mark(context, change):
    worker, control, journal, _, now = context

    def changed():
        if change == "policy":
            control.value["policy"]["revision"] += 1
        elif change == "pause":
            control.value["execution"]["paused"] = True
        elif change == "permission":
            control.value["client"]["capabilities"] = ["status:read"]
        else:
            now[0] += 5000

    control.on_second = changed
    with pytest.raises(RecoveryError):
        asyncio.run(worker.cycle())
    assert not journal.marks


@pytest.mark.parametrize(
    "change",
    [
        "missing_market",
        "wrong_quantity",
        "stale_market",
        "digest",
        "account",
        "fee_unknown",
        "fee_too_high",
        "missing_owned_units",
        "stale_account",
        "authority",
    ],
)
def test_partial_inconsistent_or_stale_reads_cannot_be_committed(context, change):
    worker, _, journal, _, now = context

    async def bad_read(*, markets):
        result = read_set(markets, now[0])
        account = result["availability"]["snapshot"]
        second = result["markets"][1]["snapshot"]
        if change == "missing_market":
            result["markets"].pop()
        elif change == "wrong_quantity":
            second["quantity"] = "0.02"
        elif change == "stale_market":
            second["broker_quote_observed_ms"] -= 5000
            second["quote"]["observed_ms"] -= 5000
        elif change == "digest":
            result["availability"]["snapshot_digest"] = "e" * 64
            return result
        elif change == "account":
            second["account_key"] = "e" * 64
        elif change == "fee_unknown":
            account["reported_fee_ratio"] = account["reported_fee_bps"] = None
        elif change == "fee_too_high":
            second["reported_fee_bps"] = 101
        elif change == "missing_owned_units":
            account["holdings"].pop("ETH")
        elif change == "stale_account":
            account["read_started_ms"] -= 5000
        elif change == "authority":
            result["execution_authority"] = True
        result["availability"] = envelope(account)
        result["markets"] = [envelope(m["snapshot"]) for m in result["markets"]]
        return result

    worker.read = bad_read
    with pytest.raises(RecoveryError):
        asyncio.run(worker.cycle())
    assert not journal.marks


@pytest.mark.parametrize("change", ["expired", "wrong_account", "expires_during_mark_window"])
def test_fee_bound_must_cover_the_account_and_full_valuation_window(context, change):
    worker, _, journal, reads, now = context
    updates = {
        "expired": {"valid_until_ms": now[0]},
        "wrong_account": {"account_key": "e" * 64},
        "expires_during_mark_window": {"valid_until_ms": now[0] + 1000},
    }
    worker.fee_bound = replace(worker.fee_bound, **updates[change])
    with pytest.raises(RecoveryError):
        asyncio.run(worker.cycle())
    assert not journal.marks
    if change != "expires_during_mark_window":
        assert not reads


def test_prepared_scope_keeps_the_real_reservation_and_clips_the_deadline(context):
    worker, _, journal, _, now = context
    asyncio.run(worker.cycle())
    journal.balance["unresolved_orders"] = True
    journal.balance["available_cash_micros"] -= 20_000_000
    result = asyncio.run(worker.cycle(client_order_id=ORDER))
    assert result["prepared_order"] == journal.scope
    assert journal.marks[-1][1] == journal.scope
    assert result["risk"]["valid_until_ms"] == now[0] + 2000
    assert journal.balance["unresolved_orders"]
    assert journal.balance["available_cash_micros"] == 80_000_000


def test_prepared_scope_cannot_initialize_risk_history_or_change_order_identity(context):
    worker, _, journal, reads, _ = context
    assert asyncio.run(worker.cycle(client_order_id=ORDER))["state"] == "risk_history_uninitialized"
    assert not reads
    asyncio.run(worker.cycle())
    journal.scope["client_order_id"] = str(uuid.UUID(int=2, version=4))
    with pytest.raises(RecoveryError, match="prepared_scope"):
        asyncio.run(worker.cycle(client_order_id=ORDER))
    assert len(journal.marks) == 1


def test_uncertain_mark_is_not_retried_inside_the_cycle(context):
    worker, _, journal, _, _ = context
    journal.error = ControlError("synthetic lost acknowledgement")
    with pytest.raises(ControlError):
        asyncio.run(worker.cycle())
    assert len(journal.marks) == 1


@pytest.mark.parametrize(
    "field,value",
    [
        ("source_digest", "e" * 64),
        ("policy_revision", 2),
        ("fee_reserve_bps", 0),
        ("balance_digest", "e" * 64),
        ("paused", True),
        ("valid_until_ms", 10_000_000),
    ],
)
def test_receipt_must_identify_the_exact_read_policy_balance_and_deadline(context, field, value):
    worker, _, journal, _, _ = context
    journal.on_receipt = lambda risk: risk.update({field: value})
    with pytest.raises(RecoveryError, match="receipt_binding"):
        asyncio.run(worker.cycle())
    assert len(journal.marks) == 1


def test_slow_acknowledgement_does_not_extend_valuation_freshness(context):
    worker, _, journal, _, now = context
    journal.on_receipt = lambda _: now.__setitem__(0, now[0] + 5000)
    result = asyncio.run(worker.cycle())
    assert result["state"] == "mark_expired" and not result["risk"]["current"]


def test_cancellation_waits_for_inflight_mark_before_releasing_the_cycle(context):
    worker, _, journal, _, _ = context
    entered, release = threading.Event(), threading.Event()
    original = journal.mark

    def slow_mark(payload, scope):
        entered.set()
        assert release.wait(3)
        return original(payload, scope)

    journal.mark = slow_mark

    async def check():
        task = asyncio.create_task(worker.cycle())
        assert await asyncio.to_thread(entered.wait, 3)
        task.cancel()
        await asyncio.sleep(0)
        assert not task.done()
        with pytest.raises(RecoveryError, match="in_progress"):
            await worker.cycle()
        release.set()
        with pytest.raises(asyncio.CancelledError):
            await task
        assert not worker._active

    asyncio.run(check())
    assert len(journal.marks) == 1


def test_golem_transport_binds_only_the_reconciler_identity(tmp_path):
    path = tmp_path / "broker-journal-connection.json"
    path.write_text(canonical({"schema_version": 1, "account_key": ACCOUNT}))
    path.chmod(0o600)

    class Store:
        root = tmp_path
        calls = []

        def _invoke_json_at(self, address, method, args):
            self.calls.append((address, method, args))
            return {"schema_version": 1, "execution_authority": False, "result": {}}

    store = Store()
    journal = v.GolemValuationJournal(store)
    journal.state()
    journal.prepared(ORDER)
    journal.mark({"schema_version": 1}, None)
    journal.mark({"schema_version": 1}, {"client_order_id": ORDER})
    assert [call[2][1] for call in store.calls] == [
        "risk_state",
        "prepared_allocation",
        "mark_allocation",
        "mark_prepared_allocation",
    ]
    assert all(call[2][0] == "broker-reconciler" for call in store.calls)


def test_disabled_or_missing_config_cannot_start_a_worker(tmp_path):
    assert asyncio.run(v.run(tmp_path, once=True))["state"] == "not_configured"
    path = tmp_path / v.CONFIG_FILE
    path.write_text(
        canonical(
            {
                "schema_version": 1,
                "enabled": False,
                "poll_seconds": 5,
                "fee_bound": vars(v.FeeBound(ACCOUNT, 100, "b" * 64, 100000)),
            }
        )
    )
    path.chmod(0o600)
    assert asyncio.run(v.run(tmp_path, once=True))["state"] == "disabled"
    with pytest.raises(RecoveryError, match="requires_once"):
        asyncio.run(v.run(tmp_path, client_order_id=ORDER))
