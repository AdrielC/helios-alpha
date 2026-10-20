#!/usr/bin/env python3
"""Recover acknowledged synthetic news and run the isolated paper entry/exit loop."""

from __future__ import annotations

import argparse
import asyncio
import hashlib
import importlib.util
import json
import os
import subprocess
import time
import uuid
from datetime import UTC, datetime
from pathlib import Path
from zoneinfo import ZoneInfo

from helios_alpha.control.golem_store import GolemControlStore, read_private_json
from helios_alpha.control.store import DEFAULT_ROOT, canonical, private_root
from helios_alpha.crypto import fast_runtime
from helios_alpha.crypto.evidence import read_news_evidence
from helios_alpha.crypto.fomc import TITLE as FOMC_TITLE
from helios_alpha.crypto.fomc import parse_statement
from helios_alpha.crypto.news import FEEDS, MAX_ACTIVE_NEWS, Feed, NewsJournal, parse_feed
from helios_alpha.crypto.paper_transport import PaperTransport
from helios_alpha.crypto.runtime import ACTOR
from helios_alpha.crypto.snapshots import (
    read_decision_input,
    read_decision_snapshots,
    replay_local_proposal,
)
from helios_alpha.crypto.strategy import PrivateStrategy


def now_ms():
    return time.time_ns() // 1_000_000


def rss(path, published_ms, title="SEC approves Bitcoin ETF", *, host="www.sec.gov"):
    stamp = datetime.fromtimestamp(published_ms / 1000, UTC).isoformat()
    return (
        f"<rss><channel><item><title>{title}</title>"
        f"<link>https://{host}/{path}</link><pubDate>{stamp}</pubDate>"
        "</item></channel></rss>"
    ).encode()


async def verify(
    strategy_path=None,
    strategy_config_path=None,
    *,
    burst_news=False,
    fomc_cut=False,
    corroborated_reports=False,
    corroborating_source="cointelegraph",
    delayed_reaction=False,
):
    os.umask(0o077)
    if fomc_cut and corroborated_reports:
        raise ValueError("choose one news fixture")
    if corroborating_source not in {"cointelegraph", "decrypt"}:
        raise ValueError("unsupported corroborating fixture source")
    config = read_private_json(DEFAULT_ROOT / "runtime.json")
    if (strategy_path is None) != (strategy_config_path is None):
        raise ValueError("candidate source and parameters must be supplied together")
    if strategy_path is not None:
        source = Path(strategy_path).expanduser().absolute()
        parameters = Path(strategy_config_path).expanduser().absolute()
        # Validate owner-selected code before creating any proof account or streams.
        PrivateStrategy(source, parameters)
        config["strategy_path"] = str(source)
        config["strategy_config_path"] = str(parameters)
    config["symbols"] = ["BTC-USD"]
    main = GolemControlStore()
    main_settings = main.settings("observer")
    main_paper = main.status("observer")["paper"]
    proof_id = "runtime-proof-" + uuid.uuid4().hex
    root = private_root(DEFAULT_ROOT / "proofs" / proof_id, create=True)
    connection = read_private_json(DEFAULT_ROOT / "golem-connection.json")
    connection["account_id"] = proof_id
    connection["profile"] = "news-proof"
    connection.pop("phantom_id", None)
    for name, value in (
        ("runtime.json", config),
        ("golem-connection.json", connection),
        ("nats-client.json", read_private_json(DEFAULT_ROOT / "nats-client.json")),
    ):
        (root / name).write_text(canonical(value))
    repo = Path(__file__).resolve().parents[1]
    artifact = repo / "golem/golem-temp/agents/helios_hypothesis_shard_debug.wasm"
    spec = importlib.util.spec_from_file_location(
        "news_proof_golem", repo / "scripts/verify-broker-journal.py"
    )
    assert spec and spec.loader
    support = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(support)
    server = support.IsolatedGolem(
        connection["executable"],
        private_root(root / "golem-server", create=True),
        artifact,
        app="helios-news-proof",
        component="helios:hypothesis-shard",
    )
    original_store = fast_runtime.GolemControlStore
    try:
        await asyncio.to_thread(server.start)
        await asyncio.to_thread(server.deploy)
        ports = read_private_json(server.root / "ports.json")
        subprocess.run(
            [
                connection["executable"],
                "--disable-app-manifest-discovery",
                "--config-dir",
                str(root / "golem-cli"),
                "profile",
                "new",
                connection["profile"],
                "--url",
                f"http://127.0.0.1:{ports['routerPort']}",
                "--static-token",
                support.LOCAL_DEVELOPMENT_TOKEN,
                "--default-format",
                "json",
            ],
            capture_output=True,
            check=True,
            timeout=15,
            cwd=root,
            env=server.env,
        )

        class ProofControlStore(GolemControlStore):
            @property
            def agent_address(self):
                return f"{server.app}/test/CryptoControlAgent({json.dumps(self.account)})"

        fast_runtime.GolemControlStore = ProofControlStore
        await verify_fixture(
            root,
            config,
            proof_id,
            main,
            main_settings,
            main_paper,
            ProofControlStore,
            hashlib.sha256(artifact.read_bytes()).hexdigest(),
            burst_news=burst_news,
            fomc_cut=fomc_cut,
            corroborated_reports=corroborated_reports,
            corroborating_source=corroborating_source,
            delayed_reaction=delayed_reaction,
        )
    finally:
        fast_runtime.GolemControlStore = original_store
        await asyncio.to_thread(server.close)


async def verify_fixture(
    root,
    config,
    proof_id,
    main,
    main_settings,
    main_paper,
    control_type,
    component_sha256,
    *,
    burst_news,
    fomc_cut,
    corroborated_reports,
    corroborating_source,
    delayed_reaction,
):
    store = control_type(root)
    bootstrap = read_private_json(DEFAULT_ROOT / "control-bootstrap.json")
    bootstrap["limits"] = main_settings["policy"]["limits"]
    store.initialize(bootstrap)
    store.command(ACTOR, "initialize_paper", config["paper"])

    # Every injected headline, quote, review and fill stays in this proof's namespace.
    suffix = uuid.uuid4().hex[:12].upper()
    fast_runtime.NEWS_STREAM = "HELIOS_RUNTIME_NEWS_PROOF_" + suffix
    fast_runtime.NEWS_SUBJECT = "helios.proof.news." + proof_id + ".>"
    fast_runtime.NEWS_MAX_BYTES = 1024 * 1024
    reference_stream = "HELIOS_RUNTIME_REFERENCE_PROOF_" + suffix
    reference_subject = "helios.proof.reference." + proof_id + ".>"
    assert fast_runtime.NEWS_SUBJECT.startswith("helios.proof.")
    assert reference_subject.startswith("helios.proof.")
    execution_events = []

    class IsolatedTransport:
        @classmethod
        async def connect(cls, client, control):
            transport = await PaperTransport.connect(
                client,
                control,
                stream=reference_stream,
                subject=reference_subject,
                durable="runtime-proof",
                max_bytes=1024 * 1024,
            )
            publish = transport.publish

            async def capture(event):
                execution_events.append(json.loads(canonical(event)))
                return await publish(event)

            transport.publish = capture
            return transport

    fast_runtime.PaperTransport = IsolatedTransport
    feed = (
        Feed(
            "fed-monetary",
            "https://www.federalreserve.gov/unused",
            "www.federalreserve.gov",
            "monetary_policy",
        )
        if fomc_cut
        else Feed("sec-press", "https://www.sec.gov/unused", "www.sec.gov", "regulation")
    )
    feeds = (feed,)
    if corroborated_reports:
        feed = Feed("coindesk", "https://www.coindesk.com/unused", "www.coindesk.com", "reporting")
        other_feed = next(source for source in FEEDS if source.id == corroborating_source)
        feeds = (feed, other_feed)
    fast_runtime.configured_feeds = lambda _: feeds
    config["news"]["feeds"] = [source.id for source in feeds]
    config["news"]["poll_seconds_by_source"] = {
        source: interval
        for source, interval in config["news"].get("poll_seconds_by_source", {}).items()
        if source in config["news"]["feeds"]
    }
    (root / "runtime.json").write_text(canonical(config))

    async def no_network_feed(_client, _feed, _journal, _health, stopped, _interval):
        await stopped.wait()

    fast_runtime.poll_feed = no_network_feed
    at = now_ms()
    news_age_ms = 60_000 if delayed_reaction else 24_000
    published_age_ms = news_age_ms + 1000
    intake = NewsJournal(root)
    for source_feed in feeds:
        intake.ingest(
            source_feed,
            parse_feed(
                source_feed,
                rss(
                    proof_id + "/baseline",
                    at - news_age_ms - 36_000,
                    host=source_feed.host,
                ),
            ),
            at - news_age_ms - 6000,
            {},
        )
    news_count = MAX_ACTIVE_NEWS if burst_news else 1 + int(corroborated_reports)
    last_noise_id = None
    for i in range(news_count - 1 - int(corroborated_reports)):
        prefix = f"{proof_id}/noise-{i}/"
        path = prefix + "x" * (2048 - len(f"https://{feed.host}/") - len(prefix))
        title = "Synthetic archive capacity fixture ".ljust(500, "x")
        noise = parse_feed(feed, rss(path, at - published_age_ms, title, host=feed.host))
        for item in noise:
            item["synthetic"] = True
        received = at - (1000 if corroborated_reports else news_age_ms) - news_count + i
        intake.ingest(feed, noise, received, {})
        last_noise_id = noise[0]["news_id"]
    support = []
    if corroborated_reports:
        support = parse_feed(
            other_feed,
            rss(
                proof_id + "/support",
                at - published_age_ms - 500,
                "SEC approves Bitcoin ETF trading",
                host=other_feed.host,
            ),
        )
        support[0]["synthetic"] = True
        intake.ingest(other_feed, support, at - news_age_ms - 500, {})
    release_date = datetime.fromtimestamp(
        (at - published_age_ms) / 1000, ZoneInfo("America/New_York")
    )
    path = (
        f"newsevents/pressreleases/monetary{release_date:%Y%m%d}a.htm"
        if fomc_cut
        else proof_id + "/fresh"
    )
    fresh = parse_feed(
        feed,
        rss(
            path,
            at - published_age_ms,
            FOMC_TITLE if fomc_cut else "SEC approves Bitcoin ETF",
            host=feed.host,
        ),
    )
    for item in fresh:
        item["synthetic"] = True
    intake.ingest(feed, fresh, at - news_age_ms, {})
    intake.close()
    crash_news_id = last_noise_id if corroborated_reports and last_noise_id else fresh[0]["news_id"]
    expected_evidence_ids = sorted(item["news_id"] for item in [*fresh, *support])
    source_fetches = 0

    async def synthetic_statement(_client, staged, clock):
        nonlocal source_fetches
        source_fetches += 1
        assert staged["news_id"] == fresh[0]["news_id"]
        await asyncio.sleep(0.01)
        raw = (
            f'<p class="article__time">{release_date:%B %d, %Y}</p>'
            f'<h3 class="title">{FOMC_TITLE}</h3><p>The Committee decided to lower '
            "the target range for the federal funds rate by 1/4 percentage point "
            "to 3-1/4 to 3-1/2 percent.</p>"
        ).encode()
        return parse_statement(staged, raw, clock())

    if fomc_cut:
        fast_runtime.fetch_statement = synthetic_statement
    model_calls = 0
    review_records = []
    dropped = False
    completed = asyncio.Event()
    stops = []
    first_quote_ms = None

    async def review(_root, _store, _config, event, *, identity, expires_ms, **_):
        nonlocal model_calls
        model_calls += 1
        assert event["news_id"] == fresh[0]["news_id"]
        assert event["evidence_news_ids"] == expected_evidence_ids
        assert {item["news_id"] for item in event["other_reports"]} == {
            item["news_id"] for item in support
        }
        assert expires_ms == at - published_age_ms - (500 if corroborated_reports else 0) + 120_000
        review_records.append(
            {
                "identity": identity,
                "evidence_news_ids": event["evidence_news_ids"],
                "expires_ms": expires_ms,
                "event_bytes": len(canonical(event).encode()),
            }
        )
        await asyncio.sleep(0.3)
        return {"provider": "synthetic-review-only", "veto_new_risk": False}

    async def quotes(book, symbols, changed, stopped):
        nonlocal first_quote_ms
        stops.append(stopped)
        if corroborated_reports and len(stops) == 1:
            # The matching reports precede the newer noise. Defer quotes until
            # restart so the first process cannot enter before the crash marker.
            await stopped.wait()
            return
        book.connected = True
        book.channel = "synthetic"
        book.connections = 1
        start = now_ms()
        first_quote_ms = start
        sequence = 0

        def accept(stamp, price):
            nonlocal sequence
            sequence += 1
            book.accept(
                {
                    "symbol": "BTC-USD",
                    "bid_micros": price,
                    "ask_micros": price + 10_000,
                    "observed_ms": stamp,
                    "received_ms": stamp,
                    "exchange_ms": stamp,
                    "sequence": sequence,
                    "channel": "synthetic",
                }
            )
            changed.set()

        history_seconds = 100 if delayed_reaction else 35
        for age in range(history_seconds, -1, -1):
            progress = max(0, 35 - age)
            accept(start - age * 1000, 100_000_000 + progress * 1_000_000 // 35)
        while not stopped.is_set():
            accept(now_ms(), 95_000_000 if dropped else 101_000_000)
            if completed.is_set():
                stopped.set()
            await asyncio.sleep(0.1)

    fast_runtime.watch_quotes = quotes
    fast_runtime.review_event = review
    acknowledged = asyncio.Event()

    class CrashAfterDelivery(NewsJournal):
        def delivered(self, news_id):
            super().delivered(news_id)
            if news_id == crash_news_id:
                acknowledged.set()
                # Interrupt after the last real ACK, before memory activation.
                raise asyncio.CancelledError()

    fast_runtime.NewsJournal = CrashAfterDelivery
    interrupted = asyncio.create_task(fast_runtime.run(root))
    try:
        await asyncio.wait_for(acknowledged.wait(), 8)
    except TimeoutError:
        if interrupted.done():
            await interrupted
        raise
    finally:
        interrupted.cancel()
        await asyncio.gather(interrupted, return_exceptions=True)
        fast_runtime.NewsJournal = NewsJournal
    assert store.status(ACTOR)["paper"]["total_orders"] == 0
    recovered_intake = NewsJournal(root)
    try:
        assert not recovered_intake.pending()
        recovered = recovered_intake.recent_published(now_ms(), {source.id for source in feeds})
        assert len(recovered) == news_count
        recovered_anchor = next(
            item for item in recovered if item["news_id"] == fresh[0]["news_id"]
        )
        assert recovered_anchor["observed_ms"] == at - news_age_ms
        if fomc_cut:
            assert recovered_anchor["source_facts"]["action"] == "lower"
    finally:
        recovered_intake.close()
    runner = asyncio.create_task(fast_runtime.run(root))
    deadline = time.monotonic() + 50

    def read_selected_candidate(reader):
        cursor = None
        while True:
            report = read_news_evidence(reader, "observer", limit=50, before_sequence=cursor)
            for candidate in report["evidence"]["candidates"]:
                if candidate["news"]["news_id"] == fresh[0]["news_id"]:
                    return candidate
            cursor = report["evidence"]["next_before_sequence"]
            if cursor is None:
                return None

    try:
        while time.monotonic() < deadline:
            if runner.done():
                await runner
                raise AssertionError("runtime ended before proof completed")
            state = await asyncio.to_thread(store.status, ACTOR)
            if state["paper"]["total_orders"] >= 1:
                dropped = True
            if state["paper"]["total_orders"] >= 2:
                candidate = await asyncio.to_thread(read_selected_candidate, store)
                if candidate and candidate["markouts"]:
                    completed.set()
                    break
            await asyncio.sleep(0.5)
        assert completed.is_set(), "full runtime proof deadline exceeded"
        await asyncio.wait_for(runner, 5)
    finally:
        for stop in stops:
            stop.set()
        if not runner.done():
            runner.cancel()
        await asyncio.gather(runner, return_exceptions=True)

    state = store.status(ACTOR)
    paper = state["paper"]
    runtime_status = state["runtime"]["crypto-shadow"]["status"]
    assert runtime_status["news_recovered"] == news_count
    assert runtime_status["news_published"] == 0
    assert paper["total_orders"] == 2
    assert [fill["side"] for fill in paper["recent_fills"]] == ["buy", "sell"]
    assert paper["positions"]["BTC-USD"] <= 1
    assert model_calls == 1
    assert source_fetches == int(fomc_cut)
    assert state["decision_budget"]["reserved_calls"] == 0
    candidate = read_selected_candidate(store)
    assert candidate["synthetic_inputs"] is True
    assert candidate["reference_source"] == "synthetic"
    if fomc_cut:
        assert candidate["news"]["source_facts"]["stated_change_bps"] == -25
    decisions = [transition["decision"] for transition in candidate["transitions"]]
    reviewed = next(item for item in decisions if item["stage"] == "review_finished")
    assert reviewed["evidence_news_ids"] == expected_evidence_ids
    assert reviewed["review_id"] == review_records[0]["identity"]
    assert reviewed["review_expires_ms"] == review_records[0]["expires_ms"]
    assert any(d.get("reason") == "model_pending" for d in decisions)
    assert any(d.get("stage") == "submission" for d in decisions)
    assert any(d.get("outcome") == "simulated_fill" for d in decisions)
    assert any(d.get("action") == "exit" for d in decisions)
    assert candidate["matches_in_recent_golem_paper_fills"]
    latency = candidate["latency_ms"]
    assert latency["publication_to_intake"] == 1000
    assert latency["review_pending_to_result"] >= 300
    assert latency["intake_to_first_fill_observed"] >= latency["intake_to_first_submission_intent"]
    captured = read_decision_snapshots(store, "observer")["snapshots"]
    entry_event = next(e for e in execution_events if e["context"]["action"] == "enter")
    exit_event = next(e for e in execution_events if e["context"]["action"] == "exit")
    entry_input = next(
        item for item in captured if item["snapshot_id"] == entry_event["signal"]["input_digest"]
    )
    assert entry_input["phase"] == "entry_proposal"
    assert entry_event["signal"]["expires_ms"] == reviewed["review_expires_ms"]
    assert entry_event["signal"]["available_ms"] == reviewed["review_expires_ms"] - 120_000
    assert entry_event["available_ms"] < entry_event["signal"]["expires_ms"]
    assert paper["recent_fills"][0]["at_ms"] < entry_event["signal"]["expires_ms"]
    assert paper["signal_deadline_version"] == 1
    assert exit_event["signal"] is None
    exit_input = next(item for item in captured if item["phase"] == "exit_proposal")
    entry_snapshot = read_decision_input(store, "observer", entry_input["snapshot_id"])
    entry_input_bytes = len(canonical(entry_snapshot["context"]).encode())
    assert len(entry_snapshot["context"]["news"]) == news_count
    if burst_news:
        assert entry_input_bytes > 64 * 1024
    entry_replay = replay_local_proposal(root, entry_snapshot, fee_bps=200)
    exit_replay = replay_local_proposal(
        root, read_decision_input(store, "observer", exit_input["snapshot_id"])
    )
    assert entry_replay["matches_original"] and exit_replay["matches_original"]
    assert entry_replay["not_strategy_pnl"] and entry_replay["synthetic_inputs"]
    first_markout = candidate["markouts"][0]
    assert first_markout["status"] == "observed"
    assert first_markout["reference_return_bps_after_costs"] < 0
    assert main.settings("observer") == main_settings
    after = main.status("observer")["paper"]
    for key in ("cash_micros", "positions", "total_orders", "fees_micros"):
        assert after[key] == main_paper[key], "main paper account changed during isolated proof"
    # The same read API used by MCP must recover the evidence from a fresh store instance.
    assert read_selected_candidate(control_type(root)) == candidate
    proof = {
        "schema_version": 1,
        "verified_ms": now_ms(),
        "proof_account": proof_id,
        "simulation_only": True,
        "synthetic_news_and_prices": True,
        "model_review_mocked": True,
        "full_runtime_entry_and_local_exit": True,
        "restart_after_news_ack_before_activation": True,
        "recovered_original_headline_without_republication": True,
        "recovered_headlines": news_count,
        "fomc_statement_fixture": fomc_cut,
        "corroborated_reporting_fixture": corroborated_reports,
        "fixture_sources": [source.id for source in feeds],
        "delayed_reaction_fixture": delayed_reaction,
        "news_age_at_fixture_start_ms": news_age_ms,
        "review_evidence": review_records,
        "synthetic_source_fetches": source_fetches,
        "simulated_fills": paper["total_orders"],
        "paid_api_calls": 0,
        "live_orders": 0,
        "model_reviews_mocked": model_calls,
        "news_and_execution_streams_isolated": True,
        "golem_server_isolated": True,
        "control_component_sha256": component_sha256,
        "main_settings_and_paper_cash_preserved": True,
        "durable_decision_transitions": True,
        "original_news_deadline_enforced_at_durable_fill": True,
        "entry_signal": entry_event["signal"],
        "reaction_latency_ms": latency,
        "decision_inputs": {
            "news_count": len(entry_snapshot["context"]["news"]),
            "context_bytes": entry_input_bytes,
            "entry_snapshot_id": entry_input["snapshot_id"],
            "exit_snapshot_id": exit_input["snapshot_id"],
            "entry_and_exit_reproduced": True,
            "fee_scenario": entry_replay["fee_scenario"],
            "not_strategy_pnl": True,
        },
        "forward_30_second_measurement": first_markout,
        "same_reader_as_mcp": True,
        "elapsed_ms": now_ms() - first_quote_ms,
        "strategy_fingerprint": candidate["strategy_fingerprint"],
    }
    (DEFAULT_ROOT / "news-runtime-proof.json").write_text(json.dumps(proof, indent=2) + "\n")
    print(json.dumps(proof, indent=2))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--strategy-path", help="Private candidate source; used only in the proof")
    parser.add_argument("--strategy-config-path", help="Private candidate parameters")
    parser.add_argument(
        "--burst-news", action="store_true", help="Exercise the full supported news window"
    )
    parser.add_argument(
        "--fomc-cut", action="store_true", help="Exercise durable official-source detail extraction"
    )
    parser.add_argument(
        "--corroborated-reports",
        action="store_true",
        help="Keep matching reports in model review despite newer unrelated headlines",
    )
    parser.add_argument(
        "--corroborating-source",
        choices=("cointelegraph", "decrypt"),
        default="cointelegraph",
        help="Second reporting feed for the corroborated-reports fixture",
    )
    parser.add_argument(
        "--delayed-reaction",
        action="store_true",
        help="Preserve the news-time price when confirmation follows an older eligible headline",
    )
    arguments = parser.parse_args()
    if (arguments.strategy_path is None) != (arguments.strategy_config_path is None):
        parser.error("--strategy-path and --strategy-config-path must be supplied together")
    if arguments.fomc_cut and arguments.corroborated_reports:
        parser.error("choose --fomc-cut or --corroborated-reports")
    asyncio.run(
        verify(
            arguments.strategy_path,
            arguments.strategy_config_path,
            burst_news=arguments.burst_news,
            fomc_cut=arguments.fomc_cut,
            corroborated_reports=arguments.corroborated_reports,
            corroborating_source=arguments.corroborating_source,
            delayed_reaction=arguments.delayed_reaction,
        )
    )
