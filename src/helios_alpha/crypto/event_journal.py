"""Durable event registry, observations, edge-triggered rules and publish outbox."""

from __future__ import annotations

import hashlib
import json
import math
import sqlite3
from pathlib import Path

from helios_alpha.control.store import canonical


def validate_config(config):
    bounds = {
        "revision": (1, 10**9),
        "poll_seconds": (15, 300),
        "discovery_seconds": (300, 86400),
        "markets_per_provider": (1, 20),
        "lookback_seconds": (120, 3600),
        "move_pp": (0.5, 50),
        "reset_pp": (0, 20),
        "cooldown_seconds": (60, 86400),
        "max_spread_pp": (0.1, 20),
        "min_depth_usd": (1, 100000),
        "max_cache_age_seconds": (0, 120),
        "retention_days": (1, 90),
    }
    if config.get("schema_version") != 1 or type(config.get("enabled")) is not bool:
        raise ValueError("invalid event configuration")
    for key, (low, high) in bounds.items():
        value = config.get(key)
        if type(value) not in (int, float) or not math.isfinite(value) or not low <= value <= high:
            raise ValueError("invalid event configuration bound")
        if (
            key not in {"move_pp", "reset_pp", "max_spread_pp", "min_depth_usd"}
            and type(value) is not int
        ):
            raise ValueError("event period must be an integer")
    if (
        config["reset_pp"] >= config["move_pp"]
        or config["lookback_seconds"] < config["poll_seconds"] * 3
    ):
        raise ValueError("invalid event hysteresis or lookback")
    for key in ("polymarket_tags", "kalshi_series"):
        if (
            not isinstance(config.get(key), list)
            or not 1 <= len(config[key]) <= 6
            or any(not isinstance(x, str) or not 1 <= len(x) <= 80 for x in config[key])
        ):
            raise ValueError("invalid discovery configuration")
    if not isinstance(config.get("topics"), list) or not 1 <= len(config["topics"]) <= 12:
        raise ValueError("invalid event topics")
    for topic in config["topics"]:
        if not isinstance(topic, dict) or not all(
            isinstance(topic.get(k), str) and 1 <= len(topic[k]) <= 80 for k in ("id", "label")
        ):
            raise ValueError("invalid event topic")
        for key in ("terms", "symbols"):
            if (
                not isinstance(topic.get(key), list)
                or not 1 <= len(topic[key]) <= 20
                or any(not isinstance(x, str) or not x or len(x) > 80 for x in topic[key])
            ):
                raise ValueError("invalid event topic inputs")
    return config


class EventJournal:
    def __init__(self, path: Path):
        if path.is_symlink():
            raise ValueError("event journal must not be a symlink")
        self.db = sqlite3.connect(path)
        path.chmod(0o600)
        self.db.row_factory = sqlite3.Row
        self.db.executescript("""
          PRAGMA journal_mode=WAL;
          PRAGMA synchronous=FULL;
          CREATE TABLE IF NOT EXISTS registry(id TEXT PRIMARY KEY, provider TEXT, data TEXT,
            first_seen_ms INTEGER, last_seen_ms INTEGER, watched INTEGER DEFAULT 1);
          CREATE TABLE IF NOT EXISTS observations(seq INTEGER PRIMARY KEY AUTOINCREMENT,
            market_id TEXT, observed_ms INTEGER, received_ms INTEGER, revision INTEGER,
            data TEXT, UNIQUE(market_id,observed_ms));
          CREATE INDEX IF NOT EXISTS event_history ON observations(market_id,observed_ms);
          CREATE INDEX IF NOT EXISTS event_retention ON observations(received_ms);
          CREATE TABLE IF NOT EXISTS rules(market_id TEXT PRIMARY KEY, revision INTEGER,
            state TEXT, delta_pp REAL, latched INTEGER DEFAULT 0,
            last_trigger_ms INTEGER DEFAULT 0);
          CREATE TABLE IF NOT EXISTS triggers(id TEXT PRIMARY KEY, created_ms INTEGER,
            data TEXT, delivered INTEGER DEFAULT 0, stream_sequence INTEGER);
          CREATE TABLE IF NOT EXISTS transitions(seq INTEGER PRIMARY KEY AUTOINCREMENT,
            market_id TEXT, at_ms INTEGER, state TEXT, revision INTEGER);
        """)

    def register(self, provider, items, at):
        with self.db:
            self.db.execute("UPDATE registry SET watched=0 WHERE provider=?", (provider,))
            for item in items:
                self.db.execute(
                    "INSERT INTO registry VALUES(?,?,?,?,?,1) ON CONFLICT(id) DO UPDATE SET "
                    "data=excluded.data,last_seen_ms=excluded.last_seen_ms,watched=1",
                    (item["id"], provider, canonical(item), at, at),
                )

    def watched(self, provider=None):
        rows = self.db.execute(
            "SELECT data FROM registry WHERE watched=1 AND (? IS NULL OR provider=?) ORDER BY id",
            (provider, provider),
        )
        return [json.loads(row[0]) for row in rows]

    def record(self, item, observation, config):
        now = observation["received_ms"]
        observed = observation["observed_ms"]
        probability = observation["probability"]
        if not 0 < probability < 1 or not all(
            math.isfinite(observation[k])
            for k in ("probability", "bid", "ask", "depth_usd", "spread_pp")
        ):
            raise ValueError("invalid event observation")
        if not 0 < observation["bid"] <= observation["ask"] < 1 or observed > now:
            raise ValueError("invalid event quote or clock")
        with self.db:
            latest = self.db.execute(
                "SELECT MAX(observed_ms) FROM observations WHERE market_id=?", (item["id"],)
            ).fetchone()[0]
            inserted = self.db.execute(
                "INSERT OR IGNORE INTO "
                "observations(market_id,observed_ms,received_ms,revision,data) VALUES(?,?,?,?,?)",
                (item["id"], observed, now, config["revision"], canonical(observation)),
            )
            # Retain late deliveries for audit, but never rewind the detector.
            if not inserted.rowcount or latest is not None and observed < latest:
                return None
            seq = inserted.lastrowid
            previous = self.db.execute(
                "SELECT * FROM rules WHERE market_id=?", (item["id"],)
            ).fetchone()
            same_rule = previous is not None and previous["revision"] == config["revision"]
            latched = previous["latched"] if same_rule else 0
            last_trigger = previous["last_trigger_ms"] if same_rule else 0
            target = observed - config["lookback_seconds"] * 1000
            tolerance = config["poll_seconds"] * 3000
            baseline = self.db.execute(
                "SELECT * FROM observations WHERE market_id=? AND revision=? AND observed_ms "
                "BETWEEN ? AND ? ORDER BY observed_ms DESC LIMIT 1",
                (item["id"], config["revision"], target - tolerance, target),
            ).fetchone()
            state, delta = "warming_up", None
            if baseline:
                rows = self.db.execute(
                    "SELECT observed_ms FROM observations WHERE market_id=? AND revision=? AND "
                    "observed_ms BETWEEN ? AND ? ORDER BY observed_ms",
                    (item["id"], config["revision"], baseline["observed_ms"], observed),
                ).fetchall()
                continuous = all(b[0] - a[0] <= tolerance for a, b in zip(rows, rows[1:]))
                if continuous:
                    delta = (probability - json.loads(baseline["data"])["probability"]) * 100
                    state = "watching"

            def quote_quality(point):
                return (
                    point["received_ms"] - point["observed_ms"]
                    <= config["max_cache_age_seconds"] * 1000
                    and point["spread_pp"] <= config["max_spread_pp"]
                    and point["depth_usd"] >= config["min_depth_usd"]
                )

            quality = quote_quality(observation) and (
                baseline is None or quote_quality(json.loads(baseline["data"]))
            )
            if not config["enabled"]:
                state = "paused"
            elif now >= item["closes_ms"]:
                state = "closed"
            elif not quality:
                state = "quality_blocked"
            signal = None
            if state == "watching" and delta is not None:
                if abs(delta) <= config["reset_pp"]:
                    latched = 0
                if latched or now - last_trigger < config["cooldown_seconds"] * 1000:
                    state = "cooldown"
                elif abs(delta) >= config["move_pp"]:
                    state, latched, last_trigger = "triggered", 1, now
                    identity = hashlib.sha256(
                        f"{item['id']}:{config['revision']}:{seq}".encode()
                    ).hexdigest()
                    signal = dict(
                        schema_version=1,
                        id=identity,
                        kind="prediction_probability_shock",
                        market_id=item["id"],
                        event_id=item["event_id"],
                        provider=item["provider"],
                        title=item["title"],
                        topic=item["topic"],
                        symbols=item["symbols"],
                        observed_ms=observed,
                        received_ms=now,
                        baseline_observed_ms=baseline["observed_ms"],
                        probability=probability,
                        previous_probability=json.loads(baseline["data"])["probability"],
                        delta_pp=delta,
                        spread_pp=observation["spread_pp"],
                        depth_usd=observation["depth_usd"],
                        rule_revision=config["revision"],
                        expires_ms=now + 120000,
                        execution_authority=False,
                    )
                    self.db.execute(
                        "INSERT OR IGNORE INTO triggers(id,created_ms,data) VALUES(?,?,?)",
                        (identity, now, canonical(signal)),
                    )
            if not previous or previous["state"] != state or not same_rule:
                self.db.execute(
                    "INSERT INTO transitions(market_id,at_ms,state,revision) VALUES(?,?,?,?)",
                    (item["id"], now, state, config["revision"]),
                )
            self.db.execute(
                "INSERT INTO rules VALUES(?,?,?,?,?,?) ON CONFLICT(market_id) DO UPDATE SET "
                "revision=excluded.revision,state=excluded.state,delta_pp=excluded.delta_pp,latched=excluded.latched,last_trigger_ms=excluded.last_trigger_ms",
                (item["id"], config["revision"], state, delta, latched, last_trigger),
            )
            return signal

    def pending(self):
        return [
            json.loads(row[0])
            for row in self.db.execute(
                "SELECT data FROM triggers WHERE delivered=0 ORDER BY created_ms LIMIT 100"
            )
        ]

    def delivered(self, identity, sequence):
        with self.db:
            self.db.execute(
                "UPDATE triggers SET delivered=1,stream_sequence=? WHERE id=?", (sequence, identity)
            )

    def trim(self, now, days):
        with self.db:
            cutoff = now - days * 86400_000
            self.db.execute("DELETE FROM observations WHERE received_ms<?", (cutoff,))
            self.db.execute("DELETE FROM transitions WHERE at_ms<?", (cutoff,))
            self.db.execute("DELETE FROM triggers WHERE created_ms<? AND delivered=1", (cutoff,))
            self.db.execute("DELETE FROM registry WHERE watched=0 AND last_seen_ms<?", (cutoff,))

    def snapshot(self, now, config):
        markets = []
        for item in self.watched():
            rows = self.db.execute(
                "SELECT data FROM observations WHERE market_id=? AND observed_ms>=? ORDER BY "
                "observed_ms",
                (item["id"], now - 3600_000),
            ).fetchall()
            points = [json.loads(row[0]) for row in rows]
            latest = points[-1] if points else None
            rule = self.db.execute(
                "SELECT * FROM rules WHERE market_id=?", (item["id"],)
            ).fetchone()
            if rule and rule["revision"] != config["revision"]:
                rule = None
            state = rule["state"] if rule else "warming_up"
            if not latest or now - latest["observed_ms"] > max(
                90000, config["poll_seconds"] * 3000
            ):
                state = "stale" if rule else "awaiting_quote"
            if now >= item["closes_ms"]:
                state = "closed"
            if not config["enabled"]:
                state = "paused"
            markets.append(
                {k: v for k, v in item.items() if k != "token_id"}
                | dict(
                    state=state,
                    delta_pp=rule["delta_pp"] if rule else None,
                    latest=latest,
                    history=[
                        {"at_ms": p["observed_ms"], "probability": p["probability"]}
                        for p in points[:: max(1, math.ceil(len(points) / 120))]
                    ],
                    samples=len(points),
                )
            )
        triggers = [
            json.loads(row["data"])
            | {"delivered": bool(row["delivered"]), "stream_sequence": row["stream_sequence"]}
            for row in self.db.execute("SELECT * FROM triggers ORDER BY created_ms DESC LIMIT 30")
        ]
        return dict(
            schema_version=1,
            observed_ms=now,
            execution_authority=False,
            config=config,
            markets=markets,
            triggers=triggers,
            recorded_observations=self.db.execute("SELECT COUNT(*) FROM observations").fetchone()[
                0
            ],
            registered_events=self.db.execute(
                "SELECT COUNT(DISTINCT provider || ':' || json_extract(data,'$.event_id')) FROM "
                "registry"
            ).fetchone()[0],
            pending_signals=self.db.execute(
                "SELECT COUNT(*) FROM triggers WHERE delivered=0"
            ).fetchone()[0],
        )

    def close(self):
        self.db.close()
