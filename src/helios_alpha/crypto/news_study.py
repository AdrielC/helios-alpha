"""Read-only summaries of prospective news observations, never strategy P&L."""

from __future__ import annotations

import hashlib
import json
import sqlite3
import time
from collections import Counter

from helios_alpha.control.store import ControlError, private_root

from .evidence import (
    HEX_ID,
    HORIZONS_MS,
    MAX_CANDIDATES,
    MAX_LAG_MS,
    SYMBOLS,
    _reader,
    reference_quote,
    reference_return_bps,
)


def _provenance(candidate):
    news = candidate["news"]
    quotes = [candidate.get("reference")] + [point.get("quote") for point in candidate["markouts"]]
    flags = [candidate.get("synthetic_inputs"), news.get("synthetic")]
    if any(flag is not None and type(flag) is not bool for flag in flags):
        raise ValueError("invalid synthetic flag")
    if (
        any(flag is True for flag in flags)
        or candidate.get("reference_source") == "synthetic"
        or any(q and q.get("channel") == "synthetic" for q in quotes)
    ):
        return "synthetic"
    if (
        all(flag is False for flag in flags)
        and candidate.get("reference_source") == "coinbase"
        and all(q is None or q.get("channel") in {"ticker", "level2_batch"} for q in quotes)
    ):
        return "explicit_non_synthetic"
    return "legacy_unlabelled"


def _returns(values):
    ordered = sorted(values)
    count = len(ordered)
    return {
        "count": count,
        "mean_bps": sum(ordered) // count if count else None,
        "minimum_bps": ordered[0] if count else None,
        "maximum_bps": ordered[-1] if count else None,
        "positive": sum(value > 0 for value in values),
        "zero": values.count(0),
        "negative": sum(value < 0 for value in values),
    }


def summarize_candidates(candidates, at_ms):
    """Keep versions, assets, cost assumptions and provenance in separate cohorts."""
    groups = {}
    for candidate in candidates:
        fingerprint, symbol = candidate["strategy_fingerprint"], candidate["symbol"]
        news_id, costs = candidate["news"]["news_id"], candidate["costs"]
        if (
            not HEX_ID.fullmatch(fingerprint)
            or not HEX_ID.fullmatch(news_id)
            or symbol not in SYMBOLS
            or any(
                type(costs.get(k)) is not int or not 0 <= costs[k] <= 1000
                for k in ("fee_bps", "slippage_bps")
            )
        ):
            raise ValueError("invalid study identity or costs")
        provenance = _provenance(candidate)
        key = (fingerprint, symbol, provenance, costs["fee_bps"], costs["slippage_bps"])
        group = groups.setdefault(
            key,
            {
                "strategy_fingerprint": fingerprint,
                "symbol": symbol,
                "provenance": provenance,
                "costs": {k: costs[k] for k in ("fee_bps", "slippage_bps")},
                "candidate_count": 0,
                "news_ids": set(),
                "candidates_with_recorded_entry_proposal": 0,
                "candidates_with_recorded_paper_fill": 0,
                "candidates_without_first_milestone_tracking": 0,
                "first_recorded_evaluation_reasons": Counter(),
                "horizons": {
                    h: {
                        "horizon_ms": h,
                        "states": Counter(),
                        "gross": [],
                        "net": [],
                    }
                    for h in HORIZONS_MS
                },
            },
        )
        group["candidate_count"] += 1
        group["news_ids"].add(news_id)
        timing = candidate.get("timing", {})
        transitions = [item["decision"] for item in candidate["transitions"]]
        group["candidates_without_first_milestone_tracking"] += int("timing" not in candidate)
        proposed = timing.get("first_entry_proposal_ms") is not None or any(
            item.get("stage") == "evaluation"
            and item.get("proposed_action") == "enter"
            and item.get("selected_news_id") == news_id
            for item in transitions
        )
        filled = timing.get("first_fill_observed_ms") is not None or any(
            item.get("stage") == "commit" and item.get("outcome") == "simulated_fill"
            for item in transitions
        )
        group["candidates_with_recorded_entry_proposal"] += int(proposed)
        group["candidates_with_recorded_paper_fill"] += int(filled)
        first_evaluation = next(
            (item for item in transitions if item.get("stage") == "evaluation"), None
        )
        if not candidate.get("omitted_transitions", 0) and first_evaluation is not None:
            group["first_recorded_evaluation_reasons"][first_evaluation["reason"]] += 1
        else:
            group["first_recorded_evaluation_reasons"]["unknown"] += 1
        reference_at = candidate["reference_at_ms"]
        entry = (
            reference_quote(candidate["reference"], reference_at)
            if type(reference_at) is int and reference_at <= at_ms
            else None
        )
        for horizon, summary in group["horizons"].items():
            points = [p for p in candidate["markouts"] if p["horizon_ms"] == horizon]
            state = "invalid"
            if reference_at is None:
                state = (
                    "missing_reference"
                    if candidate["capture_status"] != "waiting_for_reference"
                    else "pending_reference"
                )
            elif entry is not None:
                due = reference_at + horizon
                if not points:
                    state = "pending" if at_ms <= due + MAX_LAG_MS else "overdue_unrecorded"
                elif len(points) == 1:
                    point = points[0]
                    recorded = point.get("recorded_ms")
                    if (
                        type(recorded) is int
                        and point.get("due_ms") == due
                        and due <= recorded <= at_ms
                    ):
                        if point["status"] == "missing":
                            state = "missing" if recorded > due + MAX_LAG_MS else "invalid"
                        elif point["status"] == "observed" and recorded <= due + MAX_LAG_MS:
                            exit_quote = reference_quote(point.get("quote"), recorded)
                            if exit_quote and exit_quote["observed_ms"] >= due:
                                net = reference_return_bps(entry, exit_quote, costs)
                                if point.get("reference_return_bps_after_costs") == net:
                                    state = "observed"
                                    summary["net"].append(net)
                                    summary["gross"].append(
                                        reference_return_bps(
                                            entry, exit_quote, {"fee_bps": 0, "slippage_bps": 0}
                                        )
                                    )
            summary["states"][state] += 1
    result = []
    for key in sorted(groups):
        group = groups[key]
        group["unique_headlines"] = len(group.pop("news_ids"))
        group["first_recorded_evaluation_reasons"] = dict(
            sorted(group["first_recorded_evaluation_reasons"].items())
        )
        horizons = []
        for h in group["horizons"].values():
            horizons.append(
                {
                    "horizon_ms": h["horizon_ms"],
                    "states": dict(sorted(h["states"].items())),
                    "gross_reference_returns": _returns(h["gross"]),
                    "after_cost_reference_returns": _returns(h["net"]),
                }
            )
        group["horizons"] = horizons
        result.append(group)
    return result


def read_news_study(store, client, *, strategy_fingerprint=None):
    status = store.status(client)
    if strategy_fingerprint is not None and (
        not isinstance(strategy_fingerprint, str) or not HEX_ID.fullmatch(strategy_fingerprint)
    ):
        raise ControlError("invalid strategy fingerprint")
    now = time.time_ns() // 1_000_000
    runtime = status.get("runtime", {})
    runtime = runtime.get("crypto-shadow", {}) if isinstance(runtime, dict) else {}
    result = {
        "schema_version": 1,
        "execution_authority": False,
        "not_strategy_pnl": True,
        "profitability_established": False,
        "as_of_ms": now,
        "state": "not_recording",
        "active_strategy_fingerprint": runtime.get("status", {}).get("strategy_fingerprint"),
        "selected_strategy_fingerprint": strategy_fingerprint,
        "selection": "All retained fresh-news observations, including rejected headlines.",
        "semantics": [
            "Returns assume buying at the first captured ask and selling at a later bid.",
            "Returns do not use strategy entry timing, exits, allocation, or broker fills.",
            "Versions, symbols, provenance and frozen cost assumptions are separate cohorts.",
            "Headlines, assets and overlapping windows are not independent trade samples.",
            "Recorded proposal/fill counts count affected candidates, not unique orders.",
            "Missing and invalid measurements never become zero returns.",
            "Legacy unlabelled inputs cannot establish explicit non-synthetic provenance.",
            "This exploratory summary does not establish causal impact or a profitable strategy.",
        ],
        "groups": [],
    }
    path = private_root(store.root) / "news-evidence.sqlite3"
    if not path.exists() and not path.is_symlink():
        return result
    try:
        with _reader(path) as db:
            rows = db.execute(
                "SELECT data FROM candidates WHERE (? IS NULL OR "
                "json_extract(data,'$.strategy_fingerprint')=?) ORDER BY id LIMIT ?",
                (strategy_fingerprint, strategy_fingerprint, MAX_CANDIDATES + 1),
            ).fetchall()
            if len(rows) > MAX_CANDIDATES:
                raise ValueError("study exceeds retained observation bound")
            candidates = [json.loads(row["data"]) for row in rows]
            last = db.execute("SELECT last_tick_ms FROM evidence_meta").fetchone()[0]
            result.update(
                state="available",
                last_observation_ms=last,
                observation_age_ms=now - last if last else None,
                candidate_count=len(candidates),
                unique_headlines=len({c["news"]["news_id"] for c in candidates}),
                evidence_sha256=hashlib.sha256(
                    "\n".join(row["data"] for row in rows).encode()
                ).hexdigest(),
                groups=summarize_candidates(candidates, now),
            )
    except (sqlite3.Error, ValueError, KeyError, TypeError, AttributeError) as exc:
        raise ControlError("news study is unavailable or invalid") from exc
    return result
