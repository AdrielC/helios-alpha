import asyncio
import json
import random
from datetime import UTC, datetime
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import Mock

import httpx
import pytest

from helios_alpha.natgas import decisions
from helios_alpha.natgas.cli import fetch_source, run_replay
from helios_alpha.natgas.lab import Journal, settings, statistics, weather_revision
from helios_alpha.natgas.replay import (
    Contract,
    Plan,
    Quote,
    ReplayPolicy,
    expiration_scenarios,
    purged_folds,
    replay,
)
from helios_alpha.natgas.sources import parse_source

NOW = 1_800_000_000_000


@pytest.fixture
def lab(tmp_path):
    root = tmp_path / "private"
    root.mkdir(mode=0o700)
    journal = Journal(root)
    yield journal, settings(root)
    journal.close()


def contract(**changes):
    return Contract(
        **dict(
            id="test-call",
            symbol="UNG",
            kind="call",
            strike_cents=1000,
            multiplier=100,
            expiry="2027-02-19",
            close_by_ms=NOW + 86400000,
            available_ms=NOW - 1000,
            **changes,
        )
    )


def plan(at=NOW, id="p1", event="e1"):
    return Plan(
        id=id,
        event_id=event,
        evidence_ids=["fixture-evidence"],
        contract_id="test-call",
        event_available_ms=at,
        available_ms=at,
    )


def quote(offset, bid=39, ask=40, **changes):
    return Quote(
        **{
            **dict(
                contract_id="test-call",
                observed_ms=NOW + offset,
                available_ms=NOW + offset,
                bid_cents=bid,
                ask_cents=ask,
                bid_size=5,
                ask_size=5,
                regular_session=True,
            ),
            **changes,
        }
    )


def policy(config, **changes):
    return ReplayPolicy(**{**config["replay"], **changes})


def test_archive_is_immutable_deduplicated_private_and_point_in_time(lab):
    j, config = lab
    body = dict(kind="hdd", published_ms=NOW - 1000, daily={"2027-01-01": 1})
    identity = j.record("noaa-hdd", body, "public source fixture", NOW)
    assert j.record("noaa-hdd", body, "retrieved again", NOW + 100) == identity
    assert not j.evidence("noaa-hdd", NOW - 1)
    assert j.evidence("noaa-hdd", NOW + 100)[0]["available_ms"] == NOW
    assert (j.root / "natgas.sqlite3").stat().st_mode & 0o777 == 0o600
    j.close()
    j.db = Journal(j.root).db
    assert j.evidence("noaa-hdd", NOW)[0]["id"] == identity
    with pytest.raises(ValueError, match="future"):
        j.record("noaa-hdd", {**body, "published_ms": NOW + 1}, "invalid", NOW)
    assert j.snapshot(NOW, config)["execution_authority"] is False


def test_weather_uses_common_future_dates_and_ignores_rolled_window(lab):
    _, config = lab
    at = int(datetime(2027, 1, 2, tzinfo=UTC).timestamp() * 1000)
    prior = dict(
        id="a",
        kind="hdd",
        published_ms=at - 86400000,
        available_ms=at - 86000000,
        daily={f"2027-01-0{i}": 10 for i in range(1, 8)},
    )
    current = dict(
        id="b",
        kind="hdd",
        published_ms=at,
        available_ms=at + 1000,
        daily={**{f"2027-01-0{i}": 10 for i in range(2, 8)}, "2027-01-08": 100},
    )
    assert weather_revision(current, prior, config)["value"] == 0
    current["daily"]["2027-01-03"] = 20
    assert weather_revision(current, prior, config)["value"] == 10
    assert weather_revision(prior, current, config) is None


def test_eia_midnight_is_a_date_not_an_invented_release_instant():
    raw = dict(
        current_week="2026-10-09",
        release_date="2026-Oct-15 00:00:00",
        series=[
            {
                "series_id": "png.nw2_epg0_swo_r48_bcf.w",
                "data": [["2026-10-09", 3500]],
                "calculated": {
                    "net_change": 70,
                    "5yr-avg": 3400,
                    "revision_flag_net_change": "false",
                },
            }
        ],
    )
    event = parse_source("eia-storage", json.dumps(raw))
    assert event["published_ms"] is None
    assert "surprise" not in event
    assert event["seasonal_average_bcf"] == 3400


def test_noaa_parser_preserves_vintage_and_rejects_missing_days():
    raw = (
        "Product: Daily Heating Degree Days Forecast Based on NDFD Data Issued 0000UTC 20261020\n"
        "Weights: Population\nRegion|20261020|20261021|20261022|20261023|"
        "20261024|20261025|20261026|Total\n"
        "CONUS|7|7|7|8|9|7|7|52"
    )
    parsed = parse_source("noaa-hdd", raw)
    assert parsed["total"] == 52 and len(parsed["daily"]) == 7
    with pytest.raises(ValueError):
        parse_source("noaa-hdd", raw.replace("20261021|", ""))


def test_statistics_score_only_against_previous_samples():
    assert statistics([1] * 4, 99)["z_score"] is None
    assert statistics([1] * 5, 99)["z_score"] is None
    value = statistics([1, 2, 3, 4, 5], 3)
    assert value["z_score"] == 0 and value["mean"] == 3
    assert statistics([1, 2, 3, 4, 5], 999)["ewma"] == value["ewma"]


def test_realistic_entry_and_later_exit_include_spread_fees_and_latency(lab):
    _, config = lab
    ticks = [quote(999, 30, 31), quote(1000), quote(2000, 60, 61), quote(3000, 50, 51)]
    result = replay([contract()], ticks, [plan()], policy(config))
    trade = result["trades"][0]
    assert trade["entry_cost_cents"] == 4105
    assert trade["entry_ms"] == NOW + 1000
    assert trade["exit_ms"] == NOW + 3000  # Not the high quote that triggered the exit.
    assert trade["pnl_cents"] == 790  # 50-1 cents/share *100, minus both fees and entry.
    assert result["cash_cents"] == 60790


@pytest.mark.parametrize(
    "change",
    [
        dict(regular_session=False),
        dict(ask_size=0),
        dict(available_ms=NOW + 20000),
        dict(bid_cents=1),
    ],
)
def test_unfillable_or_stale_entry_cannot_become_a_trade(lab, change):
    _, config = lab
    assert not replay([contract()], [quote(1000, **change)], [plan()], policy(config))["trades"]


def test_unresolved_exit_is_retained_and_blocks_profit_claim_and_reentry(lab):
    _, config = lab
    result = replay(
        [contract()], [quote(1000)], [plan(), plan(NOW + 10000, "p2", "e2")], policy(config)
    )
    assert result["unresolved_trades"] == 1 and result["pnl_cents"] is None
    assert result["complete"] is False
    assert result["rejections"][0]["reason"] == "position_or_entry_already_pending"


def test_contract_must_be_known_when_decision_is_frozen(lab):
    _, config = lab
    future = contract().model_copy(update={"available_ms": NOW + 1})
    result = replay([future], [quote(1000)], [plan()], policy(config))
    assert result["rejections"][0]["reason"] == "contract_not_known_at_decision"


def test_future_quotes_do_not_change_a_closed_trade_and_duplicate_reads_are_not_samples(lab):
    _, config = lab
    ticks = [quote(1000), quote(2000, 60, 61), quote(3000, 50, 51)]
    before = replay([contract()], ticks, [plan()], policy(config))
    after = replay(
        [contract()], [*ticks, ticks[0], quote(100000, 1000, 1001)], [plan()], policy(config)
    )
    assert after == before
    with pytest.raises(ValueError, match="ambiguous"):
        replay([contract()], [quote(1000), quote(1000, 50, 51)], [plan()], policy(config))


def test_quote_record_is_deduplicated_and_metadata_cannot_silently_change(lab):
    journal, _ = lab
    cs, qs = [contract().model_dump()], [quote(1000).model_dump()]
    journal.record_options(cs, qs, NOW + 1000)
    journal.record_options(cs, [{**qs[0], "available_ms": NOW + 2000}], NOW + 2000)
    assert journal.db.execute("SELECT COUNT(*) FROM ng_quotes").fetchone()[0] == 1
    with pytest.raises(ValueError, match="metadata changed"):
        journal.record_options([{**cs[0], "multiplier": 10}], qs, NOW + 2000)


def test_purged_walk_forward_never_splits_events_or_overlapping_outcomes():
    events = [dict(event_id=str(i), start_ms=i * 100, end_ms=i * 100 + 40) for i in range(40)]
    events += [dict(event_id="19", start_ms=1900, end_ms=2200)]
    folds = purged_folds(events, warmup=20, test_size=10, embargo_ms=100)
    assert "19" not in folds[0]["train"] and "19" in folds[0]["purged"]
    assert set(folds[0]["train"]).isdisjoint(folds[0]["test"])
    assert folds[1]["test"] == [str(i) for i in range(30, 40)]


def test_expiration_upside_is_scenario_math_not_a_prediction():
    rows = expiration_scenarios(contract(), 40, 1000, [-2000, 0, 1000, 2000])
    assert [r["pnl_before_fees_cents"] for r in rows] == [-4000, -4000, 6000, 16000]
    assert rows[-1]["return_bps"] == 40000


def test_seeded_cost_monotonicity_and_long_option_loss_bound(lab):
    _, config = lab
    rng = random.Random(472)
    for _ in range(100):
        bid = rng.randint(20, 45)
        final = rng.randint(0, 200)
        ticks = [quote(1000, bid, bid + 1), quote(10000, final, max(1, final + 1))]
        p = policy(config, holding_ms=5000, take_profit_bps=100000, stop_loss_bps=10000)
        r = replay([contract()], ticks, [plan()], p)
        more = replay(
            [contract()], ticks, [plan()], p.model_copy(update={"fee_per_contract_cents": 10})
        )
        assert r["complete"] and more["complete"]
        assert more["pnl_cents"] <= r["pnl_cents"]
        assert r["pnl_cents"] >= -r["trades"][0]["entry_cost_cents"] - p.fee_per_contract_cents


def model_request(config):
    candidate = dict(
        candidate=True,
        available_ms=NOW,
        published_ms=NOW - 1000,
        evidence_ids=["a", "b"],
        kind="hdd",
        value=12,
    )
    evidence = [dict(id=id, available_ms=NOW) for id in ("a", "b")]
    return decisions.request_for(candidate, evidence, config, NOW), candidate, evidence


def receipt(request, choice="demand_up"):
    return {
        "Ok": {
            "receipt": {
                **{
                    k: request[k]
                    for k in ("schema_version", "request_id", "available_at_ms", "effective_at_ms")
                },
                "received_at_ms": NOW + 100,
                "outcome": {
                    "Ok": {
                        "model": "fixture-not-live",
                        "answers": {
                            "evidence_sound": {"type": "predicate", "probability": 0.99},
                            "demand": {
                                "type": "choice",
                                "choice": choice,
                                "confidence": 0.95,
                                "probabilities": [
                                    {"value": v, "probability": 1.0 if v == choice else 0.0}
                                    for v in sorted(decisions.CHOICES)
                                ],
                            },
                        },
                    }
                },
            }
        }
    }


def test_model_cannot_reverse_numeric_direction_or_become_a_market_forecast(lab):
    _, config = lab
    request, _, _ = model_request(config)
    result = decisions.classify_receipt(request, json.dumps(receipt(request)), NOW + 200)
    assert result["state"] == "research_candidate"
    assert result["calibrated_market_forecast"] is False
    assert result["execution_authority"] is False
    for choice in ("demand_down", "mixed", "insufficient"):
        assert (
            decisions.classify_receipt(request, json.dumps(receipt(request, choice)), NOW + 200)[
                "state"
            ]
            == "abstain"
        )


def test_model_rejects_future_evidence_unbound_late_and_invalid_answers(lab):
    _, config = lab
    request, candidate, evidence = model_request(config)
    with pytest.raises(ValueError):
        decisions.request_for(
            candidate, [dict(id="a", available_ms=NOW + 1), evidence[1]], config, NOW
        )
    wire = receipt(request)
    wire["Ok"]["receipt"]["request_id"] = "mismatch"
    with pytest.raises(ValueError):
        decisions.classify_receipt(request, json.dumps(wire), NOW + 200)
    with pytest.raises(ValueError):
        decisions.classify_receipt(request, json.dumps(receipt(request)), NOW + 9000)
    wire = receipt(request)
    wire["Ok"]["receipt"]["outcome"]["Ok"]["answers"]["evidence_sound"]["probability"] = True
    with pytest.raises(ValueError):
        decisions.classify_receipt(request, json.dumps(wire), NOW + 200)


def test_disabled_model_is_free_and_local_fallback_cannot_admit(lab):
    journal, config = lab
    store = SimpleNamespace(command=Mock())
    _, candidate, evidence = model_request(config)
    result = asyncio.run(
        decisions.review(journal, store, config, candidate, evidence, Path("/missing"))
    )
    store.command.assert_not_called()
    assert result["state"] == "abstain"


def test_synthetic_replay_remains_labeled_and_compares_every_exit_profile(lab):
    _, config = lab
    report = run_replay(
        dict(
            data_class="synthetic",
            provenance="unit test fixture",
            contracts=[contract().model_dump()],
            plans=[plan().model_dump()],
            quotes=[quote(1000).model_dump()],
        ),
        config,
    )
    assert report["data_class"] == "synthetic" and report["profitability_established"] is False
    assert len(report["exit_profile_sensitivity"]) == 3
    assert report["pnl_cents"] is None


def test_public_source_redirects_stay_on_official_hosts():
    calls = []

    async def check(destination):
        def handler(request):
            calls.append(str(request.url))
            if request.url.path == "/ngs/wngsr.json":
                return httpx.Response(302, headers={"location": destination})
            return httpx.Response(200, content=b"\xef\xbb\xbf{}")

        async with httpx.AsyncClient(transport=httpx.MockTransport(handler)) as client:
            return await fetch_source(client, "https://ir.eia.gov/ngs/wngsr.json")

    assert asyncio.run(check("/secure/ngs/wngsr.json")) == "{}"
    with pytest.raises(ValueError, match="unexpected source redirect"):
        asyncio.run(check("https://untrusted.example/anything"))
    assert not any("untrusted" in url for url in calls)


def test_option_collector_is_read_only_dynamic_and_restart_throttled(lab, monkeypatch):
    from helios_alpha.natgas import broker

    journal, _ = lab
    at = int(datetime(2026, 10, 21, tzinfo=UTC).timestamp() * 1000)
    monkeypatch.setattr(broker, "regular_session", lambda *_: True)
    used = []

    class Session:
        async def call_tool(self, name, args):
            used.append(name)
            if name == "get_equity_quotes":
                data = {"results": [{"quote": {"last_trade_price": "10.00"}}]}
            elif name == "get_option_chains":
                data = {
                    "chains": [
                        {
                            "id": "chain",
                            "can_open_position": True,
                            "expiration_dates": ["2026-10-23", "2026-11-13"],
                            "late_close_state": "enabled",
                        }
                    ]
                }
            elif name == "get_option_instruments":
                assert args["expiration_dates"] == "2026-11-13"
                data = {
                    "instruments": [
                        {
                            "id": f"option-{kind}",
                            "chain_id": "chain",
                            "type": kind,
                            "tradability": "tradable",
                            "strike_price": "10.00",
                            "expiration_date": "2026-11-13",
                            "trade_value_multiplier": "100",
                            "sellout_datetime": "2026-11-13T20:45:00Z",
                        }
                        for kind in ("call", "put")
                    ]
                }
            elif name == "get_option_quotes":
                data = {
                    "results": [
                        {
                            "quote": {
                                "instrument_id": id,
                                "bid_price": "0.30",
                                "ask_price": "0.40",
                                "bid_size": 1,
                                "ask_size": 2,
                                "updated_at": "2026-10-20T20:00:00Z",
                            }
                        }
                        for id in args["instrument_ids"]
                    ]
                }
            else:
                raise AssertionError("unexpected broker call")
            return SimpleNamespace(isError=False, structuredContent={"data": data})

    asyncio.run(broker.capture(Session(), journal.root, lambda: at))
    assert set(used) == broker.READ_TOOLS
    assert journal.db.execute("SELECT COUNT(*) FROM ng_quotes").fetchone()[0] == 2
    prior_calls = len(used)
    asyncio.run(broker.capture(Session(), journal.root, lambda: at + 1000))
    assert len(used) == prior_calls
    snapshot = journal.snapshot(at, settings(journal.root))
    assert snapshot["premiums"][0]["premium_cents"] == 4000
    assert not snapshot["premiums"][0]["fresh"]


def test_regular_session_uses_exchange_calendar_and_late_close():
    from helios_alpha.natgas.broker import milliseconds, regular_session

    assert regular_session(milliseconds("2026-10-20T14:00:00Z"), False)
    assert not regular_session(milliseconds("2026-10-20T12:00:00Z"), True)
    assert not regular_session(milliseconds("2026-10-17T14:00:00Z"), True)
    assert regular_session(milliseconds("2026-10-20T20:10:00Z"), True)
    assert not regular_session(milliseconds("2026-10-20T20:10:00Z"), False)
