import asyncio
import json
from email.utils import formatdate
from pathlib import Path

import httpx
import pytest

from helios_alpha.crypto.event_journal import EventJournal, validate_config
from helios_alpha.crypto.event_sources import observe, registry_item

CONFIG = json.loads((Path(__file__).parents[1] / "config/events/radar-v1.json").read_text())
START = 1_790_000_000_000
ITEM = dict(
    id="polymarket:fixture",
    market_id="fixture",
    event_id="event",
    provider="polymarket",
    title="Will Bitcoin rise?",
    topic="bitcoin",
    symbols=["BTC-USD"],
    closes_ms=START + 86400_000,
    volume_24h=1000,
    volume_unit="USD",
    url="https://polymarket.com/event/fixture",
    token_id="123",
)


def sample(at, probability=0.5, **overrides):
    return dict(
        observed_ms=at,
        received_ms=at,
        changed_ms=at,
        probability=probability,
        bid=probability - 0.005,
        ask=probability + 0.005,
        depth_usd=1000,
        spread_pp=1,
        **overrides,
    )


def fill(journal, count=21, value=0.5, config=CONFIG):
    for i in range(count):
        journal.record(ITEM, sample(START + i * 30000, value), config)


def test_baseline_edge_trigger_hysteresis_and_restart(tmp_path):
    path = tmp_path / "events.sqlite3"
    journal = EventJournal(path)
    journal.register("polymarket", [ITEM], START)
    fill(journal)
    assert not journal.pending()
    at = START + 630000
    signal = journal.record(ITEM, sample(at, 0.57), CONFIG)
    assert signal["delta_pp"] == pytest.approx(7)
    assert signal["execution_authority"] is False
    assert journal.record(ITEM, sample(at, 0.57), CONFIG) is None
    journal.close()
    journal = EventJournal(path)
    assert journal.pending() == [signal]
    for i in range(22, 65):
        journal.record(ITEM, sample(START + i * 30000, 0.57), CONFIG)
    assert len(journal.pending()) == 1
    second = journal.record(ITEM, sample(START + 65 * 30000, 0.64), CONFIG)
    assert second and second["id"] != signal["id"]
    journal.delivered(signal["id"], 42)
    assert len(journal.pending()) == 1
    assert journal.snapshot(START + 65 * 30000, CONFIG)["triggers"][1]["stream_sequence"] == 42
    journal.close()


@pytest.mark.parametrize(
    "override", [{"spread_pp": 6}, {"depth_usd": 10}, {"received_ms": START + 800000}]
)
def test_quality_gate_blocks_probability_shocks(tmp_path, override):
    journal = EventJournal(tmp_path / "events.sqlite3")
    fill(journal)
    point = sample(START + 630000, 0.8)
    point.update(override)
    assert journal.record(ITEM, point, CONFIG) is None
    assert not journal.pending()
    assert journal.db.execute("SELECT state FROM rules").fetchone()[0] == "quality_blocked"


def test_gap_pause_and_revision_do_not_replay_a_shock(tmp_path):
    journal = EventJournal(tmp_path / "events.sqlite3")
    fill(journal)
    assert journal.record(ITEM, sample(START + 900000, 0.8), CONFIG) is None
    assert (
        journal.record(
            ITEM, sample(START + 930000, 0.9), {**CONFIG, "enabled": False, "revision": 2}
        )
        is None
    )
    assert journal.record(ITEM, sample(START + 960000, 0.95), {**CONFIG, "revision": 3}) is None
    assert not journal.pending()
    assert journal.db.execute("SELECT state FROM rules").fetchone()[0] == "warming_up"


def test_probabilities_are_not_merged_across_provider_contracts(tmp_path):
    journal = EventJournal(tmp_path / "events.sqlite3")
    journal.register("polymarket", [ITEM], START)
    other = {**ITEM, "id": "kalshi:fixture", "provider": "kalshi"}
    journal.register("kalshi", [other], START)
    fill(journal)
    journal.record(other, sample(START + 630000, 0.99), CONFIG)
    assert not journal.pending()
    snapshot = journal.snapshot(START + 900000, CONFIG)
    assert snapshot["registered_events"] == 2
    assert all(row["state"] == "stale" for row in snapshot["markets"])
    assert all("token_id" not in row for row in snapshot["markets"])


def test_live_adapter_preserves_book_math_and_cache_age(monkeypatch):
    monkeypatch.setattr(
        "helios_alpha.crypto.event_sources.time.time_ns", lambda: (START + 120000) * 1000000
    )

    async def scenario():
        async def handle(request):
            if request.url.host == "clob.polymarket.com":
                data = {
                    "timestamp": "1790000000000",
                    "bids": [{"price": ".4", "size": "1000"}, {"price": ".3", "size": "50"}],
                    "asks": [{"price": ".6", "size": "1000"}, {"price": ".5", "size": "1000"}],
                }
            else:
                data = {
                    "orderbook_fp": {
                        "yes_dollars": [["0.4", "1000"]],
                        "no_dollars": [["0.5", "1000"]],
                    }
                }
            return httpx.Response(
                200,
                json=data,
                headers={"age": "120", "date": formatdate(START / 1000, usegmt=True)},
            )

        async with httpx.AsyncClient(transport=httpx.MockTransport(handle)) as client:
            for item in [ITEM, {**ITEM, "provider": "kalshi"}]:
                row = await observe(client, item)
                assert row["probability"] == pytest.approx(0.45)
                assert row["spread_pp"] == pytest.approx(10)
                assert row["depth_usd"] == 400
                assert row["received_ms"] - row["observed_ms"] == 120000
                assert row["observed_ms"] == START

    asyncio.run(scenario())


def test_registry_names_exact_contract_and_rejects_closed_market():
    raw = dict(
        ticker="KXBTCD-TEST-T80000",
        title="Bitcoin price tomorrow?",
        yes_sub_title="$80,000 or above",
        close_time="2026-10-21T00:00:00Z",
        volume_24h_fp="50.00",
        event_ticker="KXBTCD-TEST",
        status="active",
        market_type="binary",
    )
    row = registry_item("kalshi", raw, CONFIG, 1790000000000)
    assert "$80,000 or above" in row["title"]
    assert row["volume_unit"] == "contracts"
    assert registry_item("kalshi", {**raw, "status": "closed"}, CONFIG, START) is None
    with pytest.raises(ValueError):
        validate_config({**CONFIG, "reset_pp": CONFIG["move_pp"]})


def test_late_delivery_does_not_rewind_or_fire_detector(tmp_path):
    journal = EventJournal(tmp_path / "events.sqlite3")
    fill(journal)
    point = sample(START + 610000, 0.51)
    journal.record(ITEM, point, CONFIG)
    before = tuple(journal.db.execute("SELECT * FROM rules").fetchone())
    late = sample(START + 605000, 0.9)
    late["received_ms"] = START + 630000
    assert journal.record(ITEM, late, CONFIG) is None
    assert tuple(journal.db.execute("SELECT * FROM rules").fetchone()) == before
    assert not journal.pending()
    assert journal.db.execute("SELECT COUNT(*) FROM observations").fetchone()[0] == 23


def test_bad_baseline_cannot_create_signal_and_revision_clears_display(tmp_path):
    journal = EventJournal(tmp_path / "events.sqlite3")
    journal.register("polymarket", [ITEM], START)
    bad = sample(START, 0.5)
    bad["spread_pp"] = 20
    journal.record(ITEM, bad, CONFIG)
    for i in range(1, 20):
        journal.record(ITEM, sample(START + i * 30000, 0.5), CONFIG)
    assert journal.record(ITEM, sample(START + 600000, 0.8), CONFIG) is None
    assert journal.snapshot(START + 600000, CONFIG)["markets"][0]["state"] == "quality_blocked"
    snapshot = journal.snapshot(START + 600000, {**CONFIG, "revision": 2})
    assert snapshot["markets"][0]["state"] == "warming_up"
    assert snapshot["markets"][0]["delta_pp"] is None
