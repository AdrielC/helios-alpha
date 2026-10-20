import copy

import pytest

pytest.importorskip("defusedxml")

from helios_alpha.crypto.news_review import prepare_news_review
from helios_alpha.crypto.strategy import PrivateStrategy

NOW = 1_800_000_000_000
ANCHOR, SUPPORT, REPLACEMENT = "a" * 64, "b" * 64, "c" * 64
FINGERPRINT = "d" * 64


def story(identity, *, observed=NOW - 1000, published=NOW - 2000):
    return {
        "news_id": identity,
        "source": identity,
        "source_category": "reporting",
        "title": "Synthetic evidence " + identity,
        "observed_ms": observed,
        "published_ms": published,
        "eligible": True,
    }


def context():
    return {
        "symbol": "BTC-USD",
        "news": [story(ANCHOR), story(SUPPORT)],
        "quote": {"symbol": "BTC-USD", "bid_micros": 100, "ask_micros": 101},
        "market": {"samples": [[NOW - 1000, 100], [NOW, 101]]},
    }


def proposal(**changes):
    return {
        "action": "enter", "allocation_bps": 1000, "reason": "synthetic",
        "unusual_event": True, "news_id": ANCHOR, "evidence_news_ids": [ANCHOR, SUPPORT],
        **changes,
    }


def test_selected_reports_survive_a_full_window_of_unrelated_news():
    value = context()
    value["news"] += [story(f"{i:064x}") for i in range(198)]
    frozen = copy.deepcopy(value)
    prepared = prepare_news_review(proposal(), value, FINGERPRINT, NOW)
    assert prepared["event"]["evidence_news_ids"] == [ANCHOR, SUPPORT]
    assert [item["news_id"] for item in prepared["event"]["other_reports"]] == [SUPPORT]
    assert value == frozen
    value["news"].reverse()
    assert prepare_news_review(proposal(), value, FINGERPRINT, NOW) == prepared


def test_oldest_support_controls_expiry_not_only_the_anchor():
    value = context()
    value["news"][1]["published_ms"] = NOW - 119_999
    prepared = prepare_news_review(proposal(), value, FINGERPRINT, NOW)
    assert prepared["expires_ms"] == NOW + 1
    with pytest.raises(ValueError, match="expired"):
        prepare_news_review(proposal(), value, FINGERPRINT, NOW + 1)


@pytest.mark.parametrize(
    "changes",
    [
        {"eligible": False},
        {"published_ms": NOW - 120_001},
        {"observed_ms": NOW - 120_001},
        {"published_ms": NOW + 1},
        {"observed_ms": NOW + 1},
    ],
)
def test_unavailable_support_cannot_use_a_fresh_anchor(changes):
    value = context()
    value["news"][1].update(changes)
    with pytest.raises(ValueError, match="unavailable or stale"):
        prepare_news_review(proposal(), value, FINGERPRINT, NOW)


def test_missing_or_conflicting_support_is_rejected():
    value = context()
    value["news"].pop()
    with pytest.raises(ValueError, match="unavailable"):
        prepare_news_review(proposal(), value, FINGERPRINT, NOW)
    value = context()
    value["news"].append({**value["news"][1], "title": "Conflicting update"})
    with pytest.raises(ValueError, match="conflicting"):
        prepare_news_review(proposal(), value, FINGERPRINT, NOW)


def test_identity_tracks_evidence_but_not_changing_prices_or_unrelated_news():
    value = context()
    prepared = prepare_news_review(proposal(), value, FINGERPRINT, NOW)
    value["quote"]["bid_micros"] = 99
    value["news"].append(story(REPLACEMENT))
    assert prepare_news_review(proposal(), value, FINGERPRINT, NOW)["identity"] == prepared[
        "identity"
    ]
    assert prepare_news_review(
        proposal(evidence_news_ids=[ANCHOR, REPLACEMENT]), value, FINGERPRINT, NOW
    )["identity"] != prepared["identity"]
    value["news"][1]["title"] = "Changed supporting facts"
    assert prepare_news_review(proposal(), value, FINGERPRINT, NOW)["identity"] != prepared[
        "identity"
    ]
    assert prepare_news_review(proposal(), value, "e" * 64, NOW)["identity"] != prepared["identity"]


def test_async_review_input_is_frozen_with_nested_source_facts():
    value = context()
    value["news"][0]["source_facts"] = {"action": "lower"}
    prepared = prepare_news_review(proposal(), value, FINGERPRINT, NOW)
    value["news"][0]["source_facts"]["action"] = "raise"
    value["news"][1]["title"] = "Changed"
    value["quote"]["bid_micros"] = 0
    value["market"]["samples"][0][1] = 0
    assert prepared["event"]["source_facts"]["action"] == "lower"
    assert prepared["event"]["other_reports"][0]["title"] == "Synthetic evidence " + SUPPORT
    assert prepared["event"]["quote"]["bid_micros"] == 100
    assert prepared["event"]["reference_samples"][0][1] == 100


def test_legacy_proposal_uses_only_its_anchor_without_inventing_corroboration():
    value = proposal()
    del value["evidence_news_ids"]
    prepared = prepare_news_review(value, context(), FINGERPRINT, NOW)
    assert prepared["event"]["evidence_news_ids"] == [ANCHOR]
    assert prepared["event"]["other_reports"] == []


@pytest.mark.parametrize(
    "identities",
    [
        None, [], [ANCHOR, ANCHOR], [SUPPORT], ["invalid"], [True],
        [ANCHOR, REPLACEMENT], [ANCHOR] + [f"{i:064x}" for i in range(8)],
    ],
)
def test_strategy_contract_rejects_invalid_evidence(identities):
    strategy = object.__new__(PrivateStrategy)
    strategy.parameters = {}
    strategy.function = lambda *_: proposal(evidence_news_ids=identities)
    with pytest.raises(ValueError, match="evidence identities"):
        strategy.propose(context())


def test_strategy_contract_keeps_valid_evidence():
    strategy = object.__new__(PrivateStrategy)
    strategy.parameters = {}
    strategy.function = lambda *_: proposal()
    assert strategy.propose(context())["evidence_news_ids"] == [ANCHOR, SUPPORT]
