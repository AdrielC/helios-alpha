"""Account observations stay private, decimal-exact, and separate from execution."""

import copy
import json
import os
from concurrent.futures import ThreadPoolExecutor
from datetime import UTC, datetime

import pytest

from helios_alpha.control.broker_snapshot import (
    FILENAME,
    MAX_AGE_MS,
    import_broker_observation,
    normalize_observation,
    read_broker_observation,
)
from helios_alpha.control.models import ClientGrant, PolicyUpdate, RiskLimits
from helios_alpha.control.store import ControlError, ControlStore

NOW = 1_800_000_000_000
BROKERAGE = "TEST00000001"
NUMERIC = "1000000001"
CRYPTO = "RHC00000001"
ACCOUNT_ID = "00000000-0000-4000-8000-000000000001"


def iso(stamp):
    return datetime.fromtimestamp(stamp / 1000, UTC).isoformat()


@pytest.fixture
def observation():
    def call(account, data):
        return {"requested_account": account, "received_ms": NOW, "data": data}

    return {
        "schema_version": 1,
        "account_received_ms": NOW,
        "account": {
            "account_number": BROKERAGE,
            "rhs_account_number": NUMERIC,
            "rhc_account_number": CRYPTO,
            "agentic_allowed": True,
            "state": "active",
            "deactivated": False,
            "permanently_deactivated": False,
        },
        "calls": {
            "portfolio": call(BROKERAGE, {
                "currency": "USD", "total_value": "123.4567", "cash": "100",
                "crypto_value": "23.4567",
                "buying_power": {"buying_power": "10000"},
                "crypto_buying_power": {"buying_power": "100"},
            }),
            "approvals": call(BROKERAGE, {"setting": {
                "account_number": BROKERAGE, "human_must_approve_trades": True,
                "updated_at": iso(NOW - 1000),
            }}),
            "positions": call(NUMERIC, {"results": [{
                "account_id": ACCOUNT_ID,
                "currency": {"code": "BTC"},
                "quantity": "0.12345678",
                "quantity_transferable": "0.10000000",
                "quantity_held_for_sell": "0.02345678",
            }]}),
            "open_orders": call(NUMERIC, {
                "rhs_account_number": NUMERIC, "crypto_account_number": CRYPTO, "results": [],
            }),
            "quotes": call(NUMERIC, {"results": [{
                "symbol": "BTCUSD", "bid_price": "100.123", "ask_price": "100.124",
                "updated_at": iso(NOW), "routing": "synthetic",
            }]}),
        },
        "previews": [{
            "request": {
                "rhs_account_number": NUMERIC, "symbol": "BTC-USD", "side": "buy",
                "type": "market", "dollar_amount": "10",
            },
            "received_ms": NOW,
            "data": {
                "crypto_account_number": CRYPTO,
                "customer_approval_required": True,
                "fee_rate": "synthetic", "estimated_fee": "0.1",
                "order": {
                    "id": "transient-preview-identifier",
                    "account_id": ACCOUNT_ID, "currency_code": "BTC",
                    "side": "buy", "type": "market",
                    "state": "new", "state_group": "open",
                    "quantity": "0.09876543", "cumulative_quantity": "0",
                    "rounded_estimated_notional_with_estimated_fee": "10",
                    "routing": "synthetic",
                },
            },
        }],
    }


@pytest.fixture
def store(tmp_path):
    return ControlStore.initialize(
        tmp_path / "private",
        RiskLimits(
            capital_micros=100_000_000, order_micros=10_000_000,
            daily_loss_micros=5_000_000, drawdown_micros=10_000_000,
            symbol_exposure_micros=50_000_000, max_daily_orders=2,
            max_spread_bps=10, max_quote_age_ms=1000, symbols=["BTC-USD"],
        ),
        [
            ClientGrant(client_id="observer", capabilities=["status:read", "risk:read"]),
            ClientGrant(
                client_id="operator", capabilities=["status:read", "risk:read", "risk:write"]
            ),
            ClientGrant(client_id="status-only", capabilities=["status:read"]),
        ],
        actor="test",
    )


def test_exact_quantities_and_preview_never_becomes_an_order(store, observation):
    before = store.status("observer")
    imported = import_broker_observation(store, "operator", observation, at_ms=NOW)
    assert imported["imported"] and not imported["execution_authority"]
    result = read_broker_observation(store, "observer", at_ms=NOW)
    assert result["positions"][0] == {
        "asset": "BTC", "quantity": "0.12345678", "sellable_quantity": "0.10000000",
        "quantity_in_open_sells": "0.02345678", "managed_by_helios": False,
    }
    assert result["open_orders"] == []
    assert result["previews"][0]["estimated_quantity"] == "0.09876543"
    assert result["previews"][0]["simulation_only"]
    assert not result["previews"][0]["is_live_order"]
    assert result["previews"][0]["sizing_checks"]["within_current_order_limit"]
    assert result["helios_execution"]["live_admitted"] is False
    assert result["semantics"]["requires_live_revalidation"]
    assert not result["execution_authority"]
    assert store.status("observer") == before
    path = store.root / FILENAME
    assert path.stat().st_mode & 0o777 == 0o600
    saved = path.read_text()
    for private_id in (BROKERAGE, NUMERIC, CRYPTO, ACCOUNT_ID, "transient-preview-identifier"):
        assert private_id not in saved


def test_different_ages_and_clock_regression_are_visible(store, observation):
    observation["calls"]["quotes"]["data"]["results"][0]["updated_at"] = iso(NOW - 6000)
    import_broker_observation(store, "operator", observation, at_ms=NOW)
    result = read_broker_observation(store, "observer", at_ms=NOW)
    assert result["state"] == "available"
    assert not result["quotes"][0]["fresh"]
    for stamp in (NOW - 1, NOW + MAX_AGE_MS + 1):
        result = read_broker_observation(store, "observer", at_ms=stamp)
        assert result["state"] == "stale"
        assert not result["previews"][0]["sizing_checks"]["observation_fresh"]


def test_actual_open_order_is_kept_separate(observation):
    observation["calls"]["open_orders"]["data"]["results"] = [{
        "id": "actual-open-order", "account_id": ACCOUNT_ID,
        "currency_code": "BTC", "side": "sell", "state": "confirmed", "state_group": "open",
    }]
    result = normalize_observation(observation, NOW)
    assert [order["order_id"] for order in result["open_orders"]] == ["actual-open-order"]
    assert len(result["previews"]) == 1


@pytest.mark.parametrize(
    ("path", "value"),
    [
        (("schema_version",), True),
        (("account_received_ms",), NOW + 1),
        (("account", "agentic_allowed"), False),
        (("account", "deactivated"), "false"),
        (("calls", "portfolio", "requested_account"), "different"),
        (("calls", "positions", "requested_account"), CRYPTO),
        (("calls", "approvals", "data", "setting", "account_number"), "different"),
        (("calls", "open_orders", "data", "crypto_account_number"), "different"),
        (("calls", "open_orders", "data", "rhs_account_number"), "different"),
        (("calls", "positions", "data", "next"), "more-positions"),
        (("calls", "open_orders", "data", "next"), "more-orders"),
        (("calls", "positions", "data", "results"), ""),
        (("calls", "positions", "data", "results"), {}),
        (("calls", "portfolio", "received_ms"), NOW + 1),
        (("calls", "portfolio", "received_ms"), False),
        (("calls", "portfolio", "data", "currency"), "EUR"),
        (("calls", "portfolio", "data", "cash"), "NaN"),
        (("calls", "portfolio", "data", "cash"), "1e20"),
        (("calls", "approvals", "data", "setting", "updated_at"), iso(NOW + 1)),
        (("previews", 0, "data", "order", "account_id"), "different"),
        (("previews", 0, "request", "rhs_account_number"), "different"),
        (("previews", 0, "data", "crypto_account_number"), "different"),
        (("previews", 0, "data", "order", "cumulative_quantity"), "0.1"),
        (
            ("previews", 0, "data", "order", "rounded_estimated_notional_with_estimated_fee"),
            "10.01",
        ),
        (("previews", 0, "data", "estimated_fee"), "11"),
        (("previews", 0, "data", "order", "currency_code"), "ETH"),
        (("previews", 0, "data", "order", "side"), "sell"),
        (("previews", 0, "data", "customer_approval_required"), 0),
        (("previews", 0, "received_ms"), NOW + 1),
    ],
)
def test_rejects_incomplete_cross_account_and_invalid_imports(observation, path, value):
    node = observation
    for key in path[:-1]:
        node = node[key]
    node[path[-1]] = value
    with pytest.raises(ControlError):
        normalize_observation(observation, NOW)


def test_crossed_or_empty_book_is_not_valid(observation):
    quote = observation["calls"]["quotes"]["data"]["results"][0]
    for bid, ask in (("0", "1"), ("2", "1")):
        quote.update(bid_price=bid, ask_price=ask)
        assert not normalize_observation(observation, NOW)["quotes"][0]["book_valid"]
    quote["updated_at"] = iso(NOW + 1001)
    with pytest.raises(ControlError, match="future"):
        normalize_observation(observation, NOW)


def test_asset_quantities_are_validated_without_decimal_rounding(observation):
    position = observation["calls"]["positions"]["data"]["results"][0]
    position.update(
        quantity="999999999999999999.999999999999999999",
        quantity_transferable="999999999999999999.999999999999999998",
        quantity_held_for_sell="0.000000000000000001",
    )
    result = normalize_observation(observation, NOW)
    assert result["positions"][0]["quantity"] == position["quantity"]
    position["quantity_held_for_sell"] = "0.000000000000000002"
    with pytest.raises(ControlError, match="quantity"):
        normalize_observation(observation, NOW)


def test_old_account_lookup_and_newer_calls_do_not_renew_observation(store, observation):
    observation["account_received_ms"] = NOW - MAX_AGE_MS - 1
    import_broker_observation(store, "operator", observation, at_ms=NOW)
    assert read_broker_observation(store, "observer", at_ms=NOW)["state"] == "stale"


def test_missing_results_do_not_mean_empty_holdings(observation):
    del observation["calls"]["positions"]["data"]["results"]
    with pytest.raises(ControlError, match="structure"):
        normalize_observation(observation, NOW)


def test_missing_crypto_power_does_not_substitute_equity_margin(store, observation):
    del observation["calls"]["portfolio"]["data"]["crypto_buying_power"]
    import_broker_observation(store, "operator", observation, at_ms=NOW)
    result = read_broker_observation(store, "observer", at_ms=NOW)
    assert result["portfolio"]["crypto_buying_power"] is None
    assert result["previews"][0]["sizing_checks"]["within_reported_crypto_buying_power"] is None


def test_reads_reapply_current_policy_including_fees(store, observation):
    import_broker_observation(store, "operator", observation, at_ms=NOW)
    previous = store.policy("operator")
    limits = dict(previous["limits"], order_micros=9_950_000)
    store.update_policy("operator", PolicyUpdate(
        expected_revision=previous["revision"], request_id="reduce-limit",
        limits=RiskLimits(**limits), reason="synthetic policy update",
    ))
    result = read_broker_observation(store, "observer", at_ms=NOW)
    assert result["current_policy_revision"] == previous["revision"] + 1
    # The preview's fee-inclusive $10 debit exceeds $9.95; its $9.90 notional does not.
    assert not result["previews"][0]["sizing_checks"]["within_current_order_limit"]


def test_permissions_checked_again_after_revocation(store, observation):
    assert read_broker_observation(store, "observer", at_ms=NOW)["state"] == "unavailable"
    with pytest.raises(ControlError, match="operator permission"):
        import_broker_observation(store, "observer", observation, at_ms=NOW)
    with pytest.raises(ControlError, match="capability"):
        read_broker_observation(store, "status-only", at_ms=NOW)
    import_broker_observation(store, "operator", observation, at_ms=NOW)
    with store.connection(write=True) as connection:
        connection.execute("DELETE FROM clients WHERE id IN ('observer', 'operator')")
    with pytest.raises(ControlError, match="capability"):
        read_broker_observation(store, "observer", at_ms=NOW)
    with pytest.raises(ControlError, match="capability"):
        import_broker_observation(store, "operator", observation, at_ms=NOW)


def test_import_cannot_replace_a_different_account_or_regress_one_call(store, observation):
    import_broker_observation(store, "operator", observation, at_ms=NOW)
    changed = copy.deepcopy(observation)
    # The minimum timestamp still equals NOW. Check every component, not just the minimum.
    changed["calls"]["positions"]["received_ms"] += 100
    import_broker_observation(store, "operator", changed, at_ms=NOW + 100)
    with pytest.raises(ControlError, match="backwards"):
        import_broker_observation(store, "operator", observation, at_ms=NOW + 200)
    changed["account"]["rhc_account_number"] = "RHC00000002"
    changed["calls"]["open_orders"]["data"]["crypto_account_number"] = "RHC00000002"
    changed["previews"][0]["data"]["crypto_account_number"] = "RHC00000002"
    with pytest.raises(ControlError, match="different account"):
        import_broker_observation(store, "operator", changed, at_ms=NOW + 200)


def test_import_serializes_competing_writers(store, observation):
    def write(index):
        raw = copy.deepcopy(observation)
        stamp = NOW + index
        raw["account_received_ms"] = stamp
        for call in raw["calls"].values():
            call["received_ms"] = stamp
        raw["previews"][0]["received_ms"] = stamp
        try:
            import_broker_observation(store, "operator", raw, at_ms=stamp)
            return "imported"
        except ControlError as error:
            assert "backwards" in str(error)
            return "rejected"

    with ThreadPoolExecutor(max_workers=4) as pool:
        results = list(pool.map(write, reversed(range(12))))
    assert "imported" in results
    result = read_broker_observation(store, "observer", at_ms=NOW + 12)
    assert result["observed_ms"] == NOW + 11
    assert not list(store.root.glob(".broker-observation-*"))


def test_read_detects_corruption_and_import_refuses_symlink(store, observation, tmp_path):
    import_broker_observation(store, "operator", observation, at_ms=NOW)
    path = store.root / FILENAME
    saved = json.loads(path.read_text())
    saved["positions"][0]["quantity"] = "1"
    path.write_text(json.dumps(saved))
    with pytest.raises(ControlError, match="integrity"):
        read_broker_observation(store, "observer", at_ms=NOW)
    path.unlink()
    target = tmp_path / "unrelated.json"
    target.write_text("untouched")
    path.symlink_to(target)
    with pytest.raises(ControlError, match="private"):
        import_broker_observation(store, "operator", observation, at_ms=NOW)
    assert target.read_text() == "untouched"


def test_interrupted_write_preserves_previous_observation(store, observation, monkeypatch):
    import_broker_observation(store, "operator", observation, at_ms=NOW)
    original = (store.root / FILENAME).read_bytes()

    def interrupted(*args):
        raise OSError("simulated interrupted replace")

    monkeypatch.setattr(os, "replace", interrupted)
    with pytest.raises(OSError, match="interrupted"):
        import_broker_observation(store, "operator", observation, at_ms=NOW + 1)
    assert (store.root / FILENAME).read_bytes() == original
    assert not list(store.root.glob(".broker-observation-*"))


def test_basis_uses_only_complete_direct_inventory_and_never_adds_intraday(observation):
    position = observation['calls']['positions']['data']['results'][0]
    position['cost_bases'] = [{
        'direct_quantity': position['quantity'], 'direct_cost_basis': '12.34567',
        'intraday_quantity': position['quantity'], 'intraday_cost_basis': '12.34567',
    }]
    assert normalize_observation(observation, NOW)['positions'][0]['cost_basis'] == '12.34567'
    position['cost_bases'][0]['direct_quantity'] = '0.00001'
    assert 'cost_basis' not in normalize_observation(observation, NOW)['positions'][0]
    position['cost_bases'] *= 2
    assert 'cost_basis' not in normalize_observation(observation, NOW)['positions'][0]


def test_broker_mark_is_preserved_and_malformed_values_rejected(observation):
    quote = observation['calls']['quotes']['data']['results'][0]
    quote['mark_price'] = '100.1235'
    assert normalize_observation(observation, NOW)['quotes'][0]['mark'] == '100.1235'
    quote['mark_price'] = 'NaN'
    with pytest.raises(ControlError):
        normalize_observation(observation, NOW)
