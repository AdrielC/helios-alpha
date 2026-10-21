"""Durable research records. These settings never grant broker execution authority."""

from __future__ import annotations

import hashlib
import json
import math
import os
import sqlite3
from datetime import UTC, datetime
from pathlib import Path

from helios_alpha.control.broker_sync import write_private
from helios_alpha.control.golem_store import read_private_json
from helios_alpha.control.store import canonical, private_root

DEFAULT_CONFIG = Path(__file__).resolve().parents[3] / "config/natgas/research-v1.json"


def digest(value):
    return hashlib.sha256(canonical(value).encode()).hexdigest()


def settings(root):
    path = root / "natgas-config.json"
    if not path.exists():
        write_private(path, json.loads(DEFAULT_CONFIG.read_text()))
    saved = read_private_json(path)
    config = {**json.loads(DEFAULT_CONFIG.read_text()), **saved}
    if config["schema_version"] != 1 or config["mode"] != "research" or config["symbol"] != "UNG":
        raise ValueError("unsupported research configuration")
    for key, lo, hi in (
        ("poll_seconds", 60, 3600),
        ("quote_poll_seconds", 30, 3600),
        ("revision", 1, 10**9),
        ("target_dte", 7, 90),
        ("min_dte", 1, 90),
        ("max_dte", 7, 180),
        ("weather_overlap_days", 2, 7),
        ("weather_revision_threshold", 1, 100),
        ("storage_surprise_threshold_bcf", 1, 100),
        ("max_event_age_seconds", 30, 3600),
        ("decision_timeout_ms", 1000, 10000),
    ):
        if type(config[key]) is not int or not lo <= config[key] <= hi:
            raise ValueError("invalid research bound")
    if not config["min_dte"] <= config["target_dte"] <= config["max_dte"]:
        raise ValueError("invalid expiry range")
    if type(config["decision_enabled"]) is not bool:
        raise ValueError("invalid decision setting")
    for key in (
        "decision_min_evidence_probability",
        "decision_min_choice_probability",
        "decision_min_confidence",
    ):
        value = config[key]
        if type(value) not in (float, int) or not 0 <= value <= 1:
            raise ValueError("invalid decision threshold")
    from .replay import ReplayPolicy

    ReplayPolicy(**config["replay"])
    if not 1 <= len(config["exit_profiles"]) <= 10:
        raise ValueError("invalid exit profile count")
    for profile in config["exit_profiles"]:
        ReplayPolicy(**{**config["replay"], **{k: v for k, v in profile.items() if k != "id"}})
    if config != saved:
        write_private(path, config)
    return config


class Journal:
    def __init__(self, root: Path):
        self.root = private_root(root)
        path = root / "natgas.sqlite3"
        fd = os.open(path, os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
        os.close(fd)
        path.chmod(0o600)
        self.db = sqlite3.connect(path, timeout=10)
        self.db.row_factory = sqlite3.Row
        self.db.executescript("""
          PRAGMA journal_mode=WAL;
          PRAGMA synchronous=FULL;
          CREATE TABLE IF NOT EXISTS ng_evidence(
            id TEXT PRIMARY KEY, source TEXT, available_ms INTEGER, data TEXT, raw TEXT);
          CREATE INDEX IF NOT EXISTS ng_evidence_time ON ng_evidence(source, available_ms);
          CREATE TABLE IF NOT EXISTS ng_quotes(
            id TEXT PRIMARY KEY, contract_id TEXT, observed_ms INTEGER,
            available_ms INTEGER, data TEXT);
          CREATE TABLE IF NOT EXISTS ng_contracts(
            id TEXT PRIMARY KEY, available_ms INTEGER, data TEXT);
          CREATE TABLE IF NOT EXISTS ng_health(source TEXT PRIMARY KEY, data TEXT);
          CREATE TABLE IF NOT EXISTS ng_decisions(id TEXT PRIMARY KEY, data TEXT);
        """)

    def close(self):
        self.db.close()

    def record(self, source, data, raw, at):
        if type(at) is not int or at < 0 or len(raw.encode()) > 262144:
            raise ValueError("invalid evidence boundary")
        if data.get("published_ms") is not None and data["published_ms"] > at:
            raise ValueError("future source vintage")
        identity = digest({"source": source, "data": data})
        with self.db:
            self.db.execute(
                "INSERT OR IGNORE INTO ng_evidence VALUES(?,?,?,?,?)",
                (identity, source, at, canonical(data), raw),
            )
        return identity

    def evidence(self, source, as_of, limit=100):
        return [
            dict(id=r["id"], source=source, available_ms=r["available_ms"], **json.loads(r["data"]))
            for r in self.db.execute(
                "SELECT * FROM ng_evidence WHERE source=? AND available_ms<=? "
                "ORDER BY available_ms DESC,id LIMIT ?",
                (source, as_of, limit),
            )
        ]

    def health(self, source, state, at):
        with self.db:
            self.db.execute(
                "INSERT OR REPLACE INTO ng_health VALUES(?,?)",
                (source, canonical({"source": source, "state": state, "checked_ms": at})),
            )

    def record_options(self, contracts, quotes, at):
        from .replay import Contract, Quote

        parsed_contracts = [Contract(**c) for c in contracts]
        parsed_quotes = [Quote(**q) for q in quotes]
        if any(c.available_ms > at for c in parsed_contracts) or any(
            q.available_ms > at for q in parsed_quotes
        ):
            raise ValueError("future market observation")
        with self.db:
            for c in parsed_contracts:
                # Contract identity may not silently change deliverables after a split.
                old = self.db.execute(
                    "SELECT data FROM ng_contracts WHERE id=?", (c.id,)
                ).fetchone()
                data = c.model_dump()
                if old:
                    prior = json.loads(old[0])
                    data["available_ms"] = prior["available_ms"]
                    if data != prior:
                        raise ValueError("contract metadata changed; review corporate action")
                else:
                    self.db.execute(
                        "INSERT INTO ng_contracts VALUES(?,?,?)",
                        (c.id, c.available_ms, canonical(data)),
                    )
            for q in parsed_quotes:
                # Optional native history clocks must not change legacy live quote identities.
                data = q.model_dump(exclude_none=True)
                # Repeated reads of the same quote are not additional samples.
                identity = digest({k: v for k, v in data.items() if k != "available_ms"})
                self.db.execute(
                    "INSERT OR IGNORE INTO ng_quotes VALUES(?,?,?,?,?)",
                    (identity, q.contract_id, q.observed_ms, q.available_ms, canonical(data)),
                )

    def snapshot(self, now, config):
        signals = []
        for source in ("noaa-hdd", "noaa-cdd"):
            history = self.evidence(source, now)
            revisions = []
            for current, prior in zip(history, history[1:]):
                signal = weather_revision(current, prior, config)
                if signal:
                    revisions.append(signal)
            if revisions:
                signal = revisions[0]
                signal.update(
                    statistics([x["value"] for x in reversed(revisions[1:])], signal["value"])
                )
                signal["candidate"] = (
                    abs(signal["value"]) >= config["weather_revision_threshold"]
                    and 0 <= now - signal["available_ms"] <= config["max_event_age_seconds"] * 1000
                    and 0 <= now - signal["published_ms"] <= 36 * 3600_000
                )
                signals.append(signal)
        storage = self.evidence("eia-storage", now, 1)
        weather = [e for s in ("noaa-hdd", "noaa-cdd") for e in self.evidence(s, now, 1)]
        contracts = {
            r["id"]: json.loads(r["data"]) for r in self.db.execute("SELECT * FROM ng_contracts")
        }
        quotes = []
        for row in self.db.execute("""
            SELECT * FROM (SELECT *, ROW_NUMBER() OVER (
              PARTITION BY contract_id ORDER BY observed_ms DESC,id) AS rank FROM ng_quotes)
            WHERE rank=1 ORDER BY observed_ms DESC,id
        """):
            q = json.loads(row["data"])
            c = contracts.get(q["contract_id"])
            if c is None or c["close_by_ms"] <= now:
                continue
            q.update(
                kind=c["kind"],
                strike_cents=c["strike_cents"],
                expiry=c["expiry"],
                multiplier=c["multiplier"],
                premium_cents=q["ask_cents"] * c["multiplier"],
                spread_cost_cents=(q["ask_cents"] - q["bid_cents"]) * c["multiplier"],
                fresh=0 <= now - q["observed_ms"] <= config["replay"]["max_quote_age_ms"],
            )
            quotes.append(q)
            if len(quotes) == 12:
                break
        counts = {
            name: self.db.execute(f"SELECT COUNT(*) FROM ng_{name}").fetchone()[0]
            for name in ("evidence", "quotes", "decisions")
        }
        return {
            "schema_version": 1,
            "observed_ms": now,
            "execution_authority": False,
            "mode": "research",
            "config_digest": digest(config),
            "counts": counts,
            "sources": [json.loads(r[0]) for r in self.db.execute("SELECT data FROM ng_health")],
            "storage": storage[0] if storage else None,
            "weather": weather,
            "signals": signals,
            "premiums": quotes,
            "hypotheses": config["hypotheses"],
            "exit_profiles": config["exit_profiles"],
            "max_quote_age_ms": config["replay"]["max_quote_age_ms"],
            "decision_enabled": config["decision_enabled"],
            "backtest": {
                "state": "collecting_evidence",
                "profitability_established": False,
                "blockers": [
                    "Historical option bars contain no bid/ask fills.",
                    "Archive event vintages and executable quotes prospectively.",
                    "Freeze a pre-release storage forecast before testing surprise.",
                    "No held-out options performance has been measured.",
                ],
            },
        }


def weather_revision(current, prior, config):
    if (
        current["kind"] != prior["kind"]
        or current["published_ms"] <= prior["published_ms"]
        or current["available_ms"] <= prior["available_ms"]
    ):
        return None
    today = datetime.fromtimestamp(current["published_ms"] / 1000, UTC).date().isoformat()
    common = sorted(d for d in set(current["daily"]) & set(prior["daily"]) if d >= today)
    if len(common) < config["weather_overlap_days"]:
        return None
    return {
        "kind": current["kind"],
        "value": sum(current["daily"][d] - prior["daily"][d] for d in common),
        "dates": common,
        "available_ms": current["available_ms"],
        "published_ms": current["published_ms"],
        "evidence_ids": [prior["id"], current["id"]],
    }


def statistics(history, value, alpha=0.2):
    # Current event is scored against prior events only; zero means historical mean.
    if len(history) < 5:
        return {
            "baseline_samples": len(history),
            "mean": None,
            "stddev": None,
            "z_score": None,
            "ewma": None,
        }
    mean = sum(history) / len(history)
    std = math.sqrt(sum((x - mean) ** 2 for x in history) / (len(history) - 1))
    ewma = history[0]
    for x in history[1:]:
        ewma = alpha * x + (1 - alpha) * ewma
    return {
        "baseline_samples": len(history),
        "mean": mean,
        "stddev": std,
        "z_score": (value - mean) / std if std > 0 else None,
        "ewma": ewma,
    }
