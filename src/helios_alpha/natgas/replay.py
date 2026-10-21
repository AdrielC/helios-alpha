"""Causal, single-leg long-option replay from observed bid/ask ticks, never OHLC fills.

All money is integer cents. Contract selection must be frozen in a timestamped plan.
Displayed size is a necessary condition, not a guarantee of exchange execution.
"""

from __future__ import annotations

from typing import Literal

from pydantic import BaseModel, ConfigDict, Field, model_validator


class Strict(BaseModel):
    model_config = ConfigDict(extra="forbid", strict=True, frozen=True)


class Contract(Strict):
    id: str = Field(min_length=1, max_length=100)
    symbol: Literal["UNG"]
    kind: Literal["call", "put"]
    strike_cents: int = Field(gt=0)
    multiplier: int = Field(gt=0, le=10000)
    expiry: str = Field(pattern=r"^\d{4}-\d{2}-\d{2}$")
    close_by_ms: int = Field(gt=0)
    available_ms: int = Field(ge=0)


class Quote(Strict):
    contract_id: str
    observed_ms: int = Field(ge=0)
    available_ms: int = Field(ge=0)
    bid_cents: int = Field(ge=0)
    ask_cents: int = Field(gt=0)
    bid_size: int = Field(ge=0)
    ask_size: int = Field(ge=0)
    regular_session: bool

    @model_validator(mode="after")
    def valid(self):
        if self.bid_cents > self.ask_cents or self.observed_ms > self.available_ms:
            raise ValueError("crossed quote or reversed clocks")
        return self


class Plan(Strict):
    id: str
    event_id: str
    evidence_ids: list[str] = Field(min_length=1)
    contract_id: str
    event_available_ms: int = Field(ge=0)
    available_ms: int = Field(ge=0)

    @model_validator(mode="after")
    def valid(self):
        if self.event_available_ms > self.available_ms:
            raise ValueError("plan precedes evidence")
        return self


class ReplayPolicy(Strict):
    capital_cents: int = Field(gt=0, le=10000000)
    max_premium_cents: int = Field(gt=0, le=10000000)
    fee_per_contract_cents: int = Field(ge=0, le=10000)
    slippage_per_share_cents: int = Field(ge=0, le=100)
    entry_latency_ms: int = Field(ge=1, le=60000)
    max_quote_age_ms: int = Field(ge=1, le=60000)
    entry_window_ms: int = Field(gt=0, le=3600000)
    max_spread_bps: int = Field(gt=0, le=20000)
    holding_ms: int = Field(gt=0, le=7 * 86400000)
    take_profit_bps: int = Field(gt=0, le=100000)
    stop_loss_bps: int = Field(gt=0, le=10000)
    expiry_buffer_ms: int = Field(ge=60000, le=86400000)


class ReplayDataset(Strict):
    data_class: Literal["recorded", "synthetic"]
    provenance: str = Field(min_length=1, max_length=4000)
    contracts: list[Contract] = Field(max_length=10000)
    quotes: list[Quote] = Field(max_length=1000000)
    plans: list[Plan] = Field(max_length=100000)


def replay(contracts: list[Contract], quotes: list[Quote], plans: list[Plan], p: ReplayPolicy):
    if len({c.id for c in contracts}) != len(contracts) or len({x.id for x in plans}) != len(plans):
        raise ValueError("duplicate contract or plan identity")
    universe = {c.id: c for c in contracts}
    streams = {c.id: [] for c in contracts}
    seen = {}
    for q in sorted(quotes, key=lambda q: (q.available_ms, q.observed_ms, q.contract_id)):
        if q.contract_id not in universe:
            raise ValueError("quote without contract metadata")
        key = (q.contract_id, q.observed_ms)
        payload = q.model_dump(exclude={"available_ms"})
        if key in seen:
            if seen[key] != payload:
                raise ValueError("ambiguous quote revision")
            continue
        seen[key] = payload
        streams[q.contract_id].append(q)
    cash, peak, drawdown, locked_until = p.capital_cents, p.capital_cents, 0, -1
    trades, rejected, used_events = [], [], set()

    def mark(value):
        nonlocal peak, drawdown
        peak = max(peak, value)
        drawdown = max(drawdown, peak - value)

    def quality(q):
        return q.regular_session and q.available_ms - q.observed_ms <= p.max_quote_age_ms

    for plan in sorted(plans, key=lambda x: (x.available_ms, x.id)):
        c = universe.get(plan.contract_id)
        reason = None
        if plan.event_id in used_events:
            reason = "duplicate_event"
        elif plan.available_ms <= locked_until:
            reason = "position_or_entry_already_pending"
        elif c is None or c.available_ms > plan.available_ms:
            reason = "contract_not_known_at_decision"
        if reason:
            rejected.append({"plan_id": plan.id, "reason": reason})
            continue
        used_events.add(plan.event_id)
        start = plan.available_ms + p.entry_latency_ms
        end = min(plan.available_ms + p.entry_window_ms, c.close_by_ms - p.expiry_buffer_ms)
        entry = None
        for q in streams[c.id]:
            if (
                q.observed_ms < start
                or q.available_ms > end
                or not quality(q)
                or q.ask_size < 1
                or q.bid_size < 1
            ):
                continue
            if 2 * (q.ask_cents - q.bid_cents) * 10000 > p.max_spread_bps * (
                q.ask_cents + q.bid_cents
            ):
                continue
            cost = (
                q.ask_cents + p.slippage_per_share_cents
            ) * c.multiplier + p.fee_per_contract_cents
            if cost <= min(cash, p.max_premium_cents):
                entry = q
                break
        if entry is None:
            locked_until = max(locked_until, end)
            rejected.append({"plan_id": plan.id, "reason": "no_eligible_entry_quote"})
            continue
        cash -= cost
        mark(
            cash
            + max(0, entry.bid_cents - p.slippage_per_share_cents) * c.multiplier
            - p.fee_per_contract_cents
        )
        deadline = min(entry.available_ms + p.holding_ms, c.close_by_ms - p.expiry_buffer_ms)
        exit_quote, trigger, exit_reason = None, None, None
        last_mark = entry.bid_cents * c.multiplier
        for q in streams[c.id]:
            if (
                q.observed_ms <= entry.observed_ms
                or q.available_ms <= entry.available_ms
                or q.available_ms >= c.close_by_ms
                or not quality(q)
                or q.bid_size < 1
            ):
                continue
            last_mark = max(0, q.bid_cents - p.slippage_per_share_cents) * c.multiplier
            proceeds = last_mark - p.fee_per_contract_cents
            mark(cash + proceeds)
            # A time exit is scheduled beforehand; trigger exits require a later tick.
            if trigger is None and q.available_ms >= deadline:
                trigger, exit_reason = deadline, "time_or_expiry_buffer"
            if trigger is None:
                gain_bps = (proceeds - cost) * 10000 / cost
                if gain_bps >= p.take_profit_bps or gain_bps <= -p.stop_loss_bps:
                    trigger = q.available_ms
                    exit_reason = "take_profit" if gain_bps >= p.take_profit_bps else "stop_loss"
            if trigger is not None and q.observed_ms >= trigger + p.entry_latency_ms:
                exit_quote = q
                break
        trade = {
            "plan_id": plan.id,
            "event_id": plan.event_id,
            "contract_id": c.id,
            "entry_ms": entry.available_ms,
            "entry_cost_cents": cost,
            "contracts": 1,
            "exit_reason": exit_reason,
            "pnl_cents": None,
            "exit_ms": None,
            "status": "unresolved",
        }
        if exit_quote:
            cash += last_mark - p.fee_per_contract_cents
            trade.update(
                pnl_cents=last_mark - p.fee_per_contract_cents - cost,
                exit_ms=exit_quote.available_ms,
                status="closed",
            )
            locked_until = exit_quote.available_ms
            mark(cash)
        else:
            # Missing exits are not dropped, valued at midpoint, or assumed to expire worthless.
            # Reserve remaining capital and exclude profitability claims for incomplete replay.
            locked_until = 2**63 - 1
            trade["unresolved_premium_cents"] = cost
        trades.append(trade)
    complete = all(t["status"] == "closed" for t in trades)
    pnl = cash - p.capital_cents if complete else None
    return {
        "schema_version": 1,
        "execution_authority": False,
        "complete": complete,
        "trades": trades,
        "rejections": rejected,
        "cash_cents": cash,
        "pnl_cents": pnl,
        "max_observed_drawdown_cents": drawdown,
        "closed_trades": sum(t["status"] == "closed" for t in trades),
        "unresolved_trades": sum(t["status"] != "closed" for t in trades),
        "profitability_established": False,
        "fill_model": "one contract, next observed ask/bid plus adverse slippage and fees",
        "limitation": "Sampled displayed quotes do not guarantee fills or capture gap drawdown.",
    }


def purged_folds(events: list[dict], *, warmup=20, test_size=10, embargo_ms=86400000):
    """Expanding chronological folds; same-event samples stay together, overlap is purged."""
    if warmup < 1 or test_size < 1 or embargo_ms < 0:
        raise ValueError("invalid fold settings")
    groups = {}
    for e in events:
        if e["end_ms"] < e["start_ms"]:
            raise ValueError("reversed event window")
        group = groups.setdefault(e["event_id"], {"start": e["start_ms"], "end": e["end_ms"]})
        group["start"] = min(group["start"], e["start_ms"])
        group["end"] = max(group["end"], e["end_ms"])
    ordered = sorted(groups, key=lambda key: (groups[key]["start"], key))
    # Equal-time events cannot enter opposite sides of a fold.
    result = []
    offset = warmup
    while offset < len(ordered):
        end = min(len(ordered), offset + test_size)
        while (
            end < len(ordered)
            and groups[ordered[end]]["start"] == groups[ordered[end - 1]]["start"]
        ):
            end += 1
        boundary = groups[ordered[offset]]["start"] - embargo_ms
        train = [key for key in ordered[:offset] if groups[key]["end"] < boundary]
        result.append(
            {
                "train": train,
                "test": ordered[offset:end],
                "purged": [key for key in ordered[:offset] if key not in train],
            }
        )
        offset = end
    return result


def expiration_scenarios(contract: Contract, ask_cents: int, underlying_cents: int, moves_bps):
    """Payoff scenarios at expiration, explicitly not forecasts or early-exit prices."""
    if ask_cents <= 0 or underlying_cents <= 0:
        raise ValueError("positive prices required")
    premium = ask_cents * contract.multiplier
    rows = []
    for move in moves_bps:
        spot = max(0, underlying_cents * (10000 + move) // 10000)
        intrinsic = max(
            0,
            spot - contract.strike_cents
            if contract.kind == "call"
            else contract.strike_cents - spot,
        )
        pnl = intrinsic * contract.multiplier - premium
        rows.append(
            {
                "move_bps": move,
                "underlying_cents": spot,
                "pnl_before_fees_cents": pnl,
                "return_bps": pnl * 10000 // premium,
            }
        )
    return rows
