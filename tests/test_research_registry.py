"""Causal issuance, durable versions and explicit unresolved outcomes."""

import json
from pathlib import Path

import pytest

from helios_alpha.crypto.event_journal import EventJournal, validate_config
from helios_alpha.crypto.research_registry import snapshot

CONFIG = validate_config(
    json.loads((Path(__file__).parents[1] / "config/events/radar-v1.json").read_text())
)
START = 1_790_000_000_000
ITEM = dict(
    id="kalshi:btc",
    market_id="btc",
    event_id="event",
    event_title="Bitcoin tomorrow",
    provider="kalshi",
    title="Bitcoin above $80,000?",
    resolution="Fixture settlement terms",
    url="https://kalshi.com/markets/test",
    topic="bitcoin",
    symbols=["BTC-USD"],
    closes_ms=START + 86400_000,
    volume_24h=1000,
    volume_unit="contracts",
)


def quote(at, p=0.5, **updates):
    return (
        dict(
            observed_ms=at,
            received_ms=at,
            changed_ms=None,
            probability=p,
            bid=p - 0.005,
            ask=p + 0.005,
            depth_usd=1000,
            spread_pp=1,
        )
        | updates
    )


def open_journal(tmp_path):
    journal = EventJournal(tmp_path / "events.sqlite3")
    journal.register("kalshi", [ITEM], START)
    return journal


def test_forecasts_are_immutable_quality_gated_and_rate_limited(tmp_path):
    journal = open_journal(tmp_path)
    journal.record(ITEM, quote(START, depth_usd=1), CONFIG)
    assert snapshot(journal.db, START, CONFIG)["forecasts"] == []
    journal.record(ITEM, quote(START + 30000), CONFIG)
    first = snapshot(journal.db, START + 30000, CONFIG)["forecasts"][0]
    assert first["version"] == 1
    journal.record(ITEM, quote(START + 60000, 0.54), CONFIG)
    unchanged = snapshot(journal.db, START + 60000, CONFIG)["forecasts"][0]
    assert unchanged["issued_ms"] == first["issued_ms"]
    assert unchanged["probability"] == 0.5
    assert unchanged["current_probability"] == 0.54
    assert unchanged["confirmed_ms"] == START + 60000
    journal.record(ITEM, quote(START + 90000, 0.54), CONFIG)
    moved = snapshot(journal.db, START + 90000, CONFIG)["forecasts"][0]
    assert moved["version"] == 2
    assert moved["history"][1]["probability"] == 0.5
    assert moved["policy"]["max_spread_pp"] == CONFIG["max_spread_pp"]
    journal.record(ITEM, quote(START + 390000, 0.54), CONFIG)
    assert snapshot(journal.db, START + 390000, CONFIG)["forecasts"][0]["version"] == 3
    # Duplicate and late deliveries cannot change the head or confirmation time.
    journal.record(ITEM, quote(START + 390000, 0.6), CONFIG)
    journal.record(ITEM, quote(START + 70000, 0.7, received_ms=START + 400000), CONFIG)
    head = snapshot(journal.db, START + 400000, CONFIG)["forecasts"][0]
    assert head["version"] == 3
    assert head["confirmed_ms"] == START + 390000
    assert head["probability"] == 0.54
    journal.close()


def test_provider_identity_terms_revisions_and_restart(tmp_path):
    journal = open_journal(tmp_path)
    other = dict(ITEM, provider="polymarket", id="polymarket:btc")
    journal.register("polymarket", [other], START)
    journal.record(ITEM, quote(START), CONFIG)
    journal.record(other, quote(START), CONFIG)
    data = snapshot(journal.db, START, CONFIG)
    assert {e["id"] for e in data["events"]} == {"kalshi:event", "polymarket:event"}
    old = next(f for f in data["forecasts"] if f["provider"] == "kalshi")
    updated = dict(ITEM, title="Bitcoin above $90,000?", resolution="Revised settlement")
    journal.register("kalshi", [updated], START + 30000)
    stale = next(
        f
        for f in snapshot(journal.db, START + 30000, CONFIG)["forecasts"]
        if f["provider"] == "kalshi"
    )
    assert stale["state"] == "invalidated"
    assert stale["title"] == ITEM["title"]
    assert stale["resolution"] == ITEM["resolution"]
    journal.record(updated, quote(START + 30000), CONFIG)
    journal.close()
    journal = EventJournal(tmp_path / "events.sqlite3")
    revised = next(
        f
        for f in snapshot(journal.db, START + 30000, CONFIG)["forecasts"]
        if f["provider"] == "kalshi"
    )
    assert revised["version"] == 2
    assert revised["definition_id"] != old["definition_id"]
    assert revised["history"][1]["definition_id"] == old["definition_id"]
    original = journal.db.execute(
        "SELECT data FROM forecast_definitions WHERE id=?", (old["definition_id"],)
    ).fetchone()[0]
    assert json.loads(original)["title"] == ITEM["title"]
    # Failed title enrichment must not replace a known title with a ticker.
    journal.register("kalshi", [dict(updated, event_title="event")], START + 60000)
    assert (
        next(
            e
            for e in snapshot(journal.db, START + 60000, CONFIG)["events"]
            if e["provider"] == "kalshi"
        )["title"]
        == ITEM["event_title"]
    )
    journal.close()


def test_stale_blocked_archived_and_unresolved_are_not_scored(tmp_path):
    journal = open_journal(tmp_path)
    journal.record(ITEM, quote(START), CONFIG)
    assert snapshot(journal.db, START + 100000, CONFIG)["forecasts"][0]["state"] == "stale"
    journal.record(ITEM, quote(START + 120000, spread_pp=10), CONFIG)
    f = snapshot(journal.db, START + 120000, CONFIG)["forecasts"][0]
    assert f["state"] == "quality_blocked" and f["version"] == 1
    journal.register("kalshi", [], START + 130000)
    assert snapshot(journal.db, START + 130000, CONFIG)["forecasts"][0]["state"] == "archived"
    after = ITEM["closes_ms"] + 1
    journal.record(ITEM, quote(after, 0.9), CONFIG)
    f = snapshot(journal.db, after, CONFIG)["forecasts"][0]
    assert f["state"] == "awaiting_outcome"
    assert f["outcome"] is None and f["score"] is None and f["version"] == 1
    journal.trim(after + 40 * 86400_000, CONFIG["retention_days"])
    retained = snapshot(journal.db, after + 40 * 86400_000, CONFIG)
    assert retained["total_forecasts"] == 1
    assert retained["retained_versions"] == 1
    assert retained["forecasts"][0]["definition_id"] == f["definition_id"]
    assert retained["execution_authority"] is False
    journal.close()


def test_legacy_config_receives_issuance_defaults_without_changing_revision():
    old = {k: v for k, v in CONFIG.items() if not k.startswith("forecast_")}
    upgraded = validate_config(old)
    assert upgraded["revision"] == old["revision"]
    assert upgraded["forecast_min_interval_seconds"] <= upgraded["forecast_interval_seconds"]
    with pytest.raises(ValueError):
        validate_config(
            dict(upgraded, forecast_min_interval_seconds=600, forecast_interval_seconds=60)
        )


def test_large_registry_projection_keeps_private_file_checks(tmp_path):
    from helios_alpha.control.golem_store import read_private_json
    from helios_alpha.control.store import ControlError

    path = tmp_path / "registry.json"
    path.write_text(json.dumps({"public_terms": "x" * 1_100_000}))
    path.chmod(0o600)
    with pytest.raises(ControlError, match="size bound"):
        read_private_json(path)
    assert len(read_private_json(path, max_bytes=4 * 1024 * 1024)["public_terms"]) == 1_100_000
    path.chmod(0o644)
    with pytest.raises(ControlError, match="private"):
        read_private_json(path, max_bytes=4 * 1024 * 1024)
