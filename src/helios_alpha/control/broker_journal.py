"""Permission-checked inspection of the durable Golem journal. No broker or journal writes."""

from __future__ import annotations

import json
import re
import uuid
from decimal import Decimal

from .golem_store import GolemControlStore, read_private_json
from .store import ControlError, canonical, private_root

CONNECTION_FILE = "broker-journal-connection.json"
HEX_DIGEST = re.compile(r"[a-f0-9]{64}\Z")
QUANTITY = re.compile(r"(?:0|[1-9][0-9]{0,17})(?:\.[0-9]{1,18})?\Z")


def _validate_allocation(result: dict, account: str) -> None:
    if set(result) != {"initialized", "balance"} or type(result["initialized"]) is not bool:
        raise ControlError("invalid allocation state")
    balance = result["balance"]
    if not result["initialized"]:
        if balance is not None:
            raise ControlError("uninitialized allocation has a balance")
        return
    if (
        not isinstance(balance, dict)
        or balance.get("execution_authority") is not False
        or balance.get("account_key") != account
        or not isinstance(balance.get("ledger_digest"), str)
        or not HEX_DIGEST.fullmatch(balance["ledger_digest"])
        or type(balance.get("unresolved_orders")) is not bool
        or any(
            type(balance.get(key)) is not int or not 0 <= balance[key] < 2**64
            for key in (
                "observed_ms",
                "cash_micros",
                "cash_deficit_micros",
                "available_cash_micros",
                "accounted_orders",
            )
        )
        or balance["available_cash_micros"] > balance["cash_micros"]
        or balance["accounted_orders"] > 256
    ):
        raise ControlError("allocation identity or balance mismatch")
    for key in ("quantities", "available_quantities", "quantity_deficits"):
        amounts = balance.get(key)
        if (
            not isinstance(amounts, dict)
            or not set(amounts) <= {"BTC-USD", "ETH-USD"}
            or any(not isinstance(v, str) or not QUANTITY.fullmatch(v) for v in amounts.values())
        ):
            raise ControlError("invalid allocation quantities")
    if any(
        symbol not in balance["quantities"]
        or Decimal(amount) > Decimal(balance["quantities"][symbol])
        for symbol, amount in balance["available_quantities"].items()
    ):
        raise ControlError("available allocation exceeds owned units")


def _inspect_broker_journal(
    store,
    client: str,
    *,
    client_order_id: str | None = None,
    allocation: bool = False,
    risk: bool = False,
) -> dict:
    # The trusted MCP launch binds client; it is never supplied by the tool caller.
    settings = store.settings(client)
    if not {"status:read", "risk:read"} <= set(settings["client"]["capabilities"]):
        raise ControlError("journal inspection requires status and risk read permissions")
    if client_order_id is not None:
        try:
            if str(uuid.UUID(client_order_id)) != client_order_id:
                raise ValueError("noncanonical order identity")
        except (ValueError, TypeError, AttributeError) as exc:
            raise ControlError("invalid journal client order identity") from exc
    path = private_root(store.root) / CONNECTION_FILE
    if not path.exists() and not path.is_symlink():
        return {"state": "not_configured", "execution_authority": False}
    connection = read_private_json(path)
    account = connection.get("account_key")
    if (
        set(connection) != {"schema_version", "account_key"}
        or type(connection["schema_version"]) is not int
        or connection["schema_version"] != 1
        or not isinstance(account, str)
        or not HEX_DIGEST.fullmatch(account)
    ):
        raise ControlError("invalid journal connection")
    if not isinstance(store, GolemControlStore):
        raise ControlError("journal inspection requires the Golem transport")
    address = f"helios-alpha/local/CryptoBrokerJournalAgent({json.dumps(account)})"
    action = "record" if client_order_id is not None else "status"
    if allocation:
        action = "account_state"
    if risk:
        action = "risk_state"
    payload = {"client_order_id": client_order_id} if client_order_id is not None else {}
    envelope = store._invoke_json_at(address, "command", [client, action, canonical(payload)])
    if (
        not isinstance(envelope, dict)
        or type(envelope.get("schema_version")) is not int
        or envelope["schema_version"] != 1
        or envelope.get("execution_authority") is not False
        or not isinstance(envelope.get("result"), dict)
    ):
        raise ControlError("invalid journal response")
    result = envelope["result"]
    if risk:
        _validate_risk_state(result, account)
    elif allocation:
        _validate_allocation(result, account)
    elif client_order_id is None:
        if (
            result.get("account_key") != account
            or type(result.get("record_count")) is not int
            or not 0 <= result["record_count"] <= 256
            or not isinstance(result.get("snapshot_digest"), str)
            or not HEX_DIGEST.fullmatch(result["snapshot_digest"])
        ):
            raise ControlError("journal response identity or shape mismatch")
    else:
        intent = result.get("intent")
        if (
            not isinstance(intent, dict)
            or intent.get("account_key") != account
            or intent.get("client_order_id") != client_order_id
        ):
            raise ControlError("journal order identity mismatch")
    return {
        "schema_version": 1,
        "state": "available",
        "authority": "golem",
        "execution_authority": False,
        "account_key": account,
        ("allocation_risk" if risk else ("allocation" if allocation else "journal")): result,
        "current_policy_revision": settings["policy"]["revision"],
        "helios_execution": settings["execution"],
        "semantics": {
            "inspection_only": True,
            "not_a_live_broker_read": True,
            "does_not_reconcile_broker": True,
            "existing_broker_positions_are_not_automatically_managed": True,
            "dispatch_receipts_are_not_trade_authorization": True,
        },
    }


def read_broker_journal(store, client: str, *, client_order_id: str | None = None) -> dict:
    return _inspect_broker_journal(store, client, client_order_id=client_order_id)


def read_broker_allocation(store, client: str) -> dict:
    """Inspect durable strategy accounting, without seeding or adopting account funds."""
    return _inspect_broker_journal(store, client, allocation=True)


def read_broker_allocation_risk(store, client: str) -> dict:
    """Inspect durable equity and loss history; no marks, broker reads or position adoption."""
    return _inspect_broker_journal(store, client, risk=True)


def _validate_risk_state(result: dict, account: str) -> None:
    def integer(value):
        return type(value) is int and 0 <= value < 2**64

    def digest(value):
        return isinstance(value, str) and HEX_DIGEST.fullmatch(value) is not None

    if (
        set(result)
        != {
            "execution_authority",
            "account_key",
            "evaluated_ms",
            "balance",
            "balance_digest",
            "risk",
        }
        or result["execution_authority"] is not False
        or result["account_key"] != account
        or not integer(result["evaluated_ms"])
    ):
        raise ControlError("invalid allocation risk state")
    balance = result["balance"]
    _validate_allocation({"initialized": balance is not None, "balance": balance}, account)
    if balance is None:
        if result["balance_digest"] is not None or result["risk"] is not None:
            raise ControlError("uninitialized allocation cannot have risk history")
        return
    if not digest(result["balance_digest"]):
        raise ControlError("invalid allocation balance digest")
    risk = result["risk"]
    if risk is None:
        return
    integer_fields = {
        "observed_ms",
        "valid_until_ms",
        "policy_revision",
        "day_utc",
        "equity_micros",
        "equity_deficit_micros",
        "day_open_equity_micros",
        "peak_equity_micros",
        "gross_exposure_micros",
        "daily_order_count",
        "fee_reserve_bps",
        "settlement_quantum_micros",
    }
    bool_fields = {"current", "day_loss_latched", "drawdown_latched", "deficit_latched", "paused"}
    digest_fields = {"valuation_digest", "balance_digest", "source_digest", "policy_digest"}
    if (
        not isinstance(risk, dict)
        or set(risk) != integer_fields | bool_fields | digest_fields | {"symbol_exposure_micros"}
        or any(not integer(risk[key]) for key in integer_fields)
        or any(type(risk[key]) is not bool for key in bool_fields)
        or any(not digest(risk[key]) for key in digest_fields)
        or risk["daily_order_count"] > 256
        or risk["fee_reserve_bps"] > 10_000
        or risk["settlement_quantum_micros"] == 0
        or risk["policy_revision"] == 0
        or risk["observed_ms"] > result["evaluated_ms"]
        or risk["valid_until_ms"] <= risk["observed_ms"]
        or risk["peak_equity_micros"] < max(risk["equity_micros"], risk["day_open_equity_micros"])
        or (risk["equity_micros"] > 0 and risk["equity_deficit_micros"] > 0)
    ):
        raise ControlError("invalid allocation risk history")
    exposures = risk["symbol_exposure_micros"]
    if (
        not isinstance(exposures, dict)
        or not set(exposures) <= {"BTC-USD", "ETH-USD"}
        or any(not integer(v) for v in exposures.values())
        or sum(exposures.values()) != risk["gross_exposure_micros"]
    ):
        raise ControlError("invalid allocation risk exposures")
    if risk["current"] and (
        risk["balance_digest"] != result["balance_digest"]
        or result["evaluated_ms"] >= risk["valid_until_ms"]
        or risk["day_utc"] != result["evaluated_ms"] // 86_400_000
        or balance["unresolved_orders"]
    ):
        raise ControlError("stale allocation risk cannot be current")
