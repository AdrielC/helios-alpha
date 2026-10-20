import asyncio
import json
from datetime import UTC, datetime
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import Mock

import pytest

from helios_alpha.control.store import ControlError, canonical
from helios_alpha.crypto.news import FEEDS, MAX_ACTIVE_NEWS, MAX_FEED_BYTES, parse_feed
from helios_alpha.crypto.snapshots import (
    DecisionSnapshots,
    read_decision_input,
    read_decision_snapshots,
    replay_local_proposal,
)
from helios_alpha.crypto.strategy import PrivateStrategy

NOW = 1_800_000_000_000
SOURCE = b"""def decide(context, parameters):
    enter = context["costs"]["fee_bps"] <= parameters["maximum_fee"]
    result = {
        "action": "enter" if enter else "hold",
        "allocation_bps": 1000 if enter else 0,
        "reason": "synthetic_entry" if enter else "fee_hurdle",
        "unusual_event": enter,
    }
    if enter:
        result["news_id"] = context["news"][0]["news_id"]
    return result
"""


def context():
    return {
        "schema_version": 1,
        "symbol": "BTC-USD",
        "now_ms": NOW,
        "quote": {
            "symbol": "BTC-USD",
            "bid_micros": 100_000_000,
            "ask_micros": 100_010_000,
            "observed_ms": NOW,
        },
        "market": {"connected": True, "samples": [[NOW - 1000, 99_000_000], [NOW, 100_000_000]]},
        "news": [
            {
                "news_id": "a" * 64,
                "synthetic": True,
                "title": "Synthetic fixture",
                "observed_ms": NOW - 1000,
            }
        ],
        "paper": {"positions": {}, "recent_fills": []},
        "limits": {"capital_micros": 600_000_000},
        "costs": {"fee_bps": 95, "slippage_bps": 5},
    }


@pytest.fixture
def archive(tmp_path):
    root = tmp_path / "private"
    root.mkdir(mode=0o700)
    source, parameters = root / "strategy.py", root / "parameters.json"
    source.write_bytes(SOURCE)
    parameters.write_text('{"maximum_fee":100}')
    source.chmod(0o600)
    parameters.chmod(0o600)
    strategy = PrivateStrategy(source, parameters)
    recorder = DecisionSnapshots(root, strategy)
    store = SimpleNamespace(root=root, status=Mock(return_value={}), policy=Mock(return_value={}))
    yield root, strategy, recorder, store
    recorder.close()


def capture(strategy, recorder, value=None, phase="entry_proposal"):
    value = context() if value is None else value
    return recorder.capture(canonical(value), strategy.propose(value), phase, "synthetic")


def test_declared_corroboration_survives_snapshot_replay(archive):
    root, _, _, store = archive
    source = root / "with-evidence.py"
    source.write_bytes(SOURCE.replace(
        b'        result["news_id"] = context["news"][0]["news_id"]',
        b'        result["news_id"] = context["news"][0]["news_id"]\n'
        b'        result["evidence_news_ids"] = sorted(x["news_id"] for x in context["news"])',
    ))
    source.chmod(0o600)
    strategy = PrivateStrategy(source, root / "parameters.json")
    value = context()
    value["news"].append({**value["news"][0], "news_id": "b" * 64})
    recorder = DecisionSnapshots(root, strategy)
    try:
        identity = capture(strategy, recorder, value)
        snapshot = read_decision_input(store, "observer", identity)
        assert snapshot["proposal"]["evidence_news_ids"] == ["a" * 64, "b" * 64]
        replay = replay_local_proposal(root, snapshot)
        assert replay["matches_original"]
        assert replay["replayed_proposal"]["evidence_news_ids"] == ["a" * 64, "b" * 64]
    finally:
        recorder.close()


def test_original_inputs_and_strategy_version_survive_restart_and_fee_scenario(archive):
    root, strategy, recorder, store = archive
    original = context()
    identity = capture(strategy, recorder, original)
    original["quote"]["bid_micros"] = 500_000_000
    snapshot = read_decision_input(store, "observer", identity)
    assert snapshot["context"]["quote"]["bid_micros"] == 100_000_000
    # A change to the currently selected source must not rewrite the archived version.
    (root / "strategy.py").write_text("raise RuntimeError('current version must not run')")
    restarted = DecisionSnapshots(root, strategy)
    try:
        assert capture(strategy, restarted) == identity
        result = replay_local_proposal(root, snapshot, fee_bps=200)
        assert result["matches_original"] is True
        assert result["replayed_proposal"]["action"] == "enter"
        assert result["fee_scenario"]["proposal"]["reason"] == "fee_hurdle"
        assert result["fee_scenario"]["changes_settings"] is False
        assert result["not_strategy_pnl"] is True
        assert read_decision_input(store, "observer", identity) == snapshot
        assert snapshot["execution_authority"] is False
        assert snapshot["synthetic_inputs"] is True
        for path in (
            root / "decision-inputs.sqlite3",
            root / "strategy-versions" / strategy.fingerprint / "strategy.py",
        ):
            assert path.stat().st_mode & 0o777 == 0o600
    finally:
        restarted.close()


def test_archive_deduplicates_first_matching_proposal_and_captures_a_changed_reason(archive):
    _, strategy, recorder, store = archive
    first = context()
    identity = capture(strategy, recorder, first, phase="evaluation")
    changed = {**context(), "now_ms": NOW + 1000}
    assert capture(strategy, recorder, changed, phase="evaluation") == identity
    changed["costs"] = {"fee_bps": 200, "slippage_bps": 5}
    assert capture(strategy, recorder, changed, phase="evaluation") != identity
    report = read_decision_snapshots(store, "observer")
    assert report["snapshot_count"] == 2
    assert {item["proposal"]["action"] for item in report["snapshots"]} == {"enter", "hold"}
    assert all("context" not in item for item in report["snapshots"])


def test_inputs_recheck_both_permissions_and_list_rechecks_status(archive):
    _, strategy, recorder, store = archive
    identity = capture(strategy, recorder)
    store.policy.side_effect = ControlError("risk access revoked")
    assert read_decision_snapshots(store, "observer")["snapshot_count"] == 1
    with pytest.raises(ControlError, match="revoked"):
        read_decision_input(store, "observer", identity)
    store.status.side_effect = ControlError("status access revoked")
    with pytest.raises(ControlError, match="revoked"):
        read_decision_snapshots(store, "observer")


def test_archived_code_change_is_rejected_before_execution(archive):
    root, strategy, recorder, store = archive
    identity = capture(strategy, recorder)
    snapshot = read_decision_input(store, "observer", identity)
    marker = root / "should-not-exist"
    version = root / "strategy-versions" / strategy.fingerprint / "strategy.py"
    version.write_text(f"open({str(marker)!r}, 'w').write('unexpected')\n" + SOURCE.decode())
    with pytest.raises(ControlError, match="fingerprint"):
        replay_local_proposal(root, snapshot)
    assert not marker.exists()


def test_corrupt_input_fails_digest_validation(archive):
    _, strategy, recorder, store = archive
    identity = capture(strategy, recorder)
    row = recorder.db.execute("SELECT data FROM snapshots WHERE id=?", (identity,)).fetchone()
    data = json.loads(row["data"])
    data["context"]["costs"]["fee_bps"] = 0
    with recorder.db:
        recorder.db.execute("UPDATE snapshots SET data=? WHERE id=?", (canonical(data), identity))
    with pytest.raises(ControlError, match="digest"):
        read_decision_input(store, "observer", identity)
    with pytest.raises(ControlError, match="digest"):
        read_decision_snapshots(store, "observer")


def test_capacity_rotates_oldest_inputs_and_reports_eviction(archive, monkeypatch):
    from helios_alpha.crypto import snapshots

    _, strategy, recorder, store = archive
    monkeypatch.setattr(snapshots, "MAX_RECORDS", 1)
    identity = capture(strategy, recorder)
    assert capture(strategy, recorder) == identity
    another = context()
    another["news"][0]["news_id"] = "b" * 64
    newest = capture(strategy, recorder, another)
    assert read_decision_input(store, "observer", newest)
    with pytest.raises(ControlError, match="not found"):
        read_decision_input(store, "observer", identity)
    listing = read_decision_snapshots(store, "observer")
    assert listing["snapshot_count"] == 1 and listing["evicted_snapshots"] == 1
    oversized = context()
    oversized["news"][0]["title"] = "x" * snapshots.MAX_BYTES
    with pytest.raises(ValueError, match="bound"):
        capture(strategy, recorder, oversized)
    assert read_decision_input(store, "observer", newest)


def test_absent_archive_reads_create_nothing_and_symlinks_are_rejected(tmp_path):
    root = tmp_path / "private"
    root.mkdir(mode=0o700)
    store = SimpleNamespace(root=root, status=lambda _: {}, policy=lambda _: {})
    assert read_decision_snapshots(store, "observer")["state"] == "not_recording"
    assert not list(root.iterdir())
    unrelated = tmp_path / "unrelated"
    unrelated.write_text("untouched")
    (root / "decision-inputs.sqlite3").symlink_to(unrelated)
    with pytest.raises(ControlError, match="regular file"):
        read_decision_snapshots(store, "observer")
    assert unrelated.read_text() == "untouched"


def test_byte_budget_rotates_before_database_capacity_is_exhausted(archive, monkeypatch):
    from helios_alpha.crypto import snapshots

    _, strategy, recorder, store = archive
    original = capture(strategy, recorder)
    size = recorder.db.execute("SELECT LENGTH(CAST(data AS BLOB)) FROM snapshots").fetchone()[0]
    monkeypatch.setattr(snapshots, "MAX_ARCHIVE_BYTES", size + 10)
    another = context()
    another["news"][0]["news_id"] = "b" * 64
    newest = capture(strategy, recorder, another)
    assert read_decision_input(store, "observer", newest)
    assert read_decision_snapshots(store, "observer")["evicted_snapshots"] == 1
    with pytest.raises(ControlError, match="not found"):
        read_decision_input(store, "observer", original)


def test_bar_research_cannot_claim_to_backtest_a_news_strategy(tmp_path, monkeypatch):
    from helios_alpha.crypto import research

    root = tmp_path / "private"
    root.mkdir(mode=0o700)
    monkeypatch.setattr(research, "read_private_json", lambda _: {"engine": "news_shock"})
    network = Mock(side_effect=AssertionError("news preflight must not download candles"))
    monkeypatch.setattr(research.httpx, "AsyncClient", network)
    with pytest.raises(ValueError, match="decision-snapshots"):
        asyncio.run(research.research(root, Path("/unused"), 90))
    network.assert_not_called()
    assert not list(root.iterdir())


def test_pagination_and_arguments_are_bounded(archive):
    _, strategy, recorder, store = archive
    capture(strategy, recorder)
    another = context()
    another["news"][0]["news_id"] = "b" * 64
    capture(strategy, recorder, another)
    page = read_decision_snapshots(store, "observer", limit=1)
    following = read_decision_snapshots(
        store, "observer", limit=1, before_sequence=page["next_before_sequence"]
    )
    assert following["snapshots"][0]["snapshot_id"] != page["snapshots"][0]["snapshot_id"]
    for limit in (0, 51, True):
        with pytest.raises(ControlError, match="bounds"):
            read_decision_snapshots(store, "observer", limit=limit)
    for identity in ("../other", "a" * 63, None):
        with pytest.raises(ControlError, match="identity"):
            read_decision_input(store, "observer", identity)


@pytest.mark.parametrize("character", ["x", "\U0001f680"])
def test_full_accepted_news_window_survives_capture_and_replay(archive, character):
    root, strategy, recorder, store = archive
    value = context()
    value["news"] = []
    stamp = datetime.fromtimestamp(NOW / 1000, UTC).isoformat()
    per_feed, extra = divmod(MAX_ACTIVE_NEWS, len(FEEDS))
    for feed_index, feed in enumerate(FEEDS):
        entries = []
        for i in range(per_feed + int(feed_index < extra)):
            prefix = f"https://{feed.host}/synthetic-{i}/"
            url = prefix + character * (2048 - len(prefix))
            entries.append(
                f"<item><title>{character * 500}</title><link>{url}</link>"
                f"<pubDate>{stamp}</pubDate></item>"
            )
        raw = ("<rss><channel>" + "".join(entries) + "</channel></rss>").encode()
        assert len(raw) <= MAX_FEED_BYTES
        items = parse_feed(feed, raw)
        assert len(items) == len(entries)
        value["news"].extend(
            {**item, "eligible": True, "observed_ms": NOW, "synthetic": True} for item in items
        )
    assert len(value["news"]) == MAX_ACTIVE_NEWS
    value["market"]["samples"] = [[NOW - i * 1000, 100_000_000] for i in reversed(range(360))]
    value["paper"]["recent_fills"] = [
        {
            "simulation": True,
            "event_id": f"{i:064x}",
            "symbol": "BTC-USD",
            "at_ms": NOW - 10_000,
            "side": "sell",
            "quantity_micros": 1000,
            "price_micros": 100_000_000,
            "fee_micros": 100,
        }
        for i in range(200)
    ]
    assert len(canonical(value).encode()) > 64 * 1024
    identity = capture(strategy, recorder, value)
    snapshot = read_decision_input(store, "observer", identity)
    assert snapshot["context"] == value
    assert read_decision_snapshots(store, "observer")["snapshots"][0]["snapshot_id"] == identity
    assert replay_local_proposal(root, snapshot)["matches_original"]
