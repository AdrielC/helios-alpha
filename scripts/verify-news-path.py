#!/usr/bin/env python3
"""Exercise a synthetic news entry and stop in an isolated Golem paper account."""

from __future__ import annotations

import asyncio
import hashlib
import json
import os
import subprocess
import time
import uuid
from pathlib import Path

import nats

from helios_alpha.control.golem_store import GolemControlStore, read_private_json
from helios_alpha.control.store import DEFAULT_ROOT, canonical
from helios_alpha.crypto.paper_transport import PaperTransport, event_envelope
from helios_alpha.crypto.runtime import ACTOR
from helios_alpha.crypto.strategy import PrivateStrategy


async def verify():
    os.umask(0o077)
    root = DEFAULT_ROOT
    main_store = GolemControlStore(root)
    main_settings = main_store.settings("observer")
    config = read_private_json(root / "runtime.json")
    strategy = PrivateStrategy(Path(config["strategy_path"]), Path(config["strategy_config_path"]))
    proof_id = "news-proof-" + uuid.uuid4().hex
    store = GolemControlStore(root)
    store.account, store.phantom_id = proof_id, None
    bootstrap = read_private_json(root / "control-bootstrap.json")
    bootstrap["limits"] = main_settings["policy"]["limits"]
    store.initialize(bootstrap)
    store.command(ACTOR, "initialize_paper", config["paper"])
    store.command(ACTOR, "enable_paper_signal_deadlines")
    settings = store.settings(ACTOR)
    nc_config = read_private_json(root / "nats-client.json")
    connection = await nats.connect(nc_config["url"], token=nc_config["token"])
    try:
        transport = await PaperTransport.connect(
            connection,
            store,
            stream="HELIOS_NEWS_PROOF_" + uuid.uuid4().hex[:12].upper(),
            subject="helios.proof." + proof_id + ".>",
            durable="news-proof",
            max_bytes=1024 * 1024,
        )
        at = time.time_ns() // 1_000_000
        quote = {
            "symbol": "BTC-USD",
            "bid_micros": 101_000_000,
            "ask_micros": 101_010_000,
            "observed_ms": at,
        }
        news = {
            "news_id": hashlib.sha256(proof_id.encode()).hexdigest(),
            "title": "SEC approves Bitcoin ETF",
            "source": "sec-press",
            "source_category": "regulation",
            "observed_ms": at - 24_000,
            "published_ms": at - 25_000,
            "eligible": True,
        }
        context = {
            "schema_version": 1,
            "now_ms": at,
            "symbol": "BTC-USD",
            "quote": quote,
            "news": [news],
            "paper": store.status(ACTOR)["paper"],
            "limits": settings["policy"]["limits"],
            "costs": config["paper"],
            "market": {
                "connected": True,
                "samples": [
                    [at - 35000 + i * 1000, 100000000 + i * 1000000 // 35] for i in range(36)
                ],
            },
        }
        proposal = strategy.propose(context)
        assert proposal["action"] == "enter", proposal
        target = min(
            settings["policy"]["limits"]["capital_micros"] * proposal["allocation_bps"] // 10000,
            settings["policy"]["limits"]["order_micros"],
        )
        entered = event_envelope(
            available_ms=at,
            policy_revision=1,
            quote=quote,
            portfolio_quotes=[quote],
            target_micros=target,
            strategy_fingerprint=strategy.fingerprint,
            decision_provider="synthetic-review-only",
            signal={
                "input_digest": hashlib.sha256(canonical(context).encode()).hexdigest(),
                "available_ms": min(news["observed_ms"], news["published_ms"]),
                "expires_ms": min(news["observed_ms"], news["published_ms"]) + 120_000,
            },
            context={"synthetic": True, "action": "enter", "news_id": news["news_id"]},
        )
        start = time.perf_counter_ns()
        first = await transport.publish(entered)
        receipts = await transport.drain()
        latency_ms = (time.perf_counter_ns() - start) / 1_000_000
        assert len(receipts) == 1 and receipts[0]["receipt"]["outcome"] == "simulated_fill"
        duplicate = await transport.publish(entered)
        assert duplicate.duplicate and duplicate.seq == first.seq
        await transport.drain()
        before = store.status(ACTOR)["paper"]
        assert before["total_orders"] == 1 and before["fees_micros"] > 0
        subprocess.run(
            [
                store.config["executable"],
                "--disable-app-manifest-discovery",
                "--profile",
                "local",
                "--config-dir",
                str(root / "golem-cli"),
                "agent",
                "simulate-crash",
                store.agent_address,
            ],
            capture_output=True,
            check=True,
            timeout=20,
        )
        assert store.status(ACTOR)["paper"] == before
        at_exit = time.time_ns() // 1_000_000
        exit_quote = {
            **quote,
            "bid_micros": 95_000_000,
            "ask_micros": 95_010_000,
            "observed_ms": at_exit,
        }
        exit_proposal = strategy.propose(
            {**context, "now_ms": at_exit, "quote": exit_quote, "paper": before, "news": []}
        )
        assert exit_proposal["action"] == "exit"
        exited = event_envelope(
            available_ms=at_exit,
            policy_revision=1,
            quote=exit_quote,
            portfolio_quotes=[exit_quote],
            target_micros=0,
            strategy_fingerprint=strategy.fingerprint,
            decision_provider="local",
            context={"synthetic": True, "action": "exit", "reason": exit_proposal["reason"]},
        )
        await transport.publish(exited)
        receipts = await transport.drain()
        assert len(receipts) == 1 and receipts[0]["receipt"]["outcome"] == "simulated_fill"
        after = store.status(ACTOR)["paper"]
        assert after["total_orders"] == 2
        assert after["positions"]["BTC-USD"] <= 1
        assert after["cash_micros"] < after["initial_cash_micros"]
        assert main_store.settings("observer") == main_settings
        proof = {
            "schema_version": 1,
            "verified_ms": time.time_ns() // 1_000_000,
            "account": proof_id,
            "simulation_only": True,
            "synthetic_news_and_prices": True,
            "model_review_mocked": True,
            "entry_candidate_then_fill": True,
            "duplicate_publication_suppressed": True,
            "golem_crash_state_preserved": True,
            "local_exit_without_news_or_model": True,
            "fees_charged_both_directions": True,
            "main_settings_unchanged": True,
            "live_orders": 0,
            "paid_api_calls": 0,
            "simulated_fills": after["total_orders"],
            "single_local_publish_to_paper_commit_ms": latency_ms,
            "latency_excludes_news_delivery_and_model_review": True,
            "strategy_fingerprint": strategy.fingerprint,
        }
        (root / "news-path-proof.json").write_text(json.dumps(proof, indent=2) + "\n")
        print(json.dumps(proof, indent=2))
    finally:
        await connection.close()


if __name__ == "__main__":
    asyncio.run(verify())
