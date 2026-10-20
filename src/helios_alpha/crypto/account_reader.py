"""Explicit, read-only account availability from a pinned native broker adapter."""

from __future__ import annotations

import argparse
import asyncio
import hashlib
import json
import re
import time
from decimal import ROUND_CEILING, Decimal
from pathlib import Path

from helios_alpha.control.golem_store import read_private_json
from helios_alpha.control.store import DEFAULT_ROOT, ControlError, canonical, private_root

from .order_recovery import DIGEST, NativeRecoveryBackend, RecoveryError, uint

CONFIG_FILE = "account-reader.json"
DECIMAL = re.compile(r"(?:0|[1-9][0-9]{0,17})(?:\.[0-9]{1,18})?\Z")
ASSET = re.compile(r"[A-Z0-9]{1,20}\Z")
SNAPSHOT_KEYS = {
    "schema_version",
    "execution_authority",
    "account_key",
    "api_account_fingerprint",
    "status",
    "api_tradable",
    "buying_power_usd",
    "reported_fee_ratio",
    "reported_fee_bps",
    "holdings",
    "read_started_ms",
    "read_completed_ms",
}


def _decimal(value) -> Decimal:
    if not isinstance(value, str) or not DECIMAL.fullmatch(value):
        raise RecoveryError("account_decimal")
    return Decimal(value)


def _units(value) -> int:
    _decimal(value)
    whole, _, fraction = value.partition(".")
    return int(whole) * 10**18 + int(fraction.ljust(18, "0"))


def _market_request(symbol, quantity) -> dict | None:
    if symbol is None and quantity is None:
        return None
    if symbol not in ("BTC-USD", "ETH-USD") or _decimal(quantity) <= 0:
        raise RecoveryError("market_request")
    return {"symbol": symbol, "quantity": quantity}


def _market_plan(symbol, quantity, markets):
    single = _market_request(symbol, quantity)
    if markets is None:
        return ({"market": single} if single else {}), ([single] if single else []), False
    if single or not isinstance(markets, list) or len(markets) > 2:
        raise RecoveryError("market_request")
    requested = []
    for item in markets:
        if not isinstance(item, dict) or set(item) != {"symbol", "quantity"}:
            raise RecoveryError("market_request")
        request = _market_request(item["symbol"], item["quantity"])
        if request is None or request["symbol"] in {r["symbol"] for r in requested}:
            raise RecoveryError("market_request")
        requested.append(request)
    return {"markets": requested}, requested, True


def _validate_market(envelope, request, account, now) -> None:
    if (
        not isinstance(envelope, dict)
        or set(envelope) != {"snapshot", "snapshot_digest"}
        or not isinstance(envelope["snapshot"], dict)
    ):
        raise RecoveryError("market_response")
    value = envelope["snapshot"]
    if (
        set(value)
        != {
            "schema_version",
            "execution_authority",
            "account_key",
            "api_account_fingerprint",
            "quantity",
            "rules",
            "quote",
            "reported_fee_bps",
            "broker_quote_observed_ms",
            "read_started_ms",
            "read_completed_ms",
        }
        or type(value["schema_version"]) is not int
        or value["schema_version"] != 1
        or value["execution_authority"] is not False
        or value["account_key"] != account["account_key"]
        or value["api_account_fingerprint"] != account["api_account_fingerprint"]
        or _decimal(value["quantity"]) != _decimal(request["quantity"])
        or not all(
            uint(value[key])
            for key in (
                "read_started_ms",
                "read_completed_ms",
                "broker_quote_observed_ms",
                "reported_fee_bps",
            )
        )
        or value["reported_fee_bps"] > 10_000
        or not account["read_completed_ms"]
        <= value["read_started_ms"]
        <= value["read_completed_ms"]
        <= now
        or value["broker_quote_observed_ms"] > value["read_completed_ms"]
        or now - min(value["read_started_ms"], value["broker_quote_observed_ms"]) >= 5000
        or hashlib.sha256(canonical(value).encode()).hexdigest() != envelope["snapshot_digest"]
    ):
        raise RecoveryError("market_identity_or_freshness")
    quote, rules = value["quote"], value["rules"]
    if (
        not isinstance(quote, dict)
        or set(quote) != {"account_key", "symbol", "bid", "ask", "observed_ms"}
        or quote["account_key"] != account["account_key"]
        or quote["symbol"] != request["symbol"]
        or not uint(quote["observed_ms"])
        or quote["observed_ms"] != min(value["read_started_ms"], value["broker_quote_observed_ms"])
        or _decimal(quote["bid"]) <= 0
        or _decimal(quote["ask"]) < _decimal(quote["bid"])
        or not isinstance(rules, dict)
        or set(rules)
        != {
            "symbol",
            "observed_ms",
            "asset_increment",
            "quote_increment",
            "max_order_quantity",
            "minimum_order_micros",
            "status",
            "api_tradable",
        }
        or rules["symbol"] != request["symbol"]
        or not uint(rules["observed_ms"])
        or rules["observed_ms"] != value["read_started_ms"]
        or rules["api_tradable"] is not True
        or rules["status"] not in ("tradable", "sell_only")
        or not uint(rules["minimum_order_micros"])
        or _units(rules["asset_increment"]) == 0
        or _units(rules["quote_increment"]) == 0
        or _units(request["quantity"]) % _units(rules["asset_increment"]) != 0
        or _decimal(rules["max_order_quantity"]) <= _decimal(request["quantity"])
    ):
        raise RecoveryError("market_quote_or_rules")


class NativeAccountBackend:
    """Pinned GET-only adapter for account data and optional quantity-specific market data."""

    def __init__(self, executable: Path, sha256: str, credentials: Path, account_key: str):
        self.process = NativeRecoveryBackend(
            executable, sha256, credentials, account_key, "observe"
        )
        self.account_key = account_key

    async def read(
        self,
        *,
        symbol: str | None = None,
        quantity: str | None = None,
        markets: list[dict] | None = None,
    ) -> dict:
        arguments, requests, batch = _market_plan(symbol, quantity, markets)
        started = time.time_ns() // 1_000_000
        result = await self.process._request(arguments)
        now = time.time_ns() // 1_000_000
        return validate_account_response(
            result, self.account_key, requests=requests, batch=batch, started=started, now=now
        )


def validate_account_response(result, account_key, *, requests, batch, started, now) -> dict:
    """Validate a complete native read, including again immediately before durable valuation."""
    if (
        not isinstance(result, dict)
        or type(result.get("schema_version")) is not int
        or result["schema_version"] != 1
        or not uint(started)
        or not uint(now)
        or started > now
    ):
        raise RecoveryError("account_response")
    availability = result.get("availability")
    if (
        set(result)
        != {"schema_version", "availability", "get_requests"}
        | ({"markets"} if batch else ({"market"} if requests else set()))
        or type(result["get_requests"]) is not int
        or not (2 + 2 * len(requests)) <= result["get_requests"] <= (8 + 5 * len(requests))
        or not isinstance(availability, dict)
        or set(availability) != {"snapshot", "snapshot_digest"}
        or not isinstance(availability["snapshot"], dict)
    ):
        raise RecoveryError("account_response")
    snapshot = availability["snapshot"]
    if (
        set(snapshot) != SNAPSHOT_KEYS
        or type(snapshot["schema_version"]) is not int
        or snapshot["schema_version"] != 1
        or snapshot["execution_authority"] is not False
        or snapshot["account_key"] != account_key
        or not isinstance(snapshot["api_account_fingerprint"], str)
        or not DIGEST.fullmatch(snapshot["api_account_fingerprint"])
        or not isinstance(snapshot["status"], str)
        or snapshot["status"] not in {"active", "sell_only", "deactivated"}
        or type(snapshot["api_tradable"]) is not bool
        or not uint(snapshot["read_started_ms"])
        or not uint(snapshot["read_completed_ms"])
        or not started <= snapshot["read_started_ms"] <= snapshot["read_completed_ms"] <= now
        or now - snapshot["read_started_ms"] >= 5000
        or hashlib.sha256(canonical(snapshot).encode()).hexdigest()
        != availability["snapshot_digest"]
    ):
        raise RecoveryError("account_identity_or_freshness")
    _decimal(snapshot["buying_power_usd"])
    ratio, bps = snapshot["reported_fee_ratio"], snapshot["reported_fee_bps"]
    if ratio is None:
        if bps is not None:
            raise RecoveryError("account_fee")
    else:
        fee = _decimal(ratio)
        if (
            fee > 1
            or not uint(bps)
            or bps != int((fee * 10_000).to_integral_value(rounding=ROUND_CEILING))
        ):
            raise RecoveryError("account_fee")
    holdings = snapshot["holdings"]
    if not isinstance(holdings, dict) or len(holdings) > 256:
        raise RecoveryError("account_holdings")
    for code, holding in holdings.items():
        if (
            not ASSET.fullmatch(code)
            or not isinstance(holding, dict)
            or set(holding) != {"total_quantity", "available_quantity"}
            or _decimal(holding["available_quantity"]) > _decimal(holding["total_quantity"])
        ):
            raise RecoveryError("account_holdings")
    if batch:
        returned = result["markets"]
        if not isinstance(returned, list) or len(returned) != len(requests):
            raise RecoveryError("market_batch_response")
    else:
        returned = [result["market"]] if requests else []
    for envelope, request in zip(returned, requests, strict=True):
        _validate_market(envelope, request, snapshot, now)
    return {
        "schema_version": 1,
        "state": "available",
        "execution_authority": False,
        "availability": availability,
        "broker_requests": result["get_requests"],
        **({"markets": returned} if batch else ({"market": returned[0]} if requests else {})),
        "semantics": {
            "inspection_only": True,
            "not_allocation_or_trade_authorization": True,
            "reported_fee_is_not_a_certified_ceiling": True,
            "does_not_prove_fill_settlement": True,
            "does_not_guarantee_execution_price": True,
        },
    }


async def read_configured_account(
    root: Path,
    *,
    symbol: str | None = None,
    quantity: str | None = None,
    markets: list[dict] | None = None,
) -> dict:
    _market_plan(symbol, quantity, markets)
    root = private_root(root)
    config_path = root / CONFIG_FILE
    unavailable = {"schema_version": 1, "execution_authority": False, "broker_requests": 0}
    if not config_path.exists() and not config_path.is_symlink():
        return {**unavailable, "state": "not_configured"}
    config = read_private_json(config_path)
    if (
        set(config) != {"schema_version", "executable", "sha256", "credentials"}
        or type(config["schema_version"]) is not int
        or config["schema_version"] != 1
        or any(not isinstance(config[key], str) for key in ("executable", "sha256", "credentials"))
    ):
        raise RecoveryError("account_configuration")
    connection = read_private_json(root / "broker-journal-connection.json")
    account = connection.get("account_key")
    if (
        set(connection) != {"schema_version", "account_key"}
        or type(connection["schema_version"]) is not int
        or connection["schema_version"] != 1
        or not isinstance(account, str)
        or not DIGEST.fullmatch(account)
    ):
        raise RecoveryError("journal_connection")
    credentials = Path(config["credentials"])
    backend = NativeAccountBackend(
        Path(config["executable"]), config["sha256"], credentials, account
    )
    if not credentials.exists() and not credentials.is_symlink():
        return {**unavailable, "state": "credentials_required"}
    return await backend.read(symbol=symbol, quantity=quantity, markets=markets)


async def read_account_availability(
    store, client: str, *, symbol: str | None = None, quantity: str | None = None
) -> dict:
    settings = store.settings(client)
    if not {"status:read", "risk:read"} <= set(settings["client"]["capabilities"]):
        raise ControlError("account inspection requires status and risk read permissions")
    try:
        return await read_configured_account(store.root, symbol=symbol, quantity=quantity)
    except (RecoveryError, ControlError, OSError, ValueError):
        return {"schema_version": 1, "state": "unavailable", "execution_authority": False}


def main() -> None:
    parser = argparse.ArgumentParser(description="Read broker account availability; no orders")
    parser.add_argument("--root", type=Path, default=DEFAULT_ROOT)
    parser.add_argument("--symbol", choices=["BTC-USD", "ETH-USD"])
    parser.add_argument("--quantity", help="Exact asset quantity for an optional broker estimate")
    args = parser.parse_args()
    try:
        result = asyncio.run(
            read_configured_account(args.root, symbol=args.symbol, quantity=args.quantity)
        )
    except (RecoveryError, ControlError, OSError, ValueError):
        # Never echo adapter errors, account identifiers or credential-bearing paths.
        result = {"schema_version": 1, "state": "unavailable", "execution_authority": False}
    print(json.dumps(result, sort_keys=True, indent=2))


if __name__ == "__main__":
    main()
