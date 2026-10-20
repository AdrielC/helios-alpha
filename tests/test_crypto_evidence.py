import json
from types import SimpleNamespace
from unittest.mock import Mock

import pytest

from helios_alpha.control.store import ControlError
from helios_alpha.crypto.evidence import (
    HORIZONS_MS,
    MAX_LAG_MS,
    NewsEvidence,
    read_news_evidence,
    reference_return_bps,
)

NOW = 1_800_000_000_000
FINGERPRINT = "f" * 64
COSTS = {"fee_bps": 95, "slippage_bps": 5}
STORY = {
    "news_id": "a" * 64,
    "source": "synthetic",
    "title": "Synthetic test event",
    "observed_ms": NOW,
    "published_ms": NOW - 1000,
    "eligible": True,
    "synthetic": True,
}


def quote(at, price=100_000_000):
    return {"bid_micros": price, "ask_micros": price + 10_000, "observed_ms": at, "received_ms": at}


@pytest.fixture
def evidence(tmp_path):
    root = tmp_path / "private"
    root.mkdir(mode=0o700)
    journal = NewsEvidence(root)
    store = SimpleNamespace(root=root, status=Mock(return_value={"paper": {"recent_fills": []}}))
    yield root, journal, store
    journal.close()


def seed(journal, news=None, at=NOW):
    journal.observe(
        [STORY] if news is None else news,
        {"BTC-USD": quote(at)},
        ["BTC-USD"],
        FINGERPRINT,
        COSTS,
        1,
        at,
    )


def records(store):
    return read_news_evidence(store, "observer")["evidence"]["candidates"]


def test_official_source_facts_are_frozen_with_the_original_news(evidence):
    _, journal, store = evidence
    story = {**STORY, "source_facts": {"kind": "fomc_target_range", "action": "lower"}}
    seed(journal, [story])
    story["source_facts"]["action"] = "raise"
    seed(journal, [story], at=NOW + 1000)
    assert records(store)[0]["news"]["source_facts"]["action"] == "lower"


def test_model_evidence_binding_survives_a_new_reader(evidence):
    root, journal, store = evidence
    seed(journal)
    transition = {
        "stage": "review_started",
        "review_id": "d" * 64,
        "evidence_news_ids": ["a" * 64, "b" * 64],
        "review_expires_ms": NOW + 1000,
    }
    journal.record([STORY["news_id"]], "BTC-USD", FINGERPRINT, transition, NOW)
    restarted = NewsEvidence(root)
    try:
        assert records(store)[0]["transitions"][-1]["decision"] == transition
    finally:
        restarted.close()


@pytest.mark.parametrize(
    "change",
    [
        {"review_id": "invalid"},
        {"evidence_news_ids": ["a" * 64, "a" * 64]},
        {"evidence_news_ids": ["a" * 64] * 9},
        {"review_expires_ms": True},
        {"review_expires_ms": 0},
    ],
)
def test_bad_review_evidence_binding_is_rejected(evidence, change):
    _, journal, _ = evidence
    seed(journal)
    transition = {
        "stage": "review_started",
        "review_id": "d" * 64,
        "evidence_news_ids": ["a" * 64],
        "review_expires_ms": NOW + 1000,
        **change,
    }
    with pytest.raises(ValueError, match="review evidence binding"):
        journal.record([STORY["news_id"]], "BTC-USD", FINGERPRINT, transition, NOW)


def test_reference_and_costs_are_frozen_before_outcomes(evidence):
    _, journal, store = evidence
    seed(journal)
    journal.record(
        [STORY["news_id"]],
        "BTC-USD",
        FINGERPRINT,
        {"stage": "evaluation", "action": "hold", "reason": "unconfirmed"},
        NOW,
    )
    at = NOW + HORIZONS_MS[0]
    journal.observe(
        [{**STORY, "title": "Rewritten later"}],
        {"BTC-USD": quote(at)},
        ["BTC-USD"],
        FINGERPRINT,
        {"fee_bps": 0, "slippage_bps": 0},
        2,
        at,
    )
    record = records(store)[0]
    assert record["news"] == STORY
    assert record["synthetic_inputs"] is True
    assert record["reference_source"] == "synthetic"
    assert record["costs"] == COSTS
    assert record["policy_revision_at_capture"] == 1
    assert record["reference_at_ms"] == NOW
    assert record["transitions"][0]["decision"]["action"] == "hold"
    point = record["markouts"][0]
    assert point["status"] == "observed"
    assert point["reference_return_bps_after_costs"] == reference_return_bps(
        quote(NOW), quote(at), COSTS
    )
    assert point["reference_return_bps_after_costs"] < 0
    assert record["matches_in_recent_golem_paper_fills"] == []


def test_restart_preserves_reference_and_marks_missed_windows(evidence):
    root, journal, store = evidence
    seed(journal)
    # A second connection models a new process without relying on in-memory state.
    restarted = NewsEvidence(root)
    try:
        at = NOW + HORIZONS_MS[-1] + MAX_LAG_MS + 1
        restarted.observe(
            [],
            {"BTC-USD": quote(at, 900_000_000)},
            ["BTC-USD"],
            FINGERPRINT,
            COSTS,
            1,
            at,
        )
        record = records(store)[0]
        assert record["capture_status"] == "complete"
        assert len(record["markouts"]) == len(HORIZONS_MS)
        assert all(point["status"] == "missing" for point in record["markouts"])
        assert all("reference_return_bps_after_costs" not in p for p in record["markouts"])
        # Even a later, much higher price cannot change a missed measurement.
        restarted.observe(
            [],
            {"BTC-USD": quote(at + 1000, 2_000_000_000)},
            ["BTC-USD"],
            FINGERPRINT,
            COSTS,
            1,
            at + 1000,
        )
        assert records(store)[0] == record
    finally:
        restarted.close()


def test_pre_deadline_quote_cannot_fill_a_forward_measurement(evidence):
    _, journal, store = evidence
    seed(journal)
    due = NOW + HORIZONS_MS[0]
    journal.observe(
        [],
        {"BTC-USD": quote(due - 1)},
        ["BTC-USD"],
        FINGERPRINT,
        COSTS,
        1,
        due,
    )
    assert not records(store)[0]["markouts"]
    journal.observe(
        [],
        {"BTC-USD": quote(due + 1000)},
        ["BTC-USD"],
        FINGERPRINT,
        COSTS,
        1,
        due + 1000,
    )
    point = records(store)[0]["markouts"][0]
    assert point["status"] == "observed"
    assert point["recorded_ms"] >= point["quote"]["received_ms"] >= point["due_ms"]


def test_delayed_delivery_does_not_turn_an_old_price_into_a_future_measurement(evidence):
    _, journal, store = evidence
    seed(journal)
    due = NOW + HORIZONS_MS[0]
    delayed = {**quote(due - 1), "received_ms": due + 100}
    journal.observe([], {"BTC-USD": delayed}, ["BTC-USD"], FINGERPRINT, COSTS, 1, due + 100)
    assert records(store)[0]["markouts"] == []
    journal.observe(
        [], {"BTC-USD": quote(due + 200)}, ["BTC-USD"], FINGERPRINT, COSTS, 1, due + 200
    )
    point = records(store)[0]["markouts"][0]
    assert point["quote"]["observed_ms"] == due + 200
    assert point["due_ms"] <= point["quote"]["observed_ms"] <= point["quote"]["received_ms"]


def test_stale_and_baseline_news_are_not_retroactively_measured(evidence):
    _, journal, store = evidence
    seed(
        journal,
        [
            {**STORY, "eligible": False},
            {**STORY, "news_id": "b" * 64, "published_ms": NOW - 120_001},
            {**STORY, "news_id": "c" * 64, "published_ms": NOW + 1},
        ],
    )
    assert not records(store)


def test_absent_or_future_quote_leaves_an_explicit_missing_reference(evidence):
    _, journal, store = evidence
    journal.observe(
        [STORY],
        {"BTC-USD": quote(NOW + 1)},
        ["BTC-USD"],
        FINGERPRINT,
        COSTS,
        1,
        NOW,
    )
    assert records(store)[0]["reference"] is None
    journal.observe([], {}, ["BTC-USD"], FINGERPRINT, COSTS, 1, NOW + 120_001)
    record = records(store)[0]
    assert record["capture_status"] == "missing_reference"
    assert record["reference"] is None
    assert record["markouts"] == []


def test_transitions_deduplicate_and_correlate_only_authoritative_fills(evidence):
    _, journal, store = evidence
    seed(journal)
    transition = {"stage": "submission", "event_id": "c" * 64, "action": "enter"}
    for i in range(5):
        journal.record([STORY["news_id"]], "BTC-USD", FINGERPRINT, transition, NOW + i)
    record = records(store)[0]
    assert len(record["transitions"]) == 1
    assert record["matches_in_recent_golem_paper_fills"] == []
    fill = {"event_id": "c" * 64, "side": "buy", "simulation": True}
    store.status.return_value = {
        "paper": {
            "recent_fills": [
                fill,
                {"event_id": "d" * 64, "side": "buy", "simulation": True},
            ]
        }
    }
    assert records(store)[0]["matches_in_recent_golem_paper_fills"] == [fill]


def test_first_reaction_timings_survive_restart_and_transition_retention(evidence):
    root, journal, store = evidence
    seed(journal)
    steps = [
        (
            100,
            {
                "stage": "evaluation",
                "action": "hold",
                "proposed_action": "enter",
                "selected_news_id": STORY["news_id"],
            },
        ),
        (110, {"stage": "review_started"}),
        (410, {"stage": "review_finished", "provider": "synthetic"}),
        (500, {"stage": "submission", "action": "enter", "event_id": "c" * 64}),
        (800, {"stage": "commit", "outcome": "simulated_fill", "event_id": "c" * 64}),
    ]
    for offset, transition in steps:
        journal.record([STORY["news_id"]], "BTC-USD", FINGERPRINT, transition, NOW + offset)
    expected = records(store)[0]["latency_ms"]
    assert expected == {
        "publication_to_intake": 1000,
        "intake_to_first_evaluation": 100,
        "intake_to_first_entry_proposal": 100,
        "review_pending_to_result": 300,
        "intake_to_first_submission_intent": 500,
        "intake_to_first_fill_observed": 800,
    }
    restarted = NewsEvidence(root)
    try:
        for i in range(70):
            restarted.record(
                [STORY["news_id"]],
                "BTC-USD",
                FINGERPRINT,
                {"stage": "evaluation", "action": "hold", "reason": f"fixture_{i}"},
                NOW + 1000 + i,
            )
        record = records(store)[0]
        assert record["omitted_transitions"] > 0
        assert all(step["at_ms"] >= NOW + 1000 for step in record["transitions"])
        assert record["latency_ms"] == expected
    finally:
        restarted.close()


def test_model_and_entry_timing_belong_only_to_the_selected_headline(evidence):
    _, journal, store = evidence
    other = {**STORY, "news_id": "b" * 64}
    seed(journal, [STORY, other])
    journal.record(
        [STORY["news_id"], other["news_id"]],
        "BTC-USD",
        FINGERPRINT,
        {
            "stage": "evaluation",
            "proposed_action": "enter",
            "action": "hold",
            "selected_news_id": STORY["news_id"],
        },
        NOW + 100,
    )
    journal.record(
        [STORY["news_id"]], "BTC-USD", FINGERPRINT, {"stage": "review_started"}, NOW + 110
    )
    journal.record(
        [STORY["news_id"]], "BTC-USD", FINGERPRINT, {"stage": "review_finished"}, NOW + 410
    )
    selected = next(item for item in records(store) if item["news"]["news_id"] == STORY["news_id"])
    corroboration = next(
        item for item in records(store) if item["news"]["news_id"] == other["news_id"]
    )
    assert selected["latency_ms"]["review_pending_to_result"] == 300
    assert corroboration["latency_ms"]["intake_to_first_evaluation"] == 100
    assert corroboration["latency_ms"]["intake_to_first_entry_proposal"] is None
    assert corroboration["latency_ms"]["review_pending_to_result"] is None
    assert selected["latency_ms"]["intake_to_first_fill_observed"] is None


def test_legacy_capture_never_invents_first_reaction_times(evidence):
    _, journal, store = evidence
    seed(journal)
    row = journal.db.execute("SELECT id,data FROM candidates").fetchone()
    data = json.loads(row["data"])
    del data["timing"]
    with journal.db:
        journal.db.execute("UPDATE candidates SET data=? WHERE id=?", (json.dumps(data), row["id"]))
    journal.record(
        [STORY["news_id"]],
        "BTC-USD",
        FINGERPRINT,
        {"stage": "evaluation", "action": "hold"},
        NOW + 100,
    )
    record = records(store)[0]
    assert "timing" not in record
    assert record["latency_ms"]["publication_to_intake"] == 1000
    assert all(
        value is None
        for key, value in record["latency_ms"].items()
        if key != "publication_to_intake"
    )


def test_reaction_time_cannot_precede_capture(evidence):
    _, journal, _ = evidence
    seed(journal, at=NOW + 1000)
    with pytest.raises(ValueError, match="clock regression"):
        journal.record(
            [STORY["news_id"]],
            "BTC-USD",
            FINGERPRINT,
            {"stage": "evaluation", "action": "hold"},
            NOW + 999,
        )


def test_pagination_bounds_and_read_only_inspection(evidence):
    root, journal, store = evidence
    seed(journal, [STORY, {**STORY, "news_id": "b" * 64}])
    first = read_news_evidence(store, "observer", limit=1)
    assert first["execution_authority"] is False
    assert first["semantics"]["not_strategy_pnl"] is True
    second = read_news_evidence(
        store,
        "observer",
        limit=1,
        before_sequence=first["evidence"]["next_before_sequence"],
    )
    assert (
        second["evidence"]["candidates"][0]["news"]["news_id"]
        != (first["evidence"]["candidates"][0]["news"]["news_id"])
    )
    assert second["evidence"]["next_before_sequence"] is None
    assert (root / "news-evidence.sqlite3").stat().st_mode & 0o777 == 0o600
    for limit in (0, 51, True):
        with pytest.raises(ControlError, match="bounds"):
            read_news_evidence(store, "observer", limit=limit)
    store.status.side_effect = ControlError("capability revoked")
    with pytest.raises(ControlError, match="revoked"):
        read_news_evidence(store, "observer")


def test_inspector_rejects_symlinks_and_does_not_create_a_database(tmp_path):
    root = tmp_path / "private"
    root.mkdir(mode=0o700)
    store = SimpleNamespace(root=root, status=lambda _: {})
    assert read_news_evidence(store, "observer")["evidence"]["state"] == "not_recording"
    assert not list(root.iterdir())
    unrelated = tmp_path / "unrelated.json"
    unrelated.write_text(json.dumps({"untouched": True}))
    (root / "news-evidence.sqlite3").symlink_to(unrelated)
    with pytest.raises(ControlError, match="regular file"):
        read_news_evidence(store, "observer")
    with pytest.raises(ControlError, match="regular file"):
        NewsEvidence(root)
    assert json.loads(unrelated.read_text()) == {"untouched": True}


def test_source_latency_excludes_baseline_synthetic_and_bad_clocks_and_separates_legacy(
    evidence, monkeypatch
):
    from helios_alpha.crypto import evidence as module
    from helios_alpha.crypto.news import Feed, NewsJournal

    root, _, store = evidence
    monkeypatch.setattr(module.time, "time_ns", lambda: (NOW + 1000) * 1_000_000)
    feed = Feed("synthetic", "https://example.com/rss", "example.com", "test")
    journal = NewsJournal(root)
    variants = [
        {"published_ms": NOW - 86_400_000, "synthetic": False},
        {"published_ms": NOW - 10_000, "synthetic": False},
        {"published_ms": NOW - 20_000, "synthetic": False},
        {"published_ms": NOW - 40_000, "synthetic": False},
        {"published_ms": NOW - 60_000},
        {"published_ms": NOW - 1, "synthetic": True},
        {"published_ms": NOW + 1, "synthetic": False},
        {"published_ms": None, "synthetic": False},
        {"published_ms": NOW, "synthetic": 1},
    ]
    for i, changes in enumerate(variants):
        item = {**STORY, **changes, "news_id": f"{i:064x}"}
        if "synthetic" not in changes:
            del item["synthetic"]
        journal.ingest(feed, [item], NOW, {})
    before = journal.db.execute("SELECT data FROM news ORDER BY id").fetchall()
    store.status.return_value = {
        "runtime": {
            "crypto-shadow": {
                "updated_ns": NOW * 1_000_000,
                "status": {"feeds": {"synthetic": {"response_cache_age_seconds": 500}}},
            }
        }
    }
    result = read_news_evidence(store, "observer", limit=1)
    source = result["intake"]["sources"]["synthetic"]
    assert source["stories"] == 9
    assert source["delivery_excluded"] == {"baseline": 1, "synthetic": 1, "invalid": 3}
    assert source["publication_to_intake_ms"]["explicit_non_synthetic"] == {
        "sample_count": 3,
        "mean": 23_333,
        "p50": 20_000,
        "p95": 40_000,
        "maximum": 40_000,
    }
    assert source["publication_to_intake_ms"]["legacy_unlabelled"] == {
        "sample_count": 1,
        "mean": 60_000,
        "p50": 60_000,
        "p95": 60_000,
        "maximum": 60_000,
    }
    assert result["intake"]["writer_health"]["reported_at_ns"] == NOW * 1_000_000
    assert (
        result["intake"]["writer_health"]["feeds"]["synthetic"]["response_cache_age_seconds"] == 500
    )
    assert len(result["intake"]["recent"]) == 1
    assert journal.db.execute("SELECT data FROM news ORDER BY id").fetchall() == before
    journal.close()


def test_unknown_source_latency_is_not_reported_as_zero_delay(evidence, monkeypatch):
    from helios_alpha.crypto import evidence as module
    from helios_alpha.crypto.news import Feed, NewsJournal

    root, _, store = evidence
    monkeypatch.setattr(module.time, "time_ns", lambda: NOW * 1_000_000)
    journal = NewsJournal(root)
    journal.ingest(
        Feed("synthetic", "https://example.com/rss", "example.com", "test"),
        [{**STORY, "synthetic": False}],
        NOW,
        {},
    )
    source = read_news_evidence(store, "observer")["intake"]["sources"]["synthetic"]
    assert source["publication_to_intake_ms"]["explicit_non_synthetic"] == {
        "sample_count": 0,
        "mean": None,
        "p50": None,
        "p95": None,
        "maximum": None,
    }
    journal.close()


def test_capacity_still_allows_existing_candidates_to_finish(evidence, monkeypatch):
    from helios_alpha.crypto import evidence as module

    _, journal, store = evidence
    monkeypatch.setattr(module, "MAX_CANDIDATES", 1)
    seed(journal)
    with pytest.raises(ValueError, match="capacity"):
        seed(journal, [{**STORY, "news_id": "b" * 64}])
    journal.observe(
        [STORY],
        {"BTC-USD": quote(NOW + 30_000)},
        ["BTC-USD"],
        FINGERPRINT,
        COSTS,
        1,
        NOW + 30_000,
    )
    assert records(store)[0]["markouts"][0]["status"] == "observed"
