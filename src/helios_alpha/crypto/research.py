"""Chronological, cost-stressed research using a private strategy and Rust replay."""

from __future__ import annotations

import argparse
import asyncio
import hashlib
import json
import os
import subprocess
import time
from pathlib import Path

import httpx

from helios_alpha.control.golem_store import GolemControlStore, read_private_json
from helios_alpha.control.store import DEFAULT_ROOT, canonical, private_root

from .market import HOUR, candles
from .strategy import PrivateStrategy


def simulate(executable: Path, symbol: str, bars: list, targets: list, limits: dict, fee: int):
    wire = dict(
        symbol=symbol,
        bars=bars,
        targets=targets,
        limits=limits,
        initial_cash_micros=limits["capital_micros"],
        fee_bps=fee,
        friction_bps=10,
        minimum_order_micros=5_000_000,
    )
    completed = subprocess.run(
        [str(executable)],
        input=canonical(wire),
        capture_output=True,
        text=True,
        timeout=60,
        check=False,
    )
    if completed.returncode:
        raise ValueError("Rust replay rejected research input")
    report = json.loads(completed.stdout)
    return {
        k: report[k]
        for k in (
            "simulation",
            "initial_cash_micros",
            "final_equity_micros",
            "net_pnl_micros",
            "max_drawdown_micros",
            "fees_paid_micros",
            "turnover_micros",
        )
    } | {"fill_count": len(report["fills"])}


async def research(root: Path, executable: Path, days: int):
    if not 30 <= days <= 365:
        raise ValueError("research horizon must be 30..365 days")
    os.umask(0o077)
    root = private_root(root)
    runtime = read_private_json(root / "runtime.json")
    if runtime.get("engine") == "news_shock":
        raise ValueError(
            "Hourly candles cannot reconstruct news arrival or subminute price confirmation. "
            "Use helios-control decision-snapshots and replay-decision for saved proposals; "
            "those replays are not a profitability backtest."
        )
    out = private_root(root / "research", create=True)
    strategy = PrivateStrategy(
        Path(runtime["strategy_path"]), Path(runtime["strategy_config_path"])
    )
    limits = GolemControlStore(root).policy("crypto-runtime")["limits"]
    end = int(time.time()) // HOUR * HOUR
    start = end - (days * 24 + 240) * HOUR
    results = []
    async with httpx.AsyncClient(timeout=15, follow_redirects=False, trust_env=False) as client:
        for symbol in runtime["symbols"]:
            cache = out / f"{symbol}-{start}-{end}.json"
            if cache.exists():
                bars = json.loads(cache.read_bytes())
            else:
                bars = []
                for at in range(start, end, 300 * HOUR):
                    bars.extend(await candles(client, symbol, at, min(end, at + 300 * HOUR)))
                    await asyncio.sleep(0.35)
                cache.write_text(canonical(bars))
            # Strategy sees only complete prefix bars; trades happen after availability.
            targets = []
            for i in range(239, len(bars)):
                available_ms = bars[i]["start_ms"] + HOUR * 1000
                decision = strategy.decide(
                    {
                        "symbol": symbol,
                        "bars": bars[i - 239 : i + 1],
                        "now_ms": available_ms,
                        "paper": {"positions": {}},
                    }
                )
                targets.append(
                    dict(available_ms=available_ms, allocation_bps=decision["allocation_bps"])
                )
            # Equal chronological partitions. Nothing is selected or optimized using the holdout.
            actual = bars[240:]
            split = len(actual) * 2 // 3
            for name, sample in (
                ("earlier_two_thirds", actual[:split]),
                ("later_holdout", actual[split:]),
            ):
                lower = sample[0]["start_ms"]
                upper = sample[-1]["start_ms"] + HOUR * 1000
                selected = [t for t in targets if lower <= t["available_ms"] < upper]
                variants = {
                    "private_candidate": selected,
                    "cash": [],
                    # Same risk limits, with periodic rebalancing to a constant dollar allocation.
                    "constant_allocation": [{"available_ms": lower - 1, "allocation_bps": 5000}],
                }
                for fee in sorted({20, runtime["paper"]["fee_bps"], 100}):
                    for variant, signals in variants.items():
                        result = simulate(executable, symbol, sample, signals, limits, fee)
                        results.append(
                            dict(
                                symbol=symbol,
                                period=name,
                                variant=variant,
                                fee_bps_per_side=fee,
                                friction_bps_per_side=10,
                                start_ms=lower,
                                end_ms=upper,
                                **result,
                            )
                        )
            print(json.dumps({"symbol": symbol, "bars": len(bars), "completed": True}), flush=True)
    report = dict(
        schema_version=1,
        simulation=True,
        generated_ms=time.time_ns() // 1_000_000,
        strategy_fingerprint=strategy.fingerprint,
        status="research_only_not_live_admission",
        results=results,
        limitations=[
            "Coinbase candles are not Robinhood fills",
            "Fees and friction are scenarios, not account fee quotes",
            "Each asset is simulated separately, not a portfolio performance claim",
            "No order-book capacity, intrabar stop execution or market impact proof",
            "No retrospective external-news evidence; event flags use only price history",
            "Single chronological holdout, no proof of future profitability",
        ],
    )
    path = out / "latest.json"
    path.write_text(canonical(report) + "\n")
    (out / "latest.sha256").write_text(hashlib.sha256(path.read_bytes()).hexdigest() + "\n")
    print(json.dumps({"report": str(path), "cases": len(results), "paid_api_calls": 0}))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=DEFAULT_ROOT)
    parser.add_argument("--replay-bin", type=Path, required=True)
    parser.add_argument("--days", type=int, default=90)
    args = parser.parse_args()
    asyncio.run(research(args.root, args.replay_bin.absolute(), args.days))


if __name__ == "__main__":
    main()
