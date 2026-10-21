"""Point-in-time event definitions and market-implied forecasts in the event journal.

Forecasts are observations of market expectations, never model confidence or orders.
Original definitions and issue-time evidence remain immutable when a source changes.
"""

from __future__ import annotations

import hashlib
import json
from collections import defaultdict

from helios_alpha.control.store import canonical

METHOD = "prediction-market-midpoint/v1"
LIMIT = 100
HISTORY_LIMIT = 12


def initialize(db):
    db.executescript("""
      CREATE TABLE IF NOT EXISTS event_entities(
        id TEXT PRIMARY KEY, provider TEXT, source_id TEXT, title TEXT,
        first_seen_ms INTEGER, last_seen_ms INTEGER);
      CREATE TABLE IF NOT EXISTS forecast_definitions(
        id TEXT PRIMARY KEY, data TEXT NOT NULL);
      CREATE TABLE IF NOT EXISTS forecast_versions(
        id TEXT PRIMARY KEY, market_id TEXT, version INTEGER, definition_id TEXT,
        issued_ms INTEGER, observed_ms INTEGER, observation_seq INTEGER,
        probability REAL, bid REAL, ask REAL, depth_usd REAL, rule_revision INTEGER,
        method TEXT, policy TEXT NOT NULL, UNIQUE(market_id,version));
      CREATE INDEX IF NOT EXISTS forecast_history ON forecast_versions(market_id,version DESC);
      CREATE INDEX IF NOT EXISTS forecast_issued ON forecast_versions(issued_ms);
      CREATE TABLE IF NOT EXISTS forecast_heads(
        market_id TEXT PRIMARY KEY, version_id TEXT, confirmed_ms INTEGER,
        observed_ms INTEGER);
    """)


def definition(item):
    keys = (
        "id",
        "market_id",
        "event_id",
        "provider",
        "title",
        "resolution",
        "url",
        "topic",
        "symbols",
        "closes_ms",
    )
    data = {key: item.get(key, "" if key == "resolution" else None) for key in keys}
    wire = canonical(data)
    return hashlib.sha256(wire.encode()).hexdigest(), wire


def register(db, provider, items, at):
    for item in items:
        key = f"{provider}:{item['event_id']}"
        db.execute(
            "INSERT INTO event_entities VALUES(?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET "
            "title=CASE WHEN excluded.title=excluded.source_id THEN event_entities.title "
            "ELSE excluded.title END,last_seen_ms=excluded.last_seen_ms",
            (key, provider, item["event_id"], item.get("event_title") or item["event_id"], at, at),
        )


def quality(quote, config):
    return (
        0 <= quote["received_ms"] - quote["observed_ms"] <= config["max_cache_age_seconds"] * 1000
        and quote["spread_pp"] <= config["max_spread_pp"]
        and quote["depth_usd"] >= config["min_depth_usd"]
    )


def record(db, item, quote, config, sequence):
    """Called inside the observation transaction, only for new, ordered quotes."""
    now = quote["received_ms"]
    if now >= item["closes_ms"] or not quality(quote, config):
        return
    previous = db.execute(
        "SELECT f.* FROM forecast_heads h JOIN forecast_versions f ON f.id=h.version_id "
        "WHERE h.market_id=?",
        (item["id"],),
    ).fetchone()
    identity, wire = definition(item)
    changed = previous is None or previous["definition_id"] != identity
    if previous:
        elapsed = now - previous["issued_ms"]
        moved = abs(quote["probability"] - previous["probability"]) * 100
        changed |= previous["rule_revision"] != config["revision"]
        changed |= elapsed >= config["forecast_interval_seconds"] * 1000
        changed |= (
            elapsed >= config["forecast_min_interval_seconds"] * 1000
            and moved >= config["forecast_move_pp"]
        )
    if changed:
        version = previous["version"] + 1 if previous else 1
        version_id = hashlib.sha256(f"{item['id']}:{version}:{sequence}".encode()).hexdigest()
        db.execute("INSERT OR IGNORE INTO forecast_definitions VALUES(?,?)", (identity, wire))
        db.execute(
            "INSERT INTO forecast_versions VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
            (
                version_id,
                item["id"],
                version,
                identity,
                now,
                quote["observed_ms"],
                sequence,
                quote["probability"],
                quote["bid"],
                quote["ask"],
                quote["depth_usd"],
                config["revision"],
                METHOD,
                canonical(
                    {
                        key: config[key]
                        for key in (
                            "revision",
                            "max_cache_age_seconds",
                            "max_spread_pp",
                            "min_depth_usd",
                            "forecast_interval_seconds",
                            "forecast_min_interval_seconds",
                            "forecast_move_pp",
                        )
                    }
                ),
            ),
        )
    else:
        version_id = previous["id"]
    db.execute(
        "INSERT INTO forecast_heads VALUES(?,?,?,?) ON CONFLICT(market_id) DO UPDATE SET "
        "version_id=excluded.version_id,confirmed_ms=excluded.confirmed_ms,"
        "observed_ms=excluded.observed_ms",
        (item["id"], version_id, now, quote["observed_ms"]),
    )


def trim(db, cutoff):
    # Retain each head even after its observation/old versions age out.
    db.execute(
        "DELETE FROM forecast_versions WHERE issued_ms<? AND id NOT IN "
        "(SELECT version_id FROM forecast_heads)",
        (cutoff,),
    )
    db.execute(
        "DELETE FROM forecast_definitions WHERE id NOT IN "
        "(SELECT definition_id FROM forecast_versions)"
    )


def snapshot(db, now, config):
    rows = db.execute(
        "SELECT * FROM registry ORDER BY watched DESC,last_seen_ms DESC,id LIMIT ?", (LIMIT,)
    ).fetchall()
    groups = defaultdict(list)
    forecasts = []
    for row in rows:
        item = json.loads(row["data"])
        event_key = f"{item['provider']}:{item['event_id']}"
        latest_row = db.execute(
            "SELECT data FROM observations WHERE market_id=? ORDER BY observed_ms DESC LIMIT 1",
            (item["id"],),
        ).fetchone()
        quote = json.loads(latest_row[0]) if latest_row else None
        fresh = quote is not None and 0 <= now - quote["observed_ms"] <= max(
            90000, config["poll_seconds"] * 3000
        )
        state = (
            "closed"
            if now >= item["closes_ms"]
            else "archived"
            if not row["watched"]
            else "stale"
            if not fresh
            else "quality_blocked"
            if not quality(quote, config)
            else "watching"
        )
        head = db.execute(
            "SELECT f.*,h.confirmed_ms,h.observed_ms AS confirmed_observed_ms,"
            "d.data AS definition FROM forecast_heads h "
            "JOIN forecast_versions f ON f.id=h.version_id "
            "JOIN forecast_definitions d ON d.id=f.definition_id "
            "WHERE h.market_id=?",
            (item["id"],),
        ).fetchone()
        current_definition, _ = definition(item)
        if head:
            original = json.loads(head["definition"])
            forecast_state = (
                "invalidated"
                if head["definition_id"] != current_definition
                else "awaiting_outcome"
                if now >= original["closes_ms"]
                else "archived"
                if not row["watched"]
                else "quality_blocked"
                if state == "quality_blocked"
                else "stale"
                if not fresh
                or now - head["confirmed_observed_ms"] > max(90000, config["poll_seconds"] * 3000)
                else "current"
            )
            history = [
                dict(v)
                for v in db.execute(
                    "SELECT id,version,issued_ms,observed_ms,probability,"
                    "definition_id,rule_revision "
                    "FROM forecast_versions WHERE market_id=? ORDER BY version DESC LIMIT ?",
                    (item["id"], HISTORY_LIMIT),
                )
            ]
            forecasts.append(
                dict(
                    id=item["id"],
                    event_key=event_key,
                    provider=item["provider"],
                    title=original["title"],
                    topic=original["topic"],
                    symbols=original["symbols"],
                    resolution=original["resolution"],
                    url=original["url"],
                    closes_ms=original["closes_ms"],
                    state=forecast_state,
                    kind="market_implied",
                    method=head["method"],
                    version_id=head["id"],
                    version=head["version"],
                    definition_id=head["definition_id"],
                    issued_ms=head["issued_ms"],
                    observed_ms=head["observed_ms"],
                    confirmed_ms=head["confirmed_ms"],
                    probability=head["probability"],
                    current_probability=quote["probability"] if fresh else None,
                    current_observed_ms=quote["observed_ms"] if fresh else None,
                    bid=head["bid"],
                    ask=head["ask"],
                    depth_usd=head["depth_usd"],
                    observation_seq=head["observation_seq"],
                    rule_revision=head["rule_revision"],
                    policy=json.loads(head["policy"]),
                    outcome=None,
                    score=None,
                    history=history,
                )
            )
        groups[event_key].append(
            dict(
                id=item["id"],
                title=item["title"],
                closes_ms=item["closes_ms"],
                state=state,
                probability=quote["probability"] if quote else None,
                observed_ms=quote["observed_ms"] if quote else None,
                forecast_id=item["id"] if head else None,
                definition_id=current_definition,
                topic=item["topic"],
                symbols=item["symbols"],
            )
        )
    events = []
    for key, contracts in groups.items():
        entity = db.execute("SELECT * FROM event_entities WHERE id=?", (key,)).fetchone()
        # Existing journals gain event entities on their next discovery; never invent an old title.
        if not entity:
            continue
        events.append(
            dict(
                id=key,
                source_id=entity["source_id"],
                provider=entity["provider"],
                title=entity["title"],
                first_seen_ms=entity["first_seen_ms"],
                last_seen_ms=entity["last_seen_ms"],
                closes_ms=min(
                    (c["closes_ms"] for c in contracts if c["closes_ms"] > now),
                    default=max(c["closes_ms"] for c in contracts),
                ),
                state="closed"
                if all(c["state"] == "closed" for c in contracts)
                else "archived"
                if all(c["state"] in ("closed", "archived") for c in contracts)
                else "watching",
                topics=sorted({c["topic"] for c in contracts}),
                symbols=sorted({symbol for c in contracts for symbol in c["symbols"]}),
                contracts=contracts,
            )
        )
    return dict(
        schema_version=1,
        observed_ms=now,
        execution_authority=False,
        events=events,
        forecasts=forecasts,
        total_contracts=db.execute("SELECT COUNT(*) FROM registry").fetchone()[0],
        total_events=db.execute("SELECT COUNT(*) FROM event_entities").fetchone()[0],
        total_forecasts=db.execute("SELECT COUNT(*) FROM forecast_heads").fetchone()[0],
        retained_versions=db.execute("SELECT COUNT(*) FROM forecast_versions").fetchone()[0],
        retention_days=config["retention_days"],
        display_limit=LIMIT,
        issuance_interval_seconds=config["forecast_interval_seconds"],
        issuance_move_pp=config["forecast_move_pp"],
    )
