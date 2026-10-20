"""Minimized, inspection-only projection of Golem's current strategy state."""

from __future__ import annotations

import re

from .store import ControlError


def identifier(value):
    if not isinstance(value, str) or not re.fullmatch(r"[A-Za-z0-9_.:-]{1,100}", value):
        raise ControlError("invalid strategy status identifier")
    return value


def number(value):
    if type(value) is not int or not 0 <= value <= 9_007_199_254_740_991:
        raise ControlError("invalid strategy status number")
    return value


def project(status, observed_ms):
    if status.get("authority") != "golem" or type(status.get("live_admitted")) is not bool:
        raise ControlError("strategy authority unavailable")
    books = []
    for name, entry in sorted(status.get("runtime", {}).items()):
        data = entry["status"]
        if not data.get("engine"):
            continue
        books.append(
            {
                "id": identifier(name),
                "engine": identifier(data["engine"]),
                "state": identifier(data["state"]),
                "mode": identifier(data["mode"]),
                "updated_ms": number(entry["updated_ns"] // 1_000_000),
                "feed_connected": data.get("feed_connected") is True,
                "active_news": number(data.get("active_news", 0)),
                "reference_source": identifier(data.get("reference_source", "unknown")),
                "reference_channel": identifier(data.get("reference_channel", "unknown")),
                "quote_age_ms": {
                    identifier(symbol): number(age)
                    for symbol, age in list(data.get("quote_age_ms", {}).items())[:100]
                },
                "feeds": [
                    {
                        "id": identifier(feed_id),
                        "state": identifier(feed["state"]),
                        "poll_seconds": number(feed.get("poll_seconds", 0)),
                        "last_ok_ms": number(feed["last_ok_ms"])
                        if feed.get("last_ok_ms") is not None
                        else None,
                        "cache_age_seconds": number(feed["response_cache_age_seconds"])
                        if feed.get("response_cache_age_seconds") is not None
                        else None,
                        "consecutive_failures": number(feed.get("consecutive_failures", 0)),
                    }
                    for feed_id, feed in sorted(data.get("feeds", {}).items())[:50]
                ],
                "signals": [
                    {"symbol": identifier(symbol), "reason": identifier(reason)}
                    for symbol, reason in sorted(data.get("last_reasons", {}).items())
                ][:100],
            }
        )
    paper = status.get("paper", {})
    budget = status.get("decision_budget", {})
    return {
        "observed_ms": number(observed_ms),
        "mode": identifier(status["mode"]),
        "paused": status.get("paused") is True,
        "live_admitted": status["live_admitted"],
        "live_blockers": [identifier(item) for item in status.get("live_blockers", [])][:20],
        "paper_orders": number(paper.get("total_orders", 0)),
        "paper_positions": number(len(paper.get("positions", {}))),
        "model_calls_reserved": number(budget.get("reserved_calls", 0)),
        "model_daily_limit": number(budget.get("daily_call_limit", 0)),
        "books": books[:20],
    }


def account_permissions(raw):
    account = raw["account"]
    level = account.get("user_option_level", account.get("option_level", ""))
    if account.get("brokerage_account_type") == "trust_revocable":
        level = account.get("user_option_level", "")
    approvals = raw["calls"]["approvals"]["data"]["setting"]
    return {
        "observed_ms": number(raw["account_received_ms"]),
        "options_level": level if level in {"option_level_2", "option_level_3"} else "none",
        "approval_required": approvals["human_must_approve_trades"],
    }
