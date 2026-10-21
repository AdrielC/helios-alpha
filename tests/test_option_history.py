"""Synthetic protocol fixtures. No provider data or performance claims in Git."""

import csv
import io
import json
from urllib.parse import parse_qs

import httpx
import pytest

from helios_alpha.natgas.cli import run_replay
from helios_alpha.natgas.databento import DatabentoHistory, HistoryError, load_key
from helios_alpha.natgas.history import (
    ContractManifest,
    HistoryQuery,
    archive_blob,
    expiry_close_ms,
    import_quotes,
    normalize_quotes,
    timestamp_ns,
)
from helios_alpha.natgas.history_cli import discover_contracts, export_capture
from helios_alpha.natgas.lab import Journal, settings
from helios_alpha.natgas.replay import Contract, Plan, Quote, ReplayDataset

SYMBOL = "UNG   261120C00010000"
START = "2026-10-20T14:00:00Z"
BASE = timestamp_ns(START)
NOW = timestamp_ns("2026-10-21T14:00:00Z")


@pytest.fixture
def root(tmp_path):
    path = tmp_path / "private"
    path.mkdir(mode=0o700)
    return path


def query(**changes):
    return HistoryQuery(
        **{
            "symbols": [SYMBOL],
            "start": START,
            "end": "2026-10-20T15:00:00Z",
            **changes,
        }
    )


def metadata(**changes):
    contract = Contract(
        **{
            "id": SYMBOL,
            "symbol": "UNG",
            "kind": "call",
            "strike_cents": 1000,
            "multiplier": 100,
            "expiry": "2026-11-20",
            "close_by_ms": expiry_close_ms("2026-11-20"),
            "available_ms": BASE // 1000000 - 86400000,
            **changes,
        }
    )
    return ContractManifest(
        provenance="synthetic verified standard-contract fixture", contracts=[contract]
    )


def row(offset=1_000_000_000, bid=39, ask=40, **changes):
    return {
        "ts_recv": str(BASE + offset + 100),
        "ts_event": str(BASE + offset),
        "rtype": "177",
        "publisher_id": "30",
        "instrument_id": "123",
        "action": "M",
        "flags": "192",
        "bid_px_00": str(bid * 10000000),
        "ask_px_00": str(ask * 10000000),
        "bid_sz_00": "2",
        "ask_sz_00": "3",
        "symbol": SYMBOL,
        **changes,
    }


def csv_bytes(rows):
    stream = io.StringIO()
    writer = csv.DictWriter(stream, fieldnames=list(rows[0]))
    writer.writeheader()
    writer.writerows(rows)
    return stream.getvalue().encode()


def plan():
    return Plan(
        id="fixture-plan",
        event_id="fixture-event",
        evidence_ids=["frozen-fixture-evidence"],
        contract_id=SYMBOL,
        event_available_ms=BASE // 1000000,
        available_ms=BASE // 1000000,
    )


def test_native_submillisecond_quotes_survive_and_feed_actual_replay(root):
    raw = csv_bytes(
        [
            row(1_000_000_000),
            row(1_000_000_500, bid=38),
            row(2_000_000_000, bid=60, ask=61),
            row(3_002_000_000, bid=50, ask=51),
        ]
    )
    path, receipt_path, receipt = import_quotes(root, raw, query(), metadata(), [plan()])
    dataset = json.loads(path.read_text())
    parsed = ReplayDataset(**dataset)
    assert len(parsed.quotes) == 4
    assert parsed.quotes[0].observed_ms == parsed.quotes[1].observed_ms
    assert parsed.quotes[0].observed_ns != parsed.quotes[1].observed_ns
    assert parsed.quotes[0].available_ms == BASE // 1000000 + 1001
    result = run_replay(dataset, settings(root))
    assert result["trades"][0]["pnl_cents"] == 790
    assert result["closed_trades"] == 1
    assert result["profitability_established"] is False
    assert receipt["quality"]["source_records"] == 4
    assert json.loads(receipt_path.read_text())["dataset_sha256"] == path.stem
    assert path.stat().st_mode & 0o777 == 0o600
    assert path.parent.stat().st_mode & 0o777 == 0o700
    assert import_quotes(root, raw, query(), metadata(), [plan()])[0] == path
    assert not (root / "natgas.sqlite3").exists()


def test_same_clock_updates_keep_file_order_without_fabricated_time(root):
    raw = csv_bytes([row(bid=38), row(bid=39)])
    quotes, report = normalize_quotes(raw, query(), metadata())
    assert quotes[0].observed_ns == quotes[1].observed_ns
    assert quotes[0].available_ns == quotes[1].available_ns
    assert [q.source_sequence for q in quotes] == [0, 1]
    assert report["accepted_quotes"] == 2
    path, _, _ = import_quotes(root, raw, query(), metadata(), [plan()])
    assert run_replay(json.loads(path.read_text()), settings(root))["unresolved_trades"] == 1


def test_same_receive_clock_preserves_provider_order_when_event_clocks_differ(root):
    received = str(BASE + 1_000_000_500)
    raw = csv_bytes(
        [
            row(1_000_000_300, ts_recv=received),
            row(1_000_000_100, ts_recv=received, bid=38),
        ]
    )
    path, _, _ = import_quotes(root, raw, query(), metadata(), [plan()])
    report = run_replay(json.loads(path.read_text()), settings(root))
    assert report["trades"][0]["entry_quote"]["source_sequence"] == 0


@pytest.mark.parametrize(
    "changes",
    [
        {"flags": "200"},
        {"flags": "196"},
        {"flags": "224"},
        {"bid_sz_00": "0"},
        {"ask_px_00": str(2**63 - 1)},
        {"bid_px_00": "990000000"},
        {"publisher_id": "1"},
        {"action": "T"},
        {"ts_event": str(BASE + 99_000_000_000)},
    ],
)
def test_bad_clocks_books_snapshots_and_trade_records_never_fill(changes):
    raw = csv_bytes([row(**changes), row(2_000_000_000)])
    quotes, report = normalize_quotes(raw, query(), metadata())
    assert len(quotes) == 1 and sum(report["rejected"].values()) == 1


def test_dedup_and_latency_are_explicit():
    assert timestamp_ns("2026-10-20T14:00:00.000000123Z") == BASE + 123
    raw = csv_bytes([row(), row()])
    quotes, report = normalize_quotes(raw, query(), metadata(), feed_latency_ms=250)
    assert len(quotes) == 1 and report["rejected"]["duplicate_record"] == 1
    assert quotes[0].available_ms - quotes[0].observed_ms == 251
    assert report["feed_latency_ms"] == 250


@pytest.mark.parametrize(
    "changes",
    [
        {"bid_px_00": "0.39"},
        {"bid_px_00": "391000000"},
        {"rtype": "1"},
        {"symbol": "UNG1  261120C00010000"},
        {"ts_recv": str(BASE - 1)},
        {"flags": "999"},
    ],
)
def test_schema_units_symbols_and_bounds_fail_closed(changes):
    with pytest.raises(ValueError):
        normalize_quotes(csv_bytes([row(**changes)]), query(), metadata())


def test_record_cap_and_receive_disorder_fail_closed():
    with pytest.raises(ValueError, match="cap"):
        normalize_quotes(csv_bytes([row()]), query(record_limit=1), metadata())
    with pytest.raises(ValueError, match="order"):
        normalize_quotes(csv_bytes([row(2_000_000_000), row()]), query(), metadata())


@pytest.mark.parametrize(
    "changes",
    [
        {"symbols": ["ALL_SYMBOLS"]},
        {"symbols": []},
        {"start": "2026-10-20"},
        {"end": "2026-10-24T14:00:00Z"},
        {"start": "2022-10-20T14:00:00Z"},
        {"stype_in": "parent", "symbols": ["UNG.OPT"]},
    ],
)
def test_query_cannot_expand_into_unbounded_market_or_invented_clock(changes):
    with pytest.raises(ValueError):
        query(**changes)


def test_metadata_is_point_in_time_and_adjusted_contracts_require_separate_support(root):
    with pytest.raises(ValueError, match="deliverables"):
        metadata(multiplier=10)
    with pytest.raises(ValueError, match="disagrees"):
        metadata(strike_cents=1100)
    with pytest.raises(ValueError, match="not yet"):
        import_quotes(
            root, csv_bytes([row()]), query(), metadata(available_ms=BASE // 1000000 + 1), [plan()]
        )
    assert expiry_close_ms("2026-11-27") == timestamp_ns("2026-11-27T18:00:00Z") // 1000000
    assert expiry_close_ms("2026-11-28") == expiry_close_ms("2026-11-27")


def provider(raw, *, count=1, cost="0.01", fail_stream=False):
    calls = []

    def handler(request):
        calls.append(request)
        assert request.url.host == "hist.databento.com"
        assert request.url.path.startswith("/v0/") and "/v0/v0/" not in request.url.path
        assert request.method == "POST"
        assert request.headers["Authorization"].startswith("Basic ")
        body = parse_qs(request.content.decode())
        assert body["symbols"] == [SYMBOL]
        assert body["schema"] == ["cmbp-1"]
        if request.url.path.endswith("get_record_count"):
            return httpx.Response(200, json=count)
        if request.url.path.endswith("get_cost"):
            return httpx.Response(200, json=float(cost))
        assert request.url.path == "/v0/timeseries.get_range"
        assert body["encoding"] == ["csv"] and body["map_symbols"] == ["true"]
        assert body["pretty_px"] == body["pretty_ts"] == ["false"]
        return httpx.Response(503 if fail_stream else 200, content=raw)

    return DatabentoHistory("synthetic-test-value", transport=httpx.MockTransport(handler)), calls


def test_estimate_is_read_only_and_download_requires_fresh_approved_matching_amount(root):
    client, calls = provider(csv_bytes([row()]))
    estimate = client.estimate(query(), now_ns=NOW)
    assert len(calls) == 2 and all("metadata" in r.url.path for r in calls)
    with pytest.raises(HistoryError, match="approved"):
        client.fetch(root, estimate, "0.001", now_ns=NOW)
    with pytest.raises(HistoryError, match="expired"):
        client.fetch(root, estimate, "0.02", now_ns=NOW + 3600 * 1000000000)
    tampered = {**estimate, "records": 1000}
    with pytest.raises(HistoryError, match="integrity"):
        client.fetch(root, tampered, "0.02", now_ns=NOW)
    assert len(calls) == 2
    original, receipt = client.fetch(root, estimate, "0.01", now_ns=NOW)
    assert receipt["status"] == "downloaded" and original.read_bytes() == csv_bytes([row()])
    with pytest.raises(HistoryError, match="already used"):
        client.fetch(root, estimate, "0.01", now_ns=NOW)
    assert sum("timeseries" in r.url.path for r in calls) == 1
    client.close()


def test_changed_price_blocks_billing_and_failed_stream_is_never_retried(root):
    client, _ = provider(csv_bytes([row()]))
    estimate = client.estimate(query(), now_ns=NOW)
    client.close()
    changed, calls = provider(csv_bytes([row()]), cost="0.02")
    with pytest.raises(HistoryError, match="changed"):
        changed.fetch(root, estimate, "0.03", now_ns=NOW)
    assert not any("timeseries" in r.url.path for r in calls)
    changed.close()
    failed, calls = provider(b"", fail_stream=True)
    with pytest.raises(HistoryError, match="failed"):
        failed.fetch(root, estimate, "0.01", now_ns=NOW)
    with pytest.raises(HistoryError, match="already used"):
        failed.fetch(root, estimate, "0.01", now_ns=NOW)
    assert sum("timeseries" in r.url.path for r in calls) == 1
    receipt = next((root / "options-history").glob("download-*.json"))
    assert json.loads(receipt.read_text())["status"] == "failed"
    failed.close()


def test_incomplete_stream_and_bad_counts_never_report_download_success(root):
    client, _ = provider(csv_bytes([row()]), count=2)
    estimate = client.estimate(query(), now_ns=NOW)
    with pytest.raises(HistoryError):
        client.fetch(root, estimate, "0.01", now_ns=NOW)
    assert next((root / "options-history").glob("*.csv")).read_bytes() == csv_bytes([row()])
    client.close()
    empty, _ = provider(b"", count=0)
    with pytest.raises(HistoryError):
        empty.estimate(query(), now_ns=NOW)
    empty.close()


def test_catalog_preserves_first_availability_without_inventing_multiplier():
    definition = {
        "raw_symbol": SYMBOL,
        "ts_event": str(BASE),
        "ts_recv": str(BASE + 100),
        "rtype": "19",
        "contract_multiplier": str(2**31 - 1),
    }
    result = discover_contracts(csv_bytes([definition]), query(record_schema="definition"))
    assert result["candidates"][0]["available_ms"] == BASE // 1000000 + 1
    assert result["candidates"][0]["provider_multiplier"] is None


def test_optional_clocks_do_not_duplicate_existing_live_quote_identity(root):
    journal = Journal(root)
    legacy = {
        "contract_id": SYMBOL,
        "observed_ms": BASE // 1000000,
        "available_ms": BASE // 1000000,
        "bid_cents": 39,
        "ask_cents": 40,
        "bid_size": 2,
        "ask_size": 3,
        "regular_session": True,
    }
    journal.record_options([metadata().contracts[0].model_dump()], [legacy], BASE // 1000000)
    journal.record_options(
        [metadata().contracts[0].model_dump()], [Quote(**legacy).model_dump()], BASE // 1000000
    )
    assert journal.db.execute("SELECT COUNT(*) FROM ng_quotes").fetchone()[0] == 1
    journal.close()
    result = export_capture(root, [])
    assert result["quotes"] == 1 and result["ready_for_replay"] is False
    assert result["profitability_established"] is False


def test_private_archives_reject_symlinks_and_keys_are_not_required_for_offline_import(
    root, monkeypatch
):
    monkeypatch.delenv("DATABENTO_API_KEY", raising=False)
    with pytest.raises(HistoryError):
        load_key(root)
    blob = archive_blob(root, b"test payload", "csv")
    blob.unlink()
    blob.symlink_to(root / "unrelated")
    with pytest.raises(ValueError):
        archive_blob(root, b"test payload", "csv")
    path, _, _ = import_quotes(root, csv_bytes([row()]), query(), metadata(), [])
    assert json.loads(path.read_text())["data_class"] == "recorded"


def test_quote_only_archive_cannot_be_misreported_as_a_zero_profit_backtest(root):
    path, _, receipt = import_quotes(root, csv_bytes([row()]), query(), metadata(), [])
    assert receipt["ready_for_replay"] is False
    with pytest.raises(ValueError, match="frozen plans"):
        run_replay(json.loads(path.read_text()), settings(root))


def test_native_clock_validation_and_replay_order_are_causal(root):
    import random

    raw = csv_bytes(
        [
            row(1_000_000_000),
            row(1_000_000_500, bid=38),
            row(2_000_000_000, bid=60, ask=61),
            row(3_002_000_000, bid=50, ask=51),
        ]
    )
    path, _, _ = import_quotes(root, raw, query(), metadata(), [plan()])
    dataset = json.loads(path.read_text())
    config = settings(root)
    expected = run_replay(dataset, config)["trades"]
    assert expected[0]["entry_quote"]["source_sequence"] == 0
    assert expected[0]["exit_quote"]["source_sequence"] == 3
    generator = random.Random(31337)
    for _ in range(30):
        generator.shuffle(dataset["quotes"])
        assert run_replay(dataset, config)["trades"] == expected
    quote = dataset["quotes"][0]
    with pytest.raises(ValueError, match="native quote clocks"):
        Quote(**{**quote, "available_ms": quote["available_ms"] - 1})
    with pytest.raises(ValueError, match="source sequence"):
        Quote(**{**quote, "source_sequence": None})


def test_stricter_execution_costs_reduce_profit_at_the_same_frozen_exit(root):
    raw = csv_bytes(
        [
            row(1_000_000_000),
            row(2_000_000_000, bid=90, ask=91),
            row(3_002_000_000, bid=80, ask=81),
        ]
    )
    path, _, _ = import_quotes(root, raw, query(), metadata(), [plan()])
    dataset = json.loads(path.read_text())
    config = settings(root)
    prior = None
    for slippage in range(6):
        config["replay"]["slippage_per_share_cents"] = slippage
        result = run_replay(dataset, config)
        assert result["closed_trades"] == 1
        if prior is not None:
            assert prior - result["pnl_cents"] == 200
        prior = result["pnl_cents"]


def test_malformed_cost_never_reaches_a_billed_request(root):
    from helios_alpha.natgas.databento import dollars

    for value in ("garbage", "NaN", "Infinity", "-1", True):
        with pytest.raises(HistoryError):
            dollars(value)
