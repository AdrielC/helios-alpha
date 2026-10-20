"""Private forward observations. Never an order authority or a retrospective backtest."""

from __future__ import annotations

import hashlib
import json
import os
import re
import sqlite3
import stat
import time
from contextlib import contextmanager

from helios_alpha.control.store import ControlError, canonical, private_root

from .strategy import MAX_NEWS_EVIDENCE

HORIZONS_MS = (30_000, 60_000, 300_000, 900_000)
MAX_LAG_MS = 5000
MAX_CANDIDATES = 5000
MAX_TRANSITIONS = 64
RETENTION_MS = 30 * 86_400_000
SYMBOLS = {"BTC-USD", "ETH-USD"}
HEX_ID = re.compile(r"[0-9a-f]{64}\Z")
NEWS_FIELDS = (
    "news_id",
    "source",
    "source_category",
    "title",
    "url",
    "published_ms",
    "observed_ms",
    "eligible",
    "intake_reason",
    "synthetic",
    "source_facts",
)


def _check_file(path):
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid() or info.st_mode & 0o077:
        raise ControlError("evidence must be a private, user-owned regular file")


def _identity(news_id, symbol, fingerprint):
    if not HEX_ID.fullmatch(news_id) or not HEX_ID.fullmatch(fingerprint) or symbol not in SYMBOLS:
        raise ValueError("invalid evidence identity")
    return hashlib.sha256(f"{news_id}\n{symbol}\n{fingerprint}".encode()).hexdigest()


def reference_quote(value, at_ms):
    if not isinstance(value, dict):
        return None
    fields = ("bid_micros", "ask_micros", "observed_ms", "received_ms")
    if any(type(value.get(key)) is not int for key in fields):
        return None
    if (
        not 0 < value["bid_micros"] <= value["ask_micros"] <= 2**64 - 1
        or not 0 <= value["observed_ms"] <= value["received_ms"] <= at_ms
        or at_ms - value["observed_ms"] > MAX_LAG_MS
    ):
        return None
    quote = {key: value[key] for key in fields}
    if value.get("channel") in {"ticker", "level2_batch", "synthetic"}:
        quote["channel"] = value["channel"]
    return quote


def reference_return_bps(entry, exit_quote, costs):
    """Hypothetical buy at ask, sell at bid, with frozen fees/slippage on both sides."""
    fee, slip = costs["fee_bps"], costs["slippage_bps"]
    buy = entry["ask_micros"] * (10_000 + fee) * (10_000 + slip)
    sell = exit_quote["bid_micros"] * (10_000 - fee) * (10_000 - slip)
    return (sell - buy) * 10_000 // buy


def reaction_latency(candidate):
    """Measured lifecycle intervals; absent historical milestones remain unknown."""
    news = candidate["news"]
    timing = candidate.get("timing", {})

    def elapsed(start, end):
        if type(start) is int and type(end) is int and 0 <= start <= end:
            return end - start
        return None

    return {
        "publication_to_intake": elapsed(news.get("published_ms"), news.get("observed_ms")),
        "intake_to_first_evaluation": elapsed(
            news.get("observed_ms"), timing.get("first_evaluation_ms")
        ),
        "intake_to_first_entry_proposal": elapsed(
            news.get("observed_ms"), timing.get("first_entry_proposal_ms")
        ),
        "review_pending_to_result": elapsed(
            timing.get("first_review_started_ms"), timing.get("first_review_finished_ms")
        ),
        "intake_to_first_submission_intent": elapsed(
            news.get("observed_ms"), timing.get("first_submission_intent_ms")
        ),
        "intake_to_first_fill_observed": elapsed(
            news.get("observed_ms"), timing.get("first_fill_observed_ms")
        ),
    }


def delay_summary(values):
    """Nearest-rank percentiles of measured delays; an empty sample is unknown."""
    ordered = sorted(values)
    count = len(ordered)
    return {
        "sample_count": count,
        "mean": sum(ordered) // count if count else None,
        "p50": ordered[(count * 50 + 99) // 100 - 1] if count else None,
        "p95": ordered[(count * 95 + 99) // 100 - 1] if count else None,
        "maximum": ordered[-1] if count else None,
    }


class NewsEvidence:
    """Bounded, restartable reference measurements and decision transitions."""

    def __init__(self, root):
        root = private_root(root)
        path = root / "news-evidence.sqlite3"
        if not path.exists() and not path.is_symlink():
            fd = os.open(path, os.O_CREAT | os.O_EXCL | os.O_WRONLY | os.O_NOFOLLOW, 0o600)
            os.close(fd)
        _check_file(path)
        self.db = sqlite3.connect(path.as_uri() + "?mode=rw", uri=True, timeout=0.1)
        self.db.row_factory = sqlite3.Row
        self.db.executescript(
            """
            PRAGMA journal_mode=WAL;
            PRAGMA synchronous=FULL;
            PRAGMA foreign_keys=ON;
            PRAGMA wal_autocheckpoint=256;
            PRAGMA journal_size_limit=8388608;
            PRAGMA max_page_count=32768;
            CREATE TABLE IF NOT EXISTS evidence_meta(
              singleton INTEGER PRIMARY KEY CHECK(singleton=1), last_tick_ms INTEGER NOT NULL);
            INSERT OR IGNORE INTO evidence_meta VALUES(1,0);
            CREATE TABLE IF NOT EXISTS candidates(
              sequence INTEGER PRIMARY KEY AUTOINCREMENT, id TEXT UNIQUE NOT NULL,
              created_ms INTEGER NOT NULL, complete INTEGER NOT NULL, data TEXT NOT NULL);
            CREATE INDEX IF NOT EXISTS pending_candidates ON candidates(complete,created_ms);
            """
        )

    def close(self):
        self.db.close()

    def observe(self, news, quotes, symbols, fingerprint, costs, policy_revision, at_ms):
        if any(
            type(costs.get(k)) is not int or not 0 <= costs[k] <= 1000
            for k in ("fee_bps", "slippage_bps")
        ):
            raise ValueError("invalid evidence cost assumptions")
        if len(news) > 200 or not set(symbols) <= SYMBOLS:
            raise ValueError("evidence input exceeds bound")
        with self.db:
            last = self.db.execute("SELECT last_tick_ms FROM evidence_meta").fetchone()[0]
            if at_ms < last:
                raise ValueError("evidence clock regression")
            self.db.execute(
                "DELETE FROM candidates WHERE complete=1 AND created_ms<?", (at_ms - RETENTION_MS,)
            )
            count = self.db.execute("SELECT COUNT(*) FROM candidates").fetchone()[0]
            for item in news:
                if (
                    item.get("eligible") is not True
                    or type(item.get("published_ms")) is not int
                    or type(item.get("observed_ms")) is not int
                    or not 0 <= at_ms - item["published_ms"] <= 120_000
                    or not 0 <= at_ms - item["observed_ms"] <= 120_000
                ):
                    continue
                for symbol in symbols:
                    identity = _identity(item["news_id"], symbol, fingerprint)
                    if self.db.execute(
                        "SELECT 1 FROM candidates WHERE id=?", (identity,)
                    ).fetchone():
                        continue
                    if count >= MAX_CANDIDATES:
                        raise ValueError("news evidence capacity reached")
                    data = {
                        "schema_version": 1,
                        "synthetic_inputs": item.get("synthetic") is True,
                        "reference_source": (
                            "synthetic" if item.get("synthetic") is True else "coinbase"
                        ),
                        "news": {key: item[key] for key in NEWS_FIELDS if key in item},
                        "symbol": symbol,
                        "strategy_fingerprint": fingerprint,
                        "policy_revision_at_capture": policy_revision,
                        "costs": {key: costs[key] for key in ("fee_bps", "slippage_bps")},
                        "capture_status": "waiting_for_reference",
                        "reference": None,
                        "reference_at_ms": None,
                        "markouts": [],
                        "transitions": [],
                        "omitted_transitions": 0,
                        "timing": {"capture_started_ms": at_ms},
                    }
                    wire = canonical(data)
                    if len(wire.encode()) > 4096:
                        raise ValueError("news evidence exceeds size bound")
                    self.db.execute(
                        "INSERT INTO candidates(id,created_ms,complete,data) VALUES(?,?,0,?)",
                        (identity, at_ms, wire),
                    )
                    count += 1
            for row in self.db.execute("SELECT * FROM candidates WHERE complete=0").fetchall():
                data = json.loads(row["data"])
                quote = reference_quote(quotes.get(data["symbol"]), at_ms)
                complete = False
                if data["reference"] is None:
                    first = min(data["news"]["published_ms"], data["news"]["observed_ms"])
                    if at_ms - first > 120_000:
                        data["capture_status"], complete = "missing_reference", True
                    elif quote:
                        data["reference"] = quote
                        data["reference_at_ms"] = at_ms
                        data["capture_status"] = "tracking"
                if data["reference"] is not None:
                    measured = {point["horizon_ms"] for point in data["markouts"]}
                    for horizon in HORIZONS_MS:
                        due = data["reference_at_ms"] + horizon
                        if horizon in measured or at_ms < due:
                            continue
                        if at_ms > due + MAX_LAG_MS:
                            point = {
                                "status": "missing",
                                "reason": "no_timely_quote_or_runtime_gap",
                            }
                        elif quote is not None and quote["observed_ms"] >= due:
                            point = {
                                "status": "observed",
                                "quote": quote,
                                "reference_return_bps_after_costs": reference_return_bps(
                                    data["reference"], quote, data["costs"]
                                ),
                            }
                        else:
                            continue
                        data["markouts"].append(
                            {
                                **point,
                                "horizon_ms": horizon,
                                "due_ms": due,
                                "recorded_ms": at_ms,
                            }
                        )
                    complete = len(data["markouts"]) == len(HORIZONS_MS)
                    if complete:
                        data["capture_status"] = "complete"
                wire = canonical(data)
                if wire != row["data"] or complete:
                    self.db.execute(
                        "UPDATE candidates SET data=?,complete=? WHERE id=?",
                        (wire, int(complete), row["id"]),
                    )
            self.db.execute("UPDATE evidence_meta SET last_tick_ms=?", (at_ms,))

    def record(self, news_ids, symbol, fingerprint, transition, at_ms):
        allowed = {
            "stage",
            "action",
            "reason",
            "proposed_action",
            "proposed_reason",
            "provider",
            "policy_revision",
            "selected_news_id",
            "event_id",
            "outcome",
            "cursor",
            "review_id",
            "evidence_news_ids",
            "review_expires_ms",
        }
        review_fields = {"review_id", "evidence_news_ids", "review_expires_ms"}
        if (
            type(at_ms) is not int
            or at_ms < 0
            or set(transition) - allowed
            or transition.get("stage")
            not in {
                "evaluation",
                "submission",
                "commit",
                "deferred",
                "review_started",
                "review_finished",
            }
            or len(canonical(transition).encode()) > 2048
            or len(news_ids) > 200
        ):
            raise ValueError("invalid decision evidence")
        if review_fields & set(transition):
            ids = transition.get("evidence_news_ids")
            if (
                not review_fields <= set(transition)
                or not isinstance(transition["review_id"], str)
                or not HEX_ID.fullmatch(transition["review_id"])
                or not isinstance(ids, list)
                or not 1 <= len(ids) <= MAX_NEWS_EVIDENCE
                or any(not isinstance(value, str) or not HEX_ID.fullmatch(value) for value in ids)
                or len(set(ids)) != len(ids)
                or type(transition["review_expires_ms"]) is not int
                or transition["review_expires_ms"] < 1
            ):
                raise ValueError("invalid review evidence binding")
        recorded = 0
        with self.db:
            for news_id in news_ids:
                identity = _identity(news_id, symbol, fingerprint)
                row = self.db.execute(
                    "SELECT data FROM candidates WHERE id=?", (identity,)
                ).fetchone()
                if row is None:
                    continue
                data = json.loads(row["data"])
                previous = data["transitions"][-1] if data["transitions"] else None
                if (
                    at_ms < data["news"]["observed_ms"]
                    or at_ms < data.get("timing", {}).get("capture_started_ms", 0)
                    or (previous and previous["at_ms"] > at_ms)
                ):
                    raise ValueError("decision evidence clock regression")
                # New captures retain first milestones even when bounded transitions roll over.
                # Legacy captures cannot establish their first-ever milestone retrospectively.
                if "timing" in data:
                    timing = data["timing"]
                    milestone = {
                        "evaluation": "first_evaluation_ms",
                        "review_started": "first_review_started_ms",
                        "review_finished": "first_review_finished_ms",
                        "submission": "first_submission_intent_ms",
                        "commit": "first_commit_observed_ms",
                    }.get(transition["stage"])
                    if milestone:
                        timing.setdefault(milestone, at_ms)
                    if (
                        transition["stage"] == "evaluation"
                        and transition.get("proposed_action") == "enter"
                        and transition.get("selected_news_id") == news_id
                    ):
                        timing.setdefault("first_entry_proposal_ms", at_ms)
                    if (
                        transition["stage"] == "commit"
                        and transition.get("outcome") == "simulated_fill"
                    ):
                        timing.setdefault("first_fill_observed_ms", at_ms)
                if previous is None or previous["decision"] != transition:
                    data["transitions"].append({"at_ms": at_ms, "decision": transition})
                    if len(data["transitions"]) > MAX_TRANSITIONS:
                        data["transitions"].pop(0)
                        data["omitted_transitions"] += 1
                wire = canonical(data)
                if wire != row["data"]:
                    self.db.execute("UPDATE candidates SET data=? WHERE id=?", (wire, identity))
                recorded += 1
        return recorded


@contextmanager
def _reader(path):
    _check_file(path)
    db = sqlite3.connect(path.as_uri() + "?mode=ro", uri=True, timeout=0.2)
    db.row_factory = sqlite3.Row
    try:
        db.execute("PRAGMA query_only=ON")
        db.execute("BEGIN")
        yield db
    finally:
        db.close()


def read_news_evidence(store, client, *, limit=20, before_sequence=None):
    """Recheck durable ACL before reading this non-authoritative local projection."""
    status = store.status(client)
    if (
        type(limit) is not int
        or not 1 <= limit <= 50
        or (
            before_sequence is not None
            and (type(before_sequence) is not int or before_sequence < 1)
        )
    ):
        raise ControlError("invalid evidence page bounds")
    root = private_root(store.root)
    now = time.time_ns() // 1_000_000
    result = {
        "schema_version": 1,
        "execution_authority": False,
        "as_of_ms": now,
        "semantics": {
            "scope": "forward reference quotes with explicit source and synthetic flags",
            "not_strategy_pnl": True,
            "costs": "buy at ask, sell at bid; frozen fees and slippage on both sides",
            "missing_points": "never backfilled from later or pre-deadline quotes",
            "selection": "fresh eligible stories while recorder is healthy, including rejections",
            "transitions": "decisions about the active news set; not individual headline scores",
            "latency": (
                "milliseconds between observed lifecycle milestones, including strategy waiting; "
                "publication time is publisher-reported, not verified event time; "
                "review includes cache/budget/API work and any restart downtime; "
                "fills are local receipt observations; "
                "missing historical timings are null"
            ),
            "source_delivery_latency": (
                "publisher-reported publication to first local receipt, in milliseconds; "
                "startup baselines, synthetic news and invalid clocks are excluded; "
                "explicit non-synthetic and legacy unlabelled samples are kept separate; "
                "percentiles use nearest rank; an empty sample is unknown; "
                "HTTP cache age is a transport diagnostic, not event delivery latency"
            ),
            "retention_days": 30,
            "maximum_candidates": MAX_CANDIDATES,
            "maximum_transitions_per_candidate": MAX_TRANSITIONS,
            "horizons_ms": list(HORIZONS_MS),
            "maximum_measurement_delay_ms": MAX_LAG_MS,
        },
        "intake": {"sources": {}, "recent": []},
        "evidence": {"state": "not_recording", "candidates": [], "next_before_sequence": None},
    }
    runtime = status.get("runtime", {})
    entry = runtime.get("crypto-shadow", {}) if isinstance(runtime, dict) else {}
    result["intake"]["writer_health"] = {
        "reported_at_ns": entry.get("updated_ns"),
        "feeds": entry.get("status", {}).get("feeds", {}),
    }
    try:
        path = root / "news-intake.sqlite3"
        if path.exists() or path.is_symlink():
            delays = {}
            with _reader(path) as db:
                for row in db.execute(
                    "SELECT data FROM news ORDER BY observed_ms DESC,id LIMIT 10000"
                ):
                    item = json.loads(row["data"])
                    source = result["intake"]["sources"].setdefault(
                        item["source"],
                        {
                            "stories": 0,
                            "eligible": 0,
                            "reasons": {},
                            "latest_published_ms": None,
                            "delivery_excluded": {"baseline": 0, "synthetic": 0, "invalid": 0},
                        },
                    )
                    groups = delays.setdefault(
                        item["source"], {"explicit_non_synthetic": [], "legacy_unlabelled": []}
                    )
                    source["stories"] += 1
                    source["eligible"] += int(item["eligible"])
                    reason = item["intake_reason"]
                    source["reasons"][reason] = source["reasons"].get(reason, 0) + 1
                    published = item.get("published_ms")
                    observed = item.get("observed_ms")
                    if type(published) is int:
                        source["latest_published_ms"] = max(
                            source["latest_published_ms"] or 0, published
                        )
                    if item.get("synthetic") is True:
                        source["delivery_excluded"]["synthetic"] += 1
                    elif reason == "startup_baseline":
                        source["delivery_excluded"]["baseline"] += 1
                    elif (
                        type(published) is not int
                        or type(observed) is not int
                        or not 0 <= published <= observed <= now
                        or ("synthetic" in item and type(item["synthetic"]) is not bool)
                    ):
                        source["delivery_excluded"]["invalid"] += 1
                    else:
                        group = (
                            "explicit_non_synthetic"
                            if item.get("synthetic") is False
                            else "legacy_unlabelled"
                        )
                        groups[group].append(observed - published)
                    if len(result["intake"]["recent"]) < limit:
                        result["intake"]["recent"].append(
                            {key: item[key] for key in NEWS_FIELDS if key in item}
                        )
            for name, groups in delays.items():
                result["intake"]["sources"][name]["publication_to_intake_ms"] = {
                    group: delay_summary(values) for group, values in groups.items()
                }
        path = root / "news-evidence.sqlite3"
        if path.exists() or path.is_symlink():
            with _reader(path) as db:
                last = db.execute("SELECT last_tick_ms FROM evidence_meta").fetchone()[0]
                rows = db.execute(
                    "SELECT sequence,data FROM candidates WHERE sequence<? "
                    "ORDER BY sequence DESC LIMIT ?",
                    (before_sequence if before_sequence is not None else 2**63 - 1, limit + 1),
                ).fetchall()
                candidates = [
                    {"sequence": row["sequence"], **json.loads(row["data"])} for row in rows[:limit]
                ]
                fills = status.get("paper", {}).get("recent_fills", [])
                for candidate in candidates:
                    candidate["latency_ms"] = reaction_latency(candidate)
                    event_ids = {
                        event["decision"].get("event_id") for event in candidate["transitions"]
                    } - {None}
                    candidate["matches_in_recent_golem_paper_fills"] = [
                        fill for fill in fills if fill["event_id"] in event_ids
                    ]
                result["evidence"] = {
                    "state": "available",
                    "last_observation_ms": last,
                    "observation_age_ms": now - last if last else None,
                    "candidate_count": db.execute("SELECT COUNT(*) FROM candidates").fetchone()[0],
                    "candidates": candidates,
                    "next_before_sequence": rows[limit - 1]["sequence"]
                    if len(rows) > limit
                    else None,
                }
                runtime = status.get("runtime", {})
                if isinstance(runtime, dict):
                    entry = runtime.get("crypto-shadow", {})
                    result["evidence"]["writer_health"] = {
                        "runtime_state": entry.get("status", {}).get("state"),
                        "reported_at_ns": entry.get("updated_ns"),
                        "recorder": entry.get("status", {}).get("evidence"),
                    }
    except (sqlite3.Error, ValueError, KeyError, TypeError) as exc:
        raise ControlError("news evidence is unavailable or invalid") from exc
    return result
