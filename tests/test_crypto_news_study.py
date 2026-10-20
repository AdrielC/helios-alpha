import copy
import json
from types import SimpleNamespace
from unittest.mock import Mock

import pytest

from helios_alpha.control.store import ControlError
from helios_alpha.crypto.evidence import HORIZONS_MS, MAX_LAG_MS, NewsEvidence, reference_return_bps
from helios_alpha.crypto.news_study import read_news_study, summarize_candidates

NOW = 1_800_000_000_000
FINGERPRINT = "f" * 64
COSTS = {"fee_bps": 95, "slippage_bps": 5}


def quote(at, price=100_000_000):
    return {
        "bid_micros": price,
        "ask_micros": price + 10_000,
        "observed_ms": at,
        "received_ms": at + 10,
        "channel": "level2_batch",
    }


def candidate():
    entry = quote(NOW - 10)
    points = []
    for horizon in HORIZONS_MS:
        exit_quote = quote(NOW + horizon, 103_000_000)
        points.append(
            {
                "horizon_ms": horizon,
                "due_ms": NOW + horizon,
                "recorded_ms": NOW + horizon + 10,
                "status": "observed",
                "quote": exit_quote,
                "reference_return_bps_after_costs": reference_return_bps(entry, exit_quote, COSTS),
            }
        )
    return {
        "strategy_fingerprint": FINGERPRINT,
        "symbol": "BTC-USD",
        "news": {
            "news_id": "a" * 64,
            "synthetic": False,
            "observed_ms": NOW,
            "published_ms": NOW - 1000,
            "eligible": True,
            "title": "Synthetic fixture for explicit provenance checks",
            "source": "example",
        },
        "synthetic_inputs": False,
        "reference_source": "coinbase",
        "costs": COSTS.copy(),
        "reference_at_ms": NOW,
        "reference": entry,
        "markouts": points,
        "capture_status": "complete",
        "transitions": [
            {
                "at_ms": NOW,
                "decision": {"stage": "evaluation", "action": "hold", "reason": "no_confirmation"},
            }
        ],
        "timing": {"capture_started_ms": NOW, "first_evaluation_ms": NOW},
        "omitted_transitions": 0,
    }


def completed_at():
    return NOW + HORIZONS_MS[-1] + MAX_LAG_MS + 1


def test_versions_assets_costs_and_provenance_are_never_pooled():
    base = candidate()
    changed_version = copy.deepcopy(base)
    changed_version["strategy_fingerprint"] = "b" * 64
    changed_symbol = copy.deepcopy(base)
    changed_symbol["symbol"] = "ETH-USD"
    changed_fee = copy.deepcopy(base)
    changed_fee["costs"]["fee_bps"] = 0
    for p in changed_fee["markouts"]:
        p["reference_return_bps_after_costs"] = reference_return_bps(
            changed_fee["reference"], p["quote"], changed_fee["costs"]
        )
    legacy = copy.deepcopy(base)
    del legacy["news"]["synthetic"]
    synthetic_news = copy.deepcopy(base)
    synthetic_news["news"]["synthetic"] = True
    synthetic_quote = copy.deepcopy(base)
    synthetic_quote["markouts"][0]["quote"]["channel"] = "synthetic"
    values = [
        base,
        changed_version,
        changed_symbol,
        changed_fee,
        legacy,
        synthetic_news,
        synthetic_quote,
    ]
    before = copy.deepcopy(values)
    groups = summarize_candidates(values, completed_at())
    assert len(groups) == 6
    assert groups == summarize_candidates(reversed(values), completed_at())
    assert values == before
    synthetic = next(g for g in groups if g["provenance"] == "synthetic")
    assert synthetic["candidate_count"] == 2
    assert synthetic["unique_headlines"] == 1
    ordinary = next(
        g
        for g in groups
        if g["provenance"] == "explicit_non_synthetic"
        and g["strategy_fingerprint"] == FINGERPRINT
        and g["symbol"] == "BTC-USD"
        and g["costs"] == COSTS
    )
    point = ordinary["horizons"][0]
    assert (
        point["gross_reference_returns"]["mean_bps"]
        > point["after_cost_reference_returns"]["mean_bps"]
    )
    assert point["after_cost_reference_returns"]["positive"] == 1
    assert ordinary["candidates_with_recorded_entry_proposal"] == 0
    assert ordinary["candidates_with_recorded_paper_fill"] == 0


def test_proposals_are_attributed_only_to_the_selected_headline():
    selected, other = candidate(), candidate()
    other["news"]["news_id"] = "b" * 64
    decision = {
        "stage": "evaluation",
        "action": "hold",
        "reason": "model_pending",
        "proposed_action": "enter",
        "selected_news_id": selected["news"]["news_id"],
    }
    for row in (selected, other):
        row["transitions"] = [{"at_ms": NOW, "decision": decision.copy()}]
    selected["timing"]["first_entry_proposal_ms"] = NOW
    selected["timing"]["first_fill_observed_ms"] = NOW + 100
    selected["transitions"] = []  # The first milestones survive transition eviction.
    selected["omitted_transitions"] = 100
    del other["timing"]  # Legacy absence cannot be claimed as complete lifecycle tracking.
    group = summarize_candidates([selected, other], completed_at())[0]
    assert group["candidate_count"] == group["unique_headlines"] == 2
    assert group["candidates_with_recorded_entry_proposal"] == 1
    assert group["candidates_with_recorded_paper_fill"] == 1
    assert group["candidates_without_first_milestone_tracking"] == 1
    assert group["first_recorded_evaluation_reasons"] == {"model_pending": 1, "unknown": 1}


@pytest.mark.parametrize(
    "bad", ["old_quote", "future", "late", "wrong_due", "wrong_return", "duplicate"]
)
def test_invalid_historical_measurements_are_excluded_not_rewritten(bad):
    row = candidate()
    point = row["markouts"][0]
    if bad == "old_quote":
        point["quote"]["observed_ms"] = point["due_ms"] - 1
    elif bad == "future":
        point["quote"]["received_ms"] = point["recorded_ms"] + 1
    elif bad == "late":
        point["recorded_ms"] = point["due_ms"] + MAX_LAG_MS + 1
    elif bad == "wrong_due":
        point["due_ms"] += 1
    elif bad == "wrong_return":
        point["reference_return_bps_after_costs"] += 100
    else:
        row["markouts"].append(copy.deepcopy(point))
    before = copy.deepcopy(row)
    summary = summarize_candidates([row], completed_at())[0]["horizons"][0]
    assert summary["states"] == {"invalid": 1}
    assert summary["after_cost_reference_returns"]["count"] == 0
    assert summary["after_cost_reference_returns"]["mean_bps"] is None
    assert row == before


def test_pending_overdue_missing_and_missing_reference_stay_distinct():
    row = candidate()
    row["markouts"] = []
    horizon = HORIZONS_MS[0]
    group = summarize_candidates([row], NOW + horizon)[0]
    assert group["horizons"][0]["states"] == {"pending": 1}
    group = summarize_candidates([row], completed_at())[0]
    assert group["horizons"][0]["states"] == {"overdue_unrecorded": 1}
    row["markouts"] = [
        {
            "horizon_ms": horizon,
            "due_ms": NOW + horizon,
            "recorded_ms": NOW + horizon + MAX_LAG_MS + 1,
            "status": "missing",
        }
    ]
    group = summarize_candidates([row], completed_at())[0]
    assert group["horizons"][0]["states"] == {"missing": 1}
    row["reference_at_ms"] = row["reference"] = None
    row["capture_status"] = "missing_reference"
    group = summarize_candidates([row], completed_at())[0]
    assert group["horizons"][0]["states"] == {"missing_reference": 1}
    assert all(h["after_cost_reference_returns"]["mean_bps"] is None for h in group["horizons"])


def test_unchanged_market_is_a_loss_after_costs():
    row = candidate()
    for point in row["markouts"]:
        point["quote"] = quote(point["due_ms"])
        point["reference_return_bps_after_costs"] = reference_return_bps(
            row["reference"], point["quote"], COSTS
        )
    point = summarize_candidates([row], completed_at())[0]["horizons"][0]
    assert point["gross_reference_returns"]["mean_bps"] == -1
    assert point["after_cost_reference_returns"]["mean_bps"] < -190
    assert point["after_cost_reference_returns"]["positive"] == 0


def test_reader_uses_all_retained_rows_filters_versions_and_never_mutates(tmp_path, monkeypatch):
    root = tmp_path / "private"
    root.mkdir(mode=0o700)
    recorder = NewsEvidence(root)
    store = SimpleNamespace(root=root, status=Mock(return_value={}))
    monkeypatch.setattr(
        "helios_alpha.crypto.news_study.time.time_ns", lambda: completed_at() * 1_000_000
    )
    try:
        sample = candidate()
        for version in (FINGERPRINT, "b" * 64):
            recorder.observe(
                [sample["news"]],
                {"BTC-USD": sample["reference"]},
                ["BTC-USD"],
                version,
                COSTS,
                1,
                NOW,
            )
        recorder.observe(
            [],
            {"BTC-USD": quote(completed_at() - 10)},
            ["BTC-USD"],
            FINGERPRINT,
            COSTS,
            1,
            completed_at(),
        )
        before = list(recorder.db.iterdump())
        all_versions = read_news_study(store, "observer")
        assert all_versions["candidate_count"] == 2
        assert all_versions["unique_headlines"] == 1
        assert len(all_versions["groups"]) == 2
        assert all_versions["not_strategy_pnl"]
        assert not all_versions["profitability_established"]
        selected = read_news_study(store, "observer", strategy_fingerprint=FINGERPRINT)
        assert selected["candidate_count"] == selected["unique_headlines"] == 1
        assert selected["groups"][0]["strategy_fingerprint"] == FINGERPRINT
        assert selected["evidence_sha256"] != all_versions["evidence_sha256"]
        assert read_news_study(store, "observer") == all_versions
        assert list(recorder.db.iterdump()) == before
        assert json.dumps(all_versions)
        store.status.side_effect = ControlError("revoked")
        with pytest.raises(ControlError, match="revoked"):
            read_news_study(store, "observer")
    finally:
        recorder.close()


@pytest.mark.parametrize("runtime", [{}, [], None])
def test_absent_recording_is_unknown_without_creating_files(tmp_path, runtime):
    root = tmp_path / "private"
    root.mkdir(mode=0o700)
    store = SimpleNamespace(root=root, status=Mock(return_value={"runtime": runtime}))
    result = read_news_study(store, "observer")
    assert result["state"] == "not_recording"
    assert result["groups"] == []
    assert "candidate_count" not in result
    assert list(root.iterdir()) == []
    with pytest.raises(ControlError, match="fingerprint"):
        read_news_study(store, "observer", strategy_fingerprint="invalid")


def test_reader_rejects_nonprivate_database(tmp_path):
    root = tmp_path / "private"
    root.mkdir(mode=0o700)
    recorder = NewsEvidence(root)
    recorder.close()
    (root / "news-evidence.sqlite3").chmod(0o644)
    with pytest.raises(ControlError, match="private"):
        read_news_study(SimpleNamespace(root=root, status=Mock(return_value={})), "observer")
