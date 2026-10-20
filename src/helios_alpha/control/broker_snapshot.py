"""Private, read-only observations imported from the connected broker tools."""

from __future__ import annotations

import fcntl
import hashlib
import os
import re
import tempfile
import time
from datetime import datetime
from decimal import Decimal, InvalidOperation, localcontext

from .golem_store import read_private_json
from .store import ControlError, canonical, private_root

FILENAME = "broker-observation.json"
MAX_AGE_MS = 60_000
QUOTE_MAX_AGE_MS = 5000
AMOUNT = re.compile(r"-?[0-9]{1,18}(?:\.[0-9]{1,18})?\Z")
SYMBOL = re.compile(r"[A-Z0-9]{1,12}\Z")


def _amount(value, *, nonnegative=False):
    if not isinstance(value, str) or not AMOUNT.fullmatch(value):
        raise ControlError("invalid broker amount")
    if nonnegative and Decimal(value) < 0:
        raise ControlError("negative broker quantity")
    # Keep original decimal precision. BTC quantities must not be rounded to
    # the USD-millionths representation used by the risk policy.
    return value


def _timestamp(value):
    if not isinstance(value, str):
        raise ControlError("broker timestamp missing")
    try:
        stamp = datetime.fromisoformat(value)
        if stamp.tzinfo is None:
            raise ValueError("timezone missing")
        return int(stamp.timestamp() * 1000)
    except (ValueError, OverflowError) as error:
        raise ControlError("invalid broker timestamp") from error


def _mask(value):
    if not isinstance(value, str) or not re.fullmatch(r"[A-Za-z0-9]{4,40}", value):
        raise ControlError("invalid broker account identity")
    return "••••" + value[-4:]


def _rows(data, limit):
    rows = data["results"]
    if rows is None:
        return []
    if not isinstance(rows, list) or len(rows) > limit:
        raise ControlError("invalid or oversized broker collection")
    return rows


def normalize_observation(raw, at_ms):
    """Validate imported tool responses. This does not authenticate a broker session."""
    try:
        with localcontext() as context:
            context.prec = 64
            return _normalize_observation(raw, at_ms)
    except (KeyError, TypeError, AttributeError, ValueError, InvalidOperation) as error:
        raise ControlError("invalid broker observation structure") from error


def _normalize_observation(raw, at_ms):
    if (
        type(raw.get("schema_version")) is not int or raw["schema_version"] != 1
        or type(at_ms) is not int or at_ms < 0
    ):
        raise ControlError("invalid broker observation")
    account = raw["account"]
    if account.get("agentic_allowed") is not True:
        raise ControlError("account is not accessible to the connected agent")
    if (
        not isinstance(account.get("state"), str)
        or not 1 <= len(account["state"]) <= 40
        or type(account.get("deactivated")) is not bool
        or type(account.get("permanently_deactivated")) is not bool
    ):
        raise ControlError("invalid broker account state")
    brokerage, crypto = account["account_number"], account["rhc_account_number"]
    numeric = account["rhs_account_number"]
    account_labels = {
        "brokerage": _mask(brokerage), "crypto": _mask(crypto),
        "numeric_brokerage": _mask(numeric),
    }
    account_received = raw["account_received_ms"]
    if type(account_received) is not int or not 0 <= account_received <= at_ms:
        raise ControlError("invalid broker account clock")
    received = {"account": account_received}

    def call(name, requested):
        item = raw["calls"][name]
        stamp = item["received_ms"]
        if (
            item["requested_account"] != requested
            or type(stamp) is not int
            or not 0 <= stamp <= at_ms
            or not isinstance(item["data"], dict)
        ):
            raise ControlError("broker observation account or clock mismatch")
        received[name] = stamp
        return item["data"]

    portfolio = call("portfolio", brokerage)
    if portfolio["currency"] != "USD":
        raise ControlError("broker account currency is unsupported")
    power = portfolio.get("crypto_buying_power")
    portfolio_view = {
        key: _amount(portfolio[key])
        for key in ("total_value", "cash", "crypto_value")
    }
    portfolio_view["currency"] = "USD"
    portfolio_view["crypto_buying_power"] = (
        _amount(power["buying_power"]) if isinstance(power, dict) else None
    )
    setting = call("approvals", brokerage)["setting"]
    if (
        setting["account_number"] != brokerage
        or type(setting["human_must_approve_trades"]) is not bool
    ):
        raise ControlError("approval setting belongs to a different account")
    changed = setting.get("updated_at")
    if changed and _timestamp(changed) > received["approvals"]:
        raise ControlError("approval setting has a future timestamp")
    positions_data = call("positions", numeric)
    orders_data = call("open_orders", numeric)
    if (
        orders_data["rhs_account_number"] != numeric
        or orders_data["crypto_account_number"] != crypto
        or positions_data.get("next")
        or orders_data.get("next")
    ):
        raise ControlError("incomplete or mismatched broker account data")
    positions, orders = _rows(positions_data, 100), _rows(orders_data, 100)
    position_view = []
    account_ids = set()
    seen_assets = set()
    for position in positions:
        asset = position["currency"]["code"]
        if not SYMBOL.fullmatch(asset) or asset in seen_assets:
            raise ControlError("invalid or repeated broker asset")
        seen_assets.add(asset)
        quantity = _amount(position["quantity"], nonnegative=True)
        sellable = _amount(position["quantity_transferable"], nonnegative=True)
        reserved = _amount(position["quantity_held_for_sell"], nonnegative=True)
        if Decimal(sellable) + Decimal(reserved) > Decimal(quantity):
            raise ControlError("inconsistent broker quantity")
        account_ids.add(position["account_id"])
        position_view.append({
            "asset": asset, "quantity": quantity, "sellable_quantity": sellable,
            "quantity_in_open_sells": reserved,
            "managed_by_helios": False,
        })
        # Direct and intraday bases overlap; never add them together. Only use a
        # complete, unambiguous direct basis for the entire reported holding.
        bases = position.get("cost_bases", [])
        if isinstance(bases, list) and len(bases) == 1:
            basis = bases[0]
            direct_quantity = _amount(basis["direct_quantity"], nonnegative=True)
            direct_cost = _amount(basis["direct_cost_basis"], nonnegative=True)
            if Decimal(direct_quantity) == Decimal(quantity) and Decimal(direct_cost) > 0:
                position_view[-1]["cost_basis"] = direct_cost
    order_view = []
    for order in orders:
        if order["state_group"] != "open":
            raise ControlError("closed order in open-order observation")
        account_ids.add(order["account_id"])
        order_view.append({
            "order_id": order["id"], "asset": order["currency_code"],
            "side": order["side"], "state": order["state"],
        })
    quotes_data = _rows(call("quotes", numeric), 100)
    quotes = []
    for quote in quotes_data:
        bid = _amount(quote["bid_price"], nonnegative=True)
        ask = _amount(quote["ask_price"], nonnegative=True)
        quote_ms = _timestamp(quote["updated_at"])
        if quote_ms > received["quotes"] + 1000:
            raise ControlError("broker quote has a future timestamp")
        quotes.append({
            "symbol": quote["symbol"], "bid": bid, "ask": ask,
            "quoted_ms": quote_ms, "routing": quote.get("routing"),
            "book_valid": Decimal(bid) > 0 and Decimal(ask) >= Decimal(bid),
        })
        if quote.get("mark_price") is not None:
            quotes[-1]["mark"] = _amount(quote["mark_price"], nonnegative=True)
    preview_rows = raw.get("previews", [])
    if not isinstance(preview_rows, list) or len(preview_rows) > 8:
        raise ControlError("invalid broker preview collection")
    previews = []
    for index, preview in enumerate(preview_rows):
        request, data = preview["request"], preview["data"]
        stamp = preview["received_ms"]
        if (
            request["rhs_account_number"] != numeric
            or data["crypto_account_number"] != crypto
            or type(stamp) is not int or not 0 <= stamp <= at_ms
        ):
            raise ControlError("broker preview account or clock mismatch")
        order = data["order"]
        asset = request["symbol"].removesuffix("-USD")
        if (
            request["side"] != "buy"
            or order["side"] != "buy"
            or order["currency_code"] != asset
            or order["type"] != request["type"]
            or Decimal(_amount(order["cumulative_quantity"], nonnegative=True)) != 0
            or type(data["customer_approval_required"]) is not bool
        ):
            raise ControlError("invalid buy preview observation")
        account_ids.add(order["account_id"])
        debit = _amount(order["rounded_estimated_notional_with_estimated_fee"], nonnegative=True)
        fee = _amount(data["estimated_fee"], nonnegative=True)
        requested = _amount(request["dollar_amount"], nonnegative=True)
        if Decimal(debit) > Decimal(requested) or Decimal(fee) > Decimal(debit):
            raise ControlError("preview exceeds requested debit")
        received[f"preview_{index}"] = stamp
        previews.append({
            "asset": asset, "requested_debit": requested, "estimated_debit": debit,
            "estimated_fee": fee, "fee_rate": data["fee_rate"],
            "estimated_quantity": _amount(order["quantity"], nonnegative=True),
            "customer_approval_required": data["customer_approval_required"],
            "routing": order.get("routing"), "received_ms": stamp,
            # A preview may say state=new/state_group=open and carry an ID.
            # It is never added to the actual account's open orders.
            "simulation_only": True, "is_live_order": False,
        })
    if len(account_ids) > 1:
        raise ControlError("broker positions, orders and previews disagree on account identity")
    return {
        "schema_version": 1, "provider": "robinhood",
        "source": "operator_import_of_connected_mcp_responses",
        "execution_authority": False,
        "account_key": hashlib.sha256(
            f"robinhood\n{brokerage}\n{numeric}\n{crypto}".encode()
        ).hexdigest(),
        "account": {
            **account_labels, "state": account["state"],
            "deactivated": account["deactivated"],
            "permanently_deactivated": account["permanently_deactivated"],
        },
        "observed_ms": min(received.values()),
        "received_ms": received, "imported_ms": at_ms,
        "portfolio": portfolio_view,
        "approvals": {
            "human_must_approve_trades": setting["human_must_approve_trades"],
            "last_changed_at": changed,
        },
        "positions": position_view, "open_orders": order_view,
        "quotes": quotes, "previews": previews,
    }


def import_broker_observation(store, client, raw, *, at_ms=None):
    settings = store.settings(client)
    if "risk:write" not in settings["client"]["capabilities"]:
        raise ControlError("broker observation import requires operator permission")
    stamp = time.time_ns() // 1_000_000 if at_ms is None else at_ms
    snapshot = normalize_observation(raw, stamp)
    root = private_root(store.root)
    lock_fd = os.open(
        root / "broker-observation.lock", os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600
    )
    with os.fdopen(lock_fd, "a+") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        settings = store.settings(client)
        if "risk:write" not in settings["client"]["capabilities"]:
            raise ControlError("broker observation import requires operator permission")
        _save_observation(root, snapshot)
    return {"imported": True, "execution_authority": False, "account": snapshot["account"]}


def _load_observation(path):
    snapshot = read_private_json(path)
    digest = snapshot.pop("sha256", None)
    if (
        type(snapshot.get("schema_version")) is not int
        or snapshot["schema_version"] != 1
        or snapshot.get("execution_authority") is not False
        or hashlib.sha256(canonical(snapshot).encode()).hexdigest() != digest
    ):
        raise ControlError("broker observation integrity check failed")
    return snapshot


def _save_observation(root, snapshot):
    path = root / FILENAME
    if path.exists() or path.is_symlink():
        previous = _load_observation(path)
        if previous["account_key"] != snapshot["account_key"]:
            raise ControlError("broker observation belongs to a different account")
        if any(
            snapshot["received_ms"][name] < previous["received_ms"][name]
            for name in ("account", "portfolio", "approvals", "positions", "open_orders", "quotes")
        ):
            raise ControlError("broker observation would move backwards in time")
    envelope = {
        **snapshot, "sha256": hashlib.sha256(canonical(snapshot).encode()).hexdigest(),
    }
    wire = canonical(envelope)
    if len(wire.encode()) > 1024 * 1024:
        raise ControlError("broker observation exceeds storage bound")
    fd, temporary = tempfile.mkstemp(prefix=".broker-observation-", dir=root)
    try:
        with os.fdopen(fd, "w") as handle:
            handle.write(wire)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary, path)
        directory = os.open(root, os.O_RDONLY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def read_broker_observation(store, client, *, at_ms=None):
    settings = store.settings(client)  # Rechecks status:read and risk:read every time.
    stamp = time.time_ns() // 1_000_000 if at_ms is None else at_ms
    if type(stamp) is not int or stamp < 0:
        raise ControlError("invalid broker observation read clock")
    path = private_root(store.root) / FILENAME
    if not path.exists() and not path.is_symlink():
        return {"state": "unavailable", "execution_authority": False}
    snapshot = _load_observation(path)
    age = stamp - snapshot["observed_ms"]
    fresh = (
        0 <= age <= MAX_AGE_MS
        and max(snapshot["received_ms"].values()) <= stamp
        and snapshot["imported_ms"] <= stamp
    )
    for quote in snapshot["quotes"]:
        quote["fresh"] = 0 <= stamp - quote["quoted_ms"] <= QUOTE_MAX_AGE_MS
    power = snapshot["portfolio"]["crypto_buying_power"]
    for preview in snapshot["previews"]:
        limit = Decimal(str(settings["policy"]["limits"]["order_micros"]) + "e-6")
        preview["sizing_checks"] = {
            "scope": "order_size_only; not execution authorization or a full risk check",
            "observation_fresh": fresh,
            "within_current_order_limit": Decimal(preview["estimated_debit"]) <= limit,
            "within_reported_crypto_buying_power": (
                Decimal(preview["estimated_debit"]) <= Decimal(power) if power is not None else None
            ),
        }
    return {
        **snapshot, "state": "available" if fresh else "stale",
        "observation_age_ms": age, "read_at_ms": stamp,
        "current_policy_revision": settings["policy"]["revision"],
        "helios_execution": settings["execution"],
        "semantics": {
            "not_a_live_connection": True,
            "requires_live_revalidation": True,
            "preview_orders_are_not_open_orders": True,
            "real_positions_are_not_paper_inventory": True,
            "amounts": (
                "exact decimal strings; USD values and asset quantities have different units"
            ),
        },
    }
