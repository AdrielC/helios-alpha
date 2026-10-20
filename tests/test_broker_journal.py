"""The inspection gateway cannot silently change accounts, fall back, or grant execution."""

import copy
import json
import uuid

import pytest

from helios_alpha.control.broker_journal import (
    CONNECTION_FILE,
    read_broker_allocation,
    read_broker_allocation_risk,
    read_broker_journal,
)
from helios_alpha.control.golem_store import GolemControlStore
from helios_alpha.control.store import ControlError

ACCOUNT = "a" * 64


class FakeGolemStore(GolemControlStore):
    def __init__(self, root):
        self.root = root
        self.allowed = True
        self.settings_calls = 0
        self.calls = []
        self.error = None
        self.reply = {
            "schema_version": 1,
            "execution_authority": False,
            "result": {
                "account_key": ACCOUNT,
                "record_count": 0,
                "snapshot_digest": "b" * 64,
                "reservations": {"cash_micros": 0, "sell_quantities": {}},
                "unresolved": [],
            },
        }

    def settings(self, client):
        self.settings_calls += 1
        if not self.allowed:
            raise ControlError("permission revoked")
        return {
            "client": {"id": client, "capabilities": ["status:read", "risk:read"]},
            "policy": {"revision": 3},
            "execution": {"mode": "paper", "live_admitted": False},
        }

    def _invoke_json_at(self, address, method, arguments, *, key=None):
        self.calls.append((address, method, arguments))
        if self.error:
            raise self.error
        return copy.deepcopy(self.reply)


@pytest.fixture
def store(tmp_path):
    root = tmp_path / "private"
    root.mkdir(mode=0o700)
    path = root / CONNECTION_FILE
    path.write_text(json.dumps({"schema_version": 1, "account_key": ACCOUNT}))
    path.chmod(0o600)
    return FakeGolemStore(root)


def test_reads_recheck_permissions_and_use_connection_bound_client(store):
    result = read_broker_journal(store, "observer")
    assert result["execution_authority"] is False
    assert result["authority"] == "golem"
    assert result["journal"]["record_count"] == 0
    assert result["current_policy_revision"] == 3
    assert len(store.calls) == 1
    address, method, arguments = store.calls[0]
    assert address == f'helios-alpha/local/CryptoBrokerJournalAgent("{ACCOUNT}")'
    assert method == "command" and arguments == ["observer", "status", "{}"]
    store.allowed = False
    with pytest.raises(ControlError, match="revoked"):
        read_broker_journal(store, "observer")
    assert store.settings_calls == 2 and len(store.calls) == 1


def test_missing_configuration_is_not_reported_as_zero_reservations(store):
    (store.root / CONNECTION_FILE).unlink()
    assert read_broker_journal(store, "observer") == {
        "state": "not_configured",
        "execution_authority": False,
    }
    assert not store.calls


def test_golem_failure_never_falls_back_to_empty_or_cached_state(store):
    store.error = ControlError("Golem unavailable")
    with pytest.raises(ControlError, match="unavailable"):
        read_broker_journal(store, "observer")


@pytest.mark.parametrize(
    "change",
    [
        {"schema_version": True},
        {"schema_version": 2},
        {"account_key": "A" * 64},
        {"account_key": '"injected-agent"'},
        {"server_url": "https://example.com"},
    ],
)
def test_configuration_rejects_shape_or_identity_changes(store, change):
    path = store.root / CONNECTION_FILE
    payload = {"schema_version": 1, "account_key": ACCOUNT, **change}
    path.write_text(json.dumps(payload))
    with pytest.raises(ControlError, match="connection"):
        read_broker_journal(store, "observer")
    assert not store.calls


def test_unsafe_permissions_and_symlinks_are_rejected(store):
    path = store.root / CONNECTION_FILE
    path.chmod(0o644)
    with pytest.raises(ControlError, match="private"):
        read_broker_journal(store, "observer")
    path.chmod(0o600)
    target = path.with_name("alternate.json")
    path.rename(target)
    path.symlink_to(target)
    with pytest.raises(ControlError, match="private"):
        read_broker_journal(store, "observer")
    assert not store.calls


@pytest.mark.parametrize(
    "change",
    [
        {"execution_authority": True},
        {"schema_version": True},
        {"schema_version": 2},
        {"result": []},
    ],
)
def test_malformed_or_authorizing_envelopes_are_rejected(store, change):
    store.reply.update(change)
    with pytest.raises(ControlError, match="response"):
        read_broker_journal(store, "observer")


@pytest.mark.parametrize(
    "change",
    [
        {"account_key": "f" * 64},
        {"record_count": True},
        {"record_count": -1},
        {"record_count": 257},
        {"snapshot_digest": "wrong"},
    ],
)
def test_summary_must_match_the_expected_account_and_bounds(store, change):
    store.reply["result"].update(change)
    with pytest.raises(ControlError, match="mismatch"):
        read_broker_journal(store, "observer")


def test_exact_record_identity_and_amounts_are_preserved(store):
    order_id = str(uuid.uuid4())
    record = {
        "intent": {
            "account_key": ACCOUNT,
            "client_order_id": order_id,
            "quantity": "0.000000000000000007",
        }
    }
    store.reply["result"] = record
    result = read_broker_journal(store, "observer", client_order_id=order_id)
    assert result["journal"] == record
    assert store.calls[-1][2][1:] == [
        "record",
        json.dumps({"client_order_id": order_id}, separators=(",", ":")),
    ]
    with pytest.raises(ControlError, match="identity"):
        read_broker_journal(store, "observer", client_order_id=str(uuid.uuid4()))


@pytest.mark.parametrize("order_id", ["bad", str(uuid.uuid4()).upper(), 3, "a" * 1000])
def test_invalid_order_identity_does_not_reach_golem(store, order_id):
    with pytest.raises(ControlError, match="identity"):
        read_broker_journal(store, "observer", client_order_id=order_id)
    assert not store.calls


def allocation_reply():
    return {
        "initialized": True,
        "balance": {
            "execution_authority": False,
            "account_key": ACCOUNT,
            "ledger_digest": "c" * 64,
            "observed_ms": 100,
            "cash_micros": 600_000_000,
            "cash_deficit_micros": 0,
            "available_cash_micros": 540_000_000,
            "quantities": {"BTC-USD": "0.1"},
            "available_quantities": {"BTC-USD": "0.000000000000000007"},
            "quantity_deficits": {},
            "accounted_orders": 1,
            "unresolved_orders": False,
        },
    }


def test_allocation_read_preserves_uninitialized_state_and_exact_balances(store):
    store.reply["result"] = {"initialized": False, "balance": None}
    assert read_broker_allocation(store, "observer")["allocation"] == store.reply["result"]
    assert store.calls[-1][2] == ["observer", "account_state", "{}"]
    store.reply["result"] = allocation_reply()
    result = read_broker_allocation(store, "observer")
    assert result["allocation"] == store.reply["result"]
    assert result["execution_authority"] is False
    assert result["semantics"]["not_a_live_broker_read"]
    store.allowed = False
    with pytest.raises(ControlError, match="revoked"):
        read_broker_allocation(store, "observer")
    assert len(store.calls) == 2


@pytest.mark.parametrize(
    "change",
    [
        {"account_key": "d" * 64},
        {"execution_authority": True},
        {"ledger_digest": "wrong"},
        {"cash_micros": True},
        {"cash_deficit_micros": -1},
        {"available_cash_micros": 601_000_000},
        {"accounted_orders": 257},
        {"unresolved_orders": "false"},
        {"available_quantities": {"BTC-USD": "0.2"}},
        {"available_quantities": {"ETH-USD": "0.1"}},
        {"quantities": {"BTC-USD": "NaN"}},
        {"quantity_deficits": {"UNKNOWN-USD": "1"}},
    ],
)
def test_invalid_allocation_cannot_masquerade_as_available_funds(store, change):
    store.reply["result"] = allocation_reply()
    store.reply["result"]["balance"].update(change)
    with pytest.raises(ControlError):
        read_broker_allocation(store, "observer")


def test_uninitialized_state_cannot_include_balances(store):
    store.reply["result"] = allocation_reply()
    store.reply["result"]["initialized"] = False
    with pytest.raises(ControlError, match="uninitialized"):
        read_broker_allocation(store, "observer")


def risk_reply():
    return {
        "execution_authority": False,
        "account_key": ACCOUNT,
        "evaluated_ms": 100,
        "balance": allocation_reply()["balance"],
        "balance_digest": "d" * 64,
        "risk": {
            "current": True,
            "valuation_digest": "e" * 64,
            "balance_digest": "d" * 64,
            "source_digest": "f" * 64,
            "observed_ms": 100,
            "valid_until_ms": 5000,
            "policy_revision": 3,
            "policy_digest": "1" * 64,
            "day_utc": 0,
            "equity_micros": 900,
            "equity_deficit_micros": 0,
            "day_open_equity_micros": 1000,
            "peak_equity_micros": 1100,
            "gross_exposure_micros": 200,
            "symbol_exposure_micros": {"BTC-USD": 200},
            "daily_order_count": 1,
            "day_loss_latched": True,
            "drawdown_latched": False,
            "deficit_latched": False,
            "paused": False,
            "fee_reserve_bps": 99,
            "settlement_quantum_micros": 1,
        },
    }


def test_risk_inspection_keeps_missing_and_stale_history_distinct(store):
    reply = risk_reply()
    reply.update(balance=None, balance_digest=None, risk=None)
    store.reply["result"] = reply
    result = read_broker_allocation_risk(store, "observer")
    assert result["allocation_risk"]["risk"] is None
    assert result["execution_authority"] is False
    assert store.calls[-1][2] == ["observer", "risk_state", "{}"]
    store.reply["result"] = risk_reply()
    result = read_broker_allocation_risk(store, "observer")
    assert result["allocation_risk"]["risk"]["day_loss_latched"]
    store.reply["result"]["evaluated_ms"] = 5100
    store.reply["result"]["risk"]["current"] = False
    assert not read_broker_allocation_risk(store, "observer")["allocation_risk"]["risk"]["current"]
    store.allowed = False
    with pytest.raises(ControlError, match="revoked"):
        read_broker_allocation_risk(store, "observer")
    assert len(store.calls) == 3


@pytest.mark.parametrize(
    "change",
    [
        {"current": "true"},
        {"day_loss_latched": 0},
        {"equity_micros": True},
        {"equity_micros": 1200},
        {"equity_deficit_micros": 1},
        {"daily_order_count": 257},
        {"fee_reserve_bps": 10001},
        {"settlement_quantum_micros": 0},
        {"valid_until_ms": 100},
        {"day_utc": 1},
        {"balance_digest": "2" * 64},
        {"valuation_digest": "invalid"},
        {"gross_exposure_micros": 201},
        {"symbol_exposure_micros": {"OTHER-USD": 200}},
        {"symbol_exposure_micros": {"BTC-USD": "200"}},
    ],
)
def test_risk_response_cannot_present_invalid_or_expired_evidence_as_current(store, change):
    store.reply["result"] = risk_reply()
    store.reply["result"]["risk"].update(change)
    with pytest.raises(ControlError):
        read_broker_allocation_risk(store, "observer")


def test_risk_response_rejects_unknown_account_and_pending_orders(store):
    for change in ({"account_key": "e" * 64}, {"execution_authority": True}):
        store.reply["result"] = risk_reply()
        store.reply["result"].update(change)
        with pytest.raises(ControlError):
            read_broker_allocation_risk(store, "observer")
    store.reply["result"] = risk_reply()
    store.reply["result"]["balance"]["unresolved_orders"] = True
    with pytest.raises(ControlError):
        read_broker_allocation_risk(store, "observer")
