"""Value an existing strategy allocation from fresh broker reads; never submit orders."""

from __future__ import annotations

import argparse
import asyncio
import copy
import fcntl
import hashlib
import json
import os
import signal
import sys
import time
from dataclasses import dataclass
from pathlib import Path

from helios_alpha.control.broker_journal import _validate_allocation, _validate_risk_state
from helios_alpha.control.golem_store import GolemControlStore, read_private_json
from helios_alpha.control.models import RiskLimits
from helios_alpha.control.store import DEFAULT_ROOT, ControlError, canonical, private_root

from .account_reader import _decimal, read_configured_account, validate_account_response
from .order_recovery import DIGEST, GolemRecoveryJournal, RecoveryError, identity_valid, uint

CONFIG_FILE = "allocation-valuation.json"
DAY_MS = 86_400_000
ACTOR = "crypto-runtime"


def millis() -> int:
    return time.time_ns() // 1_000_000


def digest(value) -> str:
    return hashlib.sha256(canonical(value).encode()).hexdigest()


def is_digest(value) -> bool:
    return isinstance(value, str) and DIGEST.fullmatch(value) is not None


@dataclass(frozen=True)
class FeeBound:
    """Locally established worst fee, with account binding, provenance and an expiry."""

    account_key: str
    fee_reserve_bps: int
    source_digest: str
    valid_until_ms: int

    @classmethod
    def parse(cls, value: dict) -> FeeBound:
        if (
            not isinstance(value, dict)
            or set(value) != {"account_key", "fee_reserve_bps", "source_digest", "valid_until_ms"}
            or not is_digest(value["account_key"])
            or not is_digest(value["source_digest"])
            or not uint(value["fee_reserve_bps"])
            or value["fee_reserve_bps"] > 10_000
            or not uint(value["valid_until_ms"])
            or value["valid_until_ms"] == 0
        ):
            raise RecoveryError("valuation_fee_configuration")
        return cls(**value)

    def checked(self, account: str, through_ms: int) -> dict:
        if self.account_key != account or through_ms >= self.valid_until_ms:
            raise RecoveryError("valuation_fee_bound_expired_or_unbound")
        return {
            "account_key": self.account_key,
            "fee_reserve_bps": self.fee_reserve_bps,
            "source_digest": self.source_digest,
            "valid_until_ms": self.valid_until_ms,
        }


class GolemValuationJournal:
    """Use only the journal's existing reconciler role; expose no dispatch operation."""

    def __init__(self, store: GolemControlStore):
        self._transport = GolemRecoveryJournal(store)
        self.account_key = self._transport.account_key

    def state(self) -> dict:
        return self._transport._call("broker-reconciler", "risk_state")

    def prepared(self, identity: str) -> dict:
        return self._transport._call(
            "broker-reconciler", "prepared_allocation", {"client_order_id": identity}
        )

    def mark(self, valuation: dict, scope: dict | None) -> dict:
        if scope is None:
            return self._transport._call("broker-reconciler", "mark_allocation", valuation)
        return self._transport._call(
            "broker-reconciler",
            "mark_prepared_allocation",
            {"scope": scope, "valuation": valuation},
        )


async def _blocking(function, *arguments):
    # Cancellation cannot stop a CLI invocation already running in a thread. Keep the
    # worker/file lock until it finishes, rather than allowing a second cycle to overlap it.
    task = asyncio.create_task(asyncio.to_thread(function, *arguments))
    try:
        return await asyncio.shield(task)
    except asyncio.CancelledError:
        try:
            await task
        except (RecoveryError, ControlError, OSError, ValueError):
            pass
        raise


def _policy(settings: dict, now: int) -> tuple[dict, bool]:
    if (
        not isinstance(settings, dict)
        or settings.get("authority") != "golem"
        or type(settings.get("schema_version")) is not int
        or settings["schema_version"] != 1
        or not isinstance(settings.get("client"), dict)
        or settings["client"].get("id") != ACTOR
        or not isinstance(settings["client"].get("capabilities"), list)
        or not {"risk:read", "status:read"} <= set(settings["client"]["capabilities"])
        or not isinstance(settings.get("execution"), dict)
        or type(settings["execution"].get("paused")) is not bool
    ):
        raise RecoveryError("valuation_control_permissions")
    policy = settings.get("policy")
    if (
        not isinstance(policy, dict)
        or set(policy) != {"schema_version", "revision", "limits", "sha256", "updated_ns"}
        or type(policy["schema_version"]) is not int
        or policy["schema_version"] != 1
        or not uint(policy["revision"])
        or policy["revision"] == 0
        or not uint(policy["updated_ns"])
        or policy["updated_ns"] // 1_000_000 > now
    ):
        raise RecoveryError("valuation_policy")
    limits = RiskLimits.model_validate(policy["limits"]).model_dump()
    # CryptoRiskLimits hashes Rust struct field order, not sorted JSON keys.
    expected = hashlib.sha256(json.dumps(limits, separators=(",", ":")).encode()).hexdigest()
    if policy["sha256"] != expected:
        raise RecoveryError("valuation_policy_digest")
    return copy.deepcopy(policy), settings["execution"]["paused"]


def _markets(balance: dict, account: str) -> list[dict]:
    _validate_allocation({"initialized": True, "balance": balance}, account)
    owned, deficient = balance["quantities"], balance["quantity_deficits"]
    if (
        balance["unresolved_orders"]
        or balance["available_cash_micros"] != balance["cash_micros"]
        or balance["available_quantities"] != owned
        or (balance["cash_micros"] > 0 and balance["cash_deficit_micros"] > 0)
        or owned.keys() & deficient.keys()
        or len(owned) + len(deficient) > 2
    ):
        raise RecoveryError("valuation_balance_requires_reconciliation")
    quantities = {**owned, **deficient}
    if any(_decimal(quantity) <= 0 for quantity in quantities.values()):
        raise RecoveryError("valuation_quantity")
    # Existing holdings still need prices when a policy disables new buys of that symbol.
    return [{"symbol": symbol, "quantity": quantities[symbol]} for symbol in sorted(quantities)]


class ValuationWorker:
    def __init__(self, store, journal, read, fee_bound: FeeBound):
        self.store, self.journal, self.read = store, journal, read
        # Validate even when a caller constructs the dataclass directly.
        self.fee_bound = FeeBound.parse(vars(fee_bound))
        self._active = False

    async def cycle(self, *, client_order_id: str | None = None) -> dict:
        if client_order_id is not None and not identity_valid(client_order_id):
            raise RecoveryError("valuation_prepared_identity")
        if self._active:
            raise RecoveryError("valuation_cycle_in_progress")
        self._active = True
        try:
            return await self._cycle(client_order_id)
        finally:
            self._active = False

    async def _cycle(self, identity: str | None) -> dict:
        status = {"schema_version": 1, "execution_authority": False, "broker_requests": 0}
        state = await _blocking(self.journal.state)
        _validate_risk_state(state, self.journal.account_key)
        if state["balance"] is None:
            return {**status, "state": "allocation_uninitialized"}
        scope = None
        expires_ms = 2**64 - 1
        balance, balance_digest = state["balance"], state["balance_digest"]
        if identity is not None:
            if state["risk"] is None:
                return {**status, "state": "risk_history_uninitialized"}
            prepared = await _blocking(self.journal.prepared, identity)
            if (
                not isinstance(prepared, dict)
                or set(prepared)
                != {"execution_authority", "scope", "intent", "balance", "balance_digest"}
                or prepared["execution_authority"] is not False
                or not isinstance(prepared["scope"], dict)
                or set(prepared["scope"]) != {"client_order_id", "revision", "intent_digest"}
                or prepared["scope"]["client_order_id"] != identity
                or not uint(prepared["scope"]["revision"])
                or prepared["scope"]["revision"] == 0
                or not is_digest(prepared["scope"]["intent_digest"])
                or not isinstance(prepared["intent"], dict)
                or prepared["intent"].get("client_order_id") != identity
                or prepared["intent"].get("account_key") != self.journal.account_key
                or not uint(prepared["intent"].get("expires_ms"))
                or not is_digest(prepared["balance_digest"])
            ):
                raise RecoveryError("valuation_prepared_scope")
            scope = prepared["scope"]
            expires_ms = prepared["intent"]["expires_ms"]
            balance, balance_digest = prepared["balance"], prepared["balance_digest"]
        elif balance["unresolved_orders"]:
            return {**status, "state": "order_reconciliation_required"}
        requested = _markets(balance, self.journal.account_key)
        policy, paused = _policy(await _blocking(self.store.settings, ACTOR), millis())
        self.fee_bound.checked(self.journal.account_key, millis())
        if millis() >= expires_ms:
            raise RecoveryError("valuation_prepared_expired")
        started = millis()
        read_set = await self.read(markets=requested)
        if not isinstance(read_set, dict):
            raise RecoveryError("valuation_read")
        if read_set.get("state") in {"not_configured", "credentials_required"}:
            if (
                read_set.get("broker_requests") != 0
                or read_set.get("execution_authority") is not False
            ):
                raise RecoveryError("valuation_read")
            return {**status, "state": read_set["state"]}
        if read_set.get("state") != "available" or read_set.get("execution_authority") is not False:
            raise RecoveryError("valuation_read")
        latest = _policy(await _blocking(self.store.settings, ACTOR), millis())
        if latest != (policy, paused):
            raise RecoveryError("valuation_policy_changed")
        # Reuse the native reader's complete validation after the final controller read.
        # A subprocess response or cached quote cannot refresh its own source timestamps.
        now = millis()
        read_set = validate_account_response(
            {
                "schema_version": read_set.get("schema_version"),
                "availability": read_set.get("availability"),
                "markets": read_set.get("markets"),
                "get_requests": read_set.get("broker_requests"),
            },
            self.journal.account_key,
            requests=requested,
            batch=True,
            started=started,
            now=now,
        )
        account = read_set["availability"]["snapshot"]
        if account["read_started_ms"] < balance["observed_ms"]:
            raise RecoveryError("valuation_read_predates_balance")
        for symbol, quantity in balance["quantities"].items():
            holding = account["holdings"].get(symbol.removesuffix("-USD"))
            if holding is None or _decimal(holding["total_quantity"]) < _decimal(quantity):
                raise RecoveryError("valuation_holdings_require_reconciliation")
        reported_fees = [account["reported_fee_bps"]] + [
            item["snapshot"]["reported_fee_bps"] for item in read_set["markets"]
        ]
        if any(fee is None or fee > self.fee_bound.fee_reserve_bps for fee in reported_fees):
            raise RecoveryError("valuation_fee_bound_insufficient")
        quotes = [
            {
                "quote": item["snapshot"]["quote"],
                "quantity": item["snapshot"]["quantity"],
                "source_digest": item["snapshot_digest"],
            }
            for item in read_set["markets"]
        ]
        oldest = min([account["read_started_ms"], *[q["quote"]["observed_ms"] for q in quotes]])
        deadline = min(oldest + min(policy["limits"]["max_quote_age_ms"], 5000), expires_ms)
        fee_provenance = self.fee_bound.checked(self.journal.account_key, deadline)
        if now >= deadline:
            raise RecoveryError("valuation_read_expired")
        source = digest(
            {
                "schema_version": 1,
                "account_snapshot": read_set["availability"]["snapshot_digest"],
                "market_snapshots": [item["snapshot_digest"] for item in read_set["markets"]],
                "fee_bound": fee_provenance,
                "prepared_order": scope,
            }
        )
        valuation = {
            "schema_version": 1,
            "account_key": self.journal.account_key,
            "expected_balance_digest": balance_digest,
            "policy": policy,
            "observed_ms": account["read_started_ms"],
            "source_digest": source,
            "fee_reserve_bps": self.fee_bound.fee_reserve_bps,
            "paused": paused,
            "quotes": quotes,
        }
        # One CAS mutation. An uncertain reply is never retried inside this cycle.
        receipt = await _blocking(self.journal.mark, valuation, scope)
        if (
            not isinstance(receipt, dict)
            or set(receipt) != {"changed", "execution_authority", "risk"}
            or type(receipt["changed"]) is not bool
            or receipt["execution_authority"] is not False
            or not isinstance(receipt["risk"], dict)
        ):
            raise RecoveryError("valuation_receipt")
        risk = copy.deepcopy(receipt["risk"])
        if (
            risk.get("current") is not True
            or risk.get("balance_digest") != balance_digest
            or risk.get("source_digest") != source
            or risk.get("policy_revision") != policy["revision"]
            or risk.get("policy_digest") != policy["sha256"]
            or risk.get("observed_ms") != account["read_started_ms"]
            or risk.get("valid_until_ms") != deadline
            or risk.get("fee_reserve_bps") != self.fee_bound.fee_reserve_bps
            or risk.get("paused") is not paused
        ):
            raise RecoveryError("valuation_receipt_binding")
        now = millis()
        risk["current"] = now < deadline and risk.get("day_utc") == now // DAY_MS
        _validate_risk_state(
            {
                "execution_authority": False,
                "account_key": self.journal.account_key,
                "evaluated_ms": now,
                "balance": balance,
                "balance_digest": balance_digest,
                "risk": risk,
            },
            self.journal.account_key,
        )
        return {
            **status,
            "state": "marked" if risk["current"] else "mark_expired",
            "broker_requests": read_set["broker_requests"],
            "changed": receipt["changed"],
            "prepared_order": scope,
            "risk": risk,
        }


async def serve(worker: ValuationWorker, stopped: asyncio.Event, publish, *, poll_seconds: float):
    if type(poll_seconds) not in (int, float) or not 1 <= poll_seconds <= 60:
        raise RecoveryError("valuation_poll_interval")
    while not stopped.is_set():
        cycle = asyncio.create_task(worker.cycle())
        stop = asyncio.create_task(stopped.wait())
        try:
            done, _ = await asyncio.wait([cycle, stop], return_when=asyncio.FIRST_COMPLETED)
            if stop in done:
                return
            try:
                status = cycle.result()
            except (RecoveryError, ControlError, OSError, ValueError, TypeError):
                status = {"schema_version": 1, "state": "unavailable", "execution_authority": False}
            await publish(status)
        finally:
            for task in (cycle, stop):
                if not task.done():
                    task.cancel()
            await asyncio.gather(cycle, stop, return_exceptions=True)
        try:
            await asyncio.wait_for(stopped.wait(), poll_seconds)
        except TimeoutError:
            pass


async def run(root: Path, *, once: bool = False, client_order_id: str | None = None) -> dict:
    if client_order_id is not None and (not once or not identity_valid(client_order_id)):
        raise RecoveryError("valuation_prepared_requires_once")
    path = root / CONFIG_FILE
    if not path.exists() and not path.is_symlink():
        return {"state": "not_configured", "execution_authority": False, "broker_requests": 0}
    config = read_private_json(path)
    if (
        set(config) != {"schema_version", "enabled", "fee_bound", "poll_seconds"}
        or type(config["schema_version"]) is not int
        or config["schema_version"] != 1
        or type(config["enabled"]) is not bool
        or type(config["poll_seconds"]) not in (int, float)
        or not 1 <= config["poll_seconds"] <= 60
    ):
        raise RecoveryError("valuation_configuration")
    fee_bound = FeeBound.parse(config["fee_bound"])
    if not config["enabled"]:
        return {"state": "disabled", "execution_authority": False, "broker_requests": 0}
    fd = os.open(root / "allocation-valuation.lock", os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
    with os.fdopen(fd, "a+") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        store = GolemControlStore(root)

        async def read(**kwargs):
            return await read_configured_account(root, **kwargs)

        worker = ValuationWorker(store, GolemValuationJournal(store), read, fee_bound)
        if once:
            return await worker.cycle(client_order_id=client_order_id)
        stopped = asyncio.Event()
        for sig in (signal.SIGTERM, signal.SIGINT):
            asyncio.get_running_loop().add_signal_handler(sig, stopped.set)

        async def publish(status):
            try:
                await _blocking(store.heartbeat, ACTOR, "allocation-valuation", status)
            except (RecoveryError, ControlError, OSError, ValueError):
                pass

        await serve(worker, stopped, publish, poll_seconds=config["poll_seconds"])
        return {"state": "stopped", "execution_authority": False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=DEFAULT_ROOT)
    parser.add_argument("--once", action="store_true")
    parser.add_argument("--client-order-id", help="Revalue one provably unsent journal order")
    args = parser.parse_args()
    os.umask(0o077)
    try:
        result = asyncio.run(
            run(private_root(args.root), once=args.once, client_order_id=args.client_order_id)
        )
    except (RecoveryError, ControlError, OSError, ValueError, TypeError):
        print(json.dumps({"state": "unavailable", "execution_authority": False}))
        sys.exit(1)
    print(json.dumps(result, sort_keys=True, indent=2))


if __name__ == "__main__":
    main()
