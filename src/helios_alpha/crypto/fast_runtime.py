"""Streaming news/price research with durable paper execution; no live order adapter."""

from __future__ import annotations

import asyncio
import fcntl
import hashlib
import json
import os
import signal
import time
from collections import deque
from pathlib import Path

import httpx
import nats

from helios_alpha.control.golem_store import GolemControlStore, read_private_json
from helios_alpha.control.store import canonical, private_root
from helios_alpha.signals.publisher import ensure_stream

from .decisions import review_event
from .evidence import NewsEvidence
from .fomc import fetch_statement
from .market import SYMBOLS
from .news import (
    MAX_ACTIVE_NEWS,
    MAX_NEWS_AGE_MS,
    NewsJournal,
    configured_feeds,
    fetch_feed,
    fresh_news,
    parse_feed,
    polling_intervals,
    stamp_ms,
)
from .news_review import prepare_news_review
from .paper_transport import PaperTransport, event_envelope
from .runtime import ACTOR
from .snapshots import DecisionSnapshots
from .strategy import PrivateStrategy
from .streaming import QuoteBook, watch_quotes

NEWS_STREAM = "HELIOS_CRYPTO_NEWS_V1"
NEWS_SUBJECT = "helios.crypto.news.v1.>"
NEWS_MAX_BYTES = 32 * 1024 * 1024


def now_ms():
    return time.time_ns() // 1_000_000


def retry_delay(headers: dict, now: int, default: int) -> int:
    value = headers.get("retry-after", "")
    try:
        delay = int(value) if value.isdigit() else (stamp_ms(value) - now) // 1000
        return max(default, min(delay, 3600))
    except (ValueError, TypeError, OverflowError):
        return default


async def sleep_or_stop(stopped, delay):
    try:
        await asyncio.wait_for(stopped.wait(), timeout=delay)
    except TimeoutError:
        pass


async def poll_feed(client, feed, journal, health, stopped, interval):
    failures = 0
    while not stopped.is_set():
        delay = interval
        started = time.perf_counter_ns()
        cache_age = None
        try:
            code, headers, raw = await fetch_feed(client, feed, journal.headers(feed))
            at = now_ms()
            age_header = headers.get("age", "")
            if age_header.isascii() and age_header.isdigit() and len(age_header) <= 10:
                cache_age = int(age_header)
            if code == 200:
                intake = journal.ingest(feed, parse_feed(feed, raw), at, headers)
                health[feed.id] = {"state": "running", "last_ok_ms": at, **intake}
                failures = 0
            elif code == 304:
                health[feed.id] = {"state": "running", "last_ok_ms": at, "not_modified": True}
                failures = 0
            else:
                failures += 1
                delay = retry_delay(headers, at, min(300, interval * 2 ** min(failures, 4)))
                health[feed.id] = {"state": "deferred", "http_status": code, "retry_seconds": delay}
        except asyncio.CancelledError:
            raise
        except Exception as error:
            failures += 1
            delay = min(300, interval * 2 ** min(failures, 4))
            health[feed.id] = {"state": "degraded", "error_type": type(error).__name__}
        health[feed.id].update(
            poll_seconds=interval,
            next_poll_ms=now_ms() + delay * 1000,
            fetch_ms=(time.perf_counter_ns() - started) // 1_000_000,
            response_cache_age_seconds=cache_age,
            consecutive_failures=failures,
        )
        await sleep_or_stop(stopped, delay)


async def publish_news(journal, js, arrived, stopped, metrics):
    while not stopped.is_set():
        try:
            for item in journal.pending():
                ack = await js.publish(
                    NEWS_SUBJECT.removesuffix(">") + item["source"],
                    canonical(item).encode(),
                    headers={"Nats-Msg-Id": item["news_id"]},
                )
                if ack.stream != NEWS_STREAM:
                    raise ValueError("unexpected news acknowledgement")
                # Startup recovers acknowledged headlines using their original clocks.
                journal.delivered(item["news_id"])
                if fresh_news([item], now_ms()):
                    arrived.append(item)
                metrics["news_published"] += 1
            metrics["news_outbox"] = "ready"
        except asyncio.CancelledError:
            raise
        except Exception as error:
            metrics["news_outbox"] = type(error).__name__
        await sleep_or_stop(stopped, 0.25)


async def enrich_news(client, journal, stopped, metrics):
    retries = {}
    while not stopped.is_set():
        try:
            pending = journal.pending_source_details()
            active_ids = {item["news_id"] for item in pending}
            retries = {key: value for key, value in retries.items() if key in active_ids}
            health = metrics["source_details"]
            health.update(state="pending" if pending else "idle", pending=len(pending))
            for item in pending:
                at = now_ms()
                if not fresh_news([item], at):
                    journal.finish_source_details(item["news_id"], None, at)
                    health["expired"] = health.get("expired", 0) + 1
                    continue
                failures, due = retries.get(item["news_id"], (0, 0))
                if at < due:
                    continue
                try:
                    facts = await fetch_statement(client, item, now_ms)
                    journal.finish_source_details(item["news_id"], facts, now_ms())
                    health.update(last_success_ms=facts["fetched_ms"], last_error_type=None)
                    retries.pop(item["news_id"], None)
                except asyncio.CancelledError:
                    raise
                except Exception as error:
                    failures += 1
                    retries[item["news_id"]] = (
                        failures,
                        now_ms() + min(15_000, 250 * 2 ** min(failures, 6)),
                    )
                    health.update(state="retrying", last_error_type=type(error).__name__)
        except asyncio.CancelledError:
            raise
        except Exception as error:
            metrics["source_details"].update(state="degraded", last_error_type=type(error).__name__)
        await sleep_or_stop(stopped, 0.25)


async def run(root: Path):
    os.umask(0o077)
    root = private_root(root)
    lock = (root / "crypto-runtime.lock").open("a+")
    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    config = read_private_json(root / "runtime.json")
    if config.get("schema_version") != 1 or config.get("mode") != "paper":
        raise ValueError("news engine supports paper simulation only")
    symbols = config["symbols"]
    if not symbols or len(set(symbols)) != len(symbols) or not set(symbols) <= SYMBOLS:
        raise ValueError("invalid streaming symbols")
    feeds = configured_feeds(root)
    intervals = polling_intervals(config["news"], feeds)
    strategy = PrivateStrategy(Path(config["strategy_path"]), Path(config["strategy_config_path"]))
    store = GolemControlStore(root)
    await asyncio.to_thread(store.command, ACTOR, "initialize_paper", config["paper"])
    initialized = await asyncio.to_thread(store.command, ACTOR, "enable_paper_signal_deadlines")
    if initialized.get("signal_deadline_version") != 1:
        raise ValueError("paper control component requires a signal-deadline update")
    nc_config = read_private_json(root / "nats-client.json")
    connection = await nats.connect(
        nc_config["url"], token=nc_config["token"], connect_timeout=3, max_reconnect_attempts=3
    )
    transport = await PaperTransport.connect(connection, store)
    await ensure_stream(transport.js, NEWS_STREAM, [NEWS_SUBJECT], max_bytes=NEWS_MAX_BYTES)
    journal = NewsJournal(root)
    stopped, changed = asyncio.Event(), asyncio.Event()
    for sig in (signal.SIGINT, signal.SIGTERM):
        asyncio.get_running_loop().add_signal_handler(sig, stopped.set)
    book = QuoteBook()
    arrived: deque = deque(maxlen=MAX_ACTIVE_NEWS)
    sources = {feed.id for feed in feeds}
    active = {item["news_id"]: item for item in journal.recent_published(now_ms(), sources)}
    reviews: dict[str, asyncio.Task] = {}
    review_expiry = {}
    feed_health = {}
    metrics = {
        "news_published": 0,
        "news_recovered": len(active),
        "news_outbox": "starting",
        "paper_commits": 0,
        "model_candidates": 0,
        "last_reasons": {},
        "reference_source": "coinbase",
        "broker_orders": 0,
        "evidence": {"state": "starting"},
        "decision_inputs": {"state": "waiting_for_input"},
        "source_details": {"state": "starting" if "fed-monetary" in sources else "disabled"},
    }
    evidence = None
    snapshots = None
    evidence_ready = False
    tasks = []
    next_refresh = next_heartbeat = next_observation = next_evidence = 0.0
    state = settings = None
    tick_durations: deque = deque(maxlen=1000)
    exit_deferred_until = {}

    def capture_inputs(context_wire, proposal, phase):
        nonlocal snapshots
        try:
            if snapshots is None:
                snapshots = DecisionSnapshots(root, strategy)
            identity = snapshots.capture(context_wire, proposal, phase, book.channel)
            metrics["decision_inputs"] = {"state": "ready", "latest_snapshot_id": identity}
            return identity
        except Exception as error:
            metrics["decision_inputs"] = {"state": "degraded", "error_type": type(error).__name__}
            return None

    def record_evidence(news_ids, symbol, transition, at):
        nonlocal evidence_ready
        if not news_ids:
            return True
        try:
            if evidence is None:
                return False
            return evidence.record(news_ids, symbol, strategy.fingerprint, transition, at) > 0
        except Exception as error:
            evidence_ready = False
            metrics["evidence"] = {"state": "degraded", "error_type": type(error).__name__}
            return False

    def collect_receipts(committed):
        for item in committed:
            metrics["paper_commits"] += 1
            metrics["last_receipt"] = item["receipt"]
            print(json.dumps({"event": "news_paper_commit", **item["receipt"]}), flush=True)
            event, receipt = item["event"], item["receipt"]
            news_id = event.get("context", {}).get("news_id")
            if news_id and event["strategy_fingerprint"] == strategy.fingerprint:
                record_evidence(
                    [news_id],
                    event["quote"]["symbol"],
                    {
                        "stage": "commit",
                        "event_id": event["event_id"],
                        "outcome": receipt["outcome"],
                        "cursor": receipt["cursor"],
                    },
                    now_ms(),
                )

    async def tracked_review(event, identity, expires_ms):
        ids, symbol = [event["news_id"]], event["symbol"]
        binding = {
            "review_id": identity,
            "evidence_news_ids": event["evidence_news_ids"],
            "review_expires_ms": expires_ms,
        }
        if not record_evidence(ids, symbol, {"stage": "review_started", **binding}, now_ms()):
            return {"provider": "local", "veto_new_risk": True}
        review = await review_event(
            root,
            store,
            config["decisions"],
            event,
            fallback_veto=True,
            identity=identity,
            expires_ms=expires_ms,
        )
        record_evidence(
            ids,
            symbol,
            {"stage": "review_finished", "provider": review["provider"], **binding},
            now_ms(),
        )
        return review

    async def heartbeat(state_name):
        ages = {
            symbol: max(0, now_ms() - value["observed_ms"]) for symbol, value in book.latest.items()
        }
        ordered = sorted(tick_durations)
        await asyncio.to_thread(
            store.heartbeat,
            ACTOR,
            "crypto-shadow",
            {
                "state": state_name,
                "mode": "paper",
                "engine": "news_shock",
                "feed_connected": book.connected,
                "connections": book.connections,
                "ticks": book.ticks,
                "rejected_ticks": book.rejected,
                "reference_channel": book.channel,
                "orderbook_snapshots": book.snapshots,
                "feed_resets": book.protocol_resets,
                "last_feed_error": book.last_feed_error,
                "quote_age_ms": ages,
                "feeds": feed_health,
                **metrics,
                "active_news": len(active),
                "decisions_enabled": config["decisions"]["enabled"],
                "strategy_fingerprint": strategy.fingerprint,
                "local_rule_p95_ms": ordered[int((len(ordered) - 1) * 0.95)] if ordered else None,
            },
        )

    try:
        async with httpx.AsyncClient(
            timeout=8,
            follow_redirects=False,
            trust_env=False,
            headers={
                "User-Agent": "HeliosAlphaResearch/0.1 (+https://github.com/AdrielC/helios-alpha)"
            },
        ) as client:
            tasks = [
                asyncio.create_task(watch_quotes(book, symbols, changed, stopped)),
                asyncio.create_task(publish_news(journal, transport.js, arrived, stopped, metrics)),
                *[
                    asyncio.create_task(
                        poll_feed(client, feed, journal, feed_health, stopped, intervals[feed.id])
                    )
                    for feed in feeds
                ],
            ]
            if "fed-monetary" in sources:
                tasks.append(asyncio.create_task(enrich_news(client, journal, stopped, metrics)))
            while not stopped.is_set():
                started = time.monotonic()
                state_name = "running"
                try:
                    # Reconcile unknown publication outcomes before making another proposal.
                    committed = await transport.drain()
                    collect_receipts(committed)
                    for entry_id, pending_event in journal.pending_entries():
                        ack = await transport.publish(pending_event)
                        recovered = await transport.drain()
                        collect_receipts(recovered)
                        recovered_state = await asyncio.to_thread(store.status, ACTOR)
                        if recovered_state["paper"]["cursor"] < ack.seq:
                            raise ValueError("pending entry outcome not yet reconciled")
                        journal.entry_completed(entry_id)
                        next_refresh = 0
                    if committed or started >= next_refresh:
                        settings = await asyncio.to_thread(store.settings, ACTOR)
                        state = await asyncio.to_thread(store.status, ACTOR)
                        next_refresh = time.monotonic() + 5
                    at = now_ms()
                    arrived_now = bool(arrived)
                    while arrived:
                        item = arrived.popleft()
                        if item.get("source") in sources:
                            active[item["news_id"]] = item
                    active = {
                        item["news_id"]: item
                        for item in sorted(
                            fresh_news(list(active.values()), at),
                            key=lambda item: (item["observed_ms"], item["news_id"]),
                        )[-MAX_ACTIVE_NEWS:]
                    }
                    for identity in list(reviews):
                        if at > review_expiry[identity]:
                            reviews.pop(identity).cancel()
                            del review_expiry[identity]
                    limits, revision = settings["policy"]["limits"], settings["policy"]["revision"]
                    if started >= next_evidence or arrived_now:
                        try:
                            if evidence is None:
                                evidence = NewsEvidence(root)
                            evidence.observe(
                                list(active.values()),
                                book.latest if book.connected else {},
                                symbols,
                                strategy.fingerprint,
                                settings["paper_costs"],
                                revision,
                                at,
                            )
                            evidence_ready = True
                            metrics["evidence"] = {"state": "running", "last_observation_ms": at}
                        except Exception as error:
                            evidence_ready = False
                            metrics["evidence"] = {
                                "state": "degraded",
                                "error_type": type(error).__name__,
                            }
                        next_evidence = time.monotonic() + 1
                    quotes = book.available_quotes(
                        symbols, at, min(5000, limits["max_quote_age_ms"])
                    )
                    unavailable = set(symbols) - {q["symbol"] for q in quotes}
                    for symbol in unavailable:
                        metrics["last_reasons"][symbol] = "reference_quote_unavailable"
                        record_evidence(
                            list(active),
                            symbol,
                            {
                                "stage": "evaluation",
                                "action": "hold",
                                "reason": "reference_quote_unavailable",
                                "policy_revision": revision,
                            },
                            at,
                        )
                    if not quotes:
                        state_name = "waiting_for_quotes"
                    if settings["paper_costs"] != config["paper"]:
                        raise ValueError("paper costs changed; restart with reconciled settings")
                    observation_due = started >= next_observation
                    for quote in quotes:
                        symbol = quote["symbol"]
                        paper = state["paper"]
                        context = {
                            "schema_version": 1,
                            "symbol": symbol,
                            "now_ms": at,
                            "quote": quote,
                            "market": book.context(symbol, at),
                            "news": sorted(active.values(), key=lambda item: item["observed_ms"]),
                            "paper": paper,
                            "limits": limits,
                            "costs": settings["paper_costs"],
                        }
                        # Freeze inputs before owner-selected strategy code can mutate them.
                        context_wire = (
                            canonical(context) if active or paper["positions"].get(symbol) else None
                        )
                        rule_start = time.perf_counter_ns()
                        proposal = strategy.propose(context)
                        tick_durations.append((time.perf_counter_ns() - rule_start) / 1_000_000)
                        action = proposal["action"]
                        reason = proposal["reason"]
                        if active:
                            capture_inputs(context_wire, proposal, "evaluation")
                        entry_snapshot = None
                        if action == "enter":
                            entry_snapshot = capture_inputs(
                                context_wire, proposal, "entry_proposal"
                            )
                        elif action == "exit":
                            capture_inputs(context_wire, proposal, "exit_proposal")
                        if action == "exit" and at < exit_deferred_until.get(symbol, 0):
                            action = "hold"
                            reason = "exit_retry_deferred"
                        review = {"provider": "local", "veto_new_risk": False}
                        selected = None
                        signal_validity = None
                        candidate_entry_id = None
                        if action == "enter":
                            selected = active.get(proposal["news_id"])
                            if selected is None or not fresh_news([selected], now_ms()):
                                raise ValueError("entry references unavailable news")
                            candidate_entry_id = hashlib.sha256(
                                f"{selected['news_id']}\n{symbol}".encode()
                            ).hexdigest()
                            if entry_snapshot is None:
                                action, reason = "hold", "decision_inputs_unavailable"
                            elif not evidence_ready:
                                action, reason = "hold", "evidence_unavailable"
                            elif settings["execution"]["paused"]:
                                action, reason = "hold", "policy_paused"
                            elif paper["positions"].get(symbol, 0):
                                action = "hold"
                                reason = "position_already_held"
                            elif journal.entry_attempted(candidate_entry_id):
                                action, reason = "hold", "entry_already_attempted"
                            else:
                                try:
                                    prepared = prepare_news_review(
                                        proposal,
                                        json.loads(context_wire),
                                        strategy.fingerprint,
                                        now_ms(),
                                    )
                                except ValueError:
                                    prepared = None
                                    action, reason = "hold", "review_evidence_unavailable"
                                if prepared is not None:
                                    signal_validity = {
                                        "input_digest": entry_snapshot,
                                        "available_ms": prepared["expires_ms"] - MAX_NEWS_AGE_MS,
                                        "expires_ms": prepared["expires_ms"],
                                    }
                            if action == "enter" and proposal["unusual_event"]:
                                identity = prepared["identity"] if prepared else None
                                if prepared is None:
                                    pass
                                elif identity not in reviews:
                                    reason = "model_capacity"
                                    if sum(not task.done() for task in reviews.values()) < 2:
                                        reason = "model_pending"
                                        expires = prepared["expires_ms"]
                                        reviews[identity] = asyncio.create_task(
                                            tracked_review(
                                                prepared["event"],
                                                identity=identity,
                                                expires_ms=expires,
                                            )
                                        )
                                        review_expiry[identity] = expires
                                        metrics["model_candidates"] += 1
                                    action = "hold"
                                elif not reviews[identity].done():
                                    action = "hold"
                                    reason = "model_pending"
                                else:
                                    review = reviews[identity].result()
                                    if review["veto_new_risk"]:
                                        action = "hold"
                                        reason = (
                                            "model_veto"
                                            if review["provider"] == "openai"
                                            else "model_unavailable"
                                        )
                        recorded = record_evidence(
                            list(active),
                            symbol,
                            {
                                "stage": "evaluation",
                                "action": action,
                                "reason": reason,
                                "proposed_action": proposal["action"],
                                "proposed_reason": proposal["reason"],
                                "provider": review["provider"],
                                "policy_revision": revision,
                                "selected_news_id": selected["news_id"] if selected else None,
                            },
                            at,
                        )
                        if action == "enter" and not recorded:
                            action, reason = "hold", "evidence_unavailable"
                        metrics["last_reasons"][symbol] = reason
                        # Quote updates and exit checks continue while model tasks are pending.
                        held = paper["positions"].get(symbol, 0) * quote["bid_micros"] // 1_000_000
                        if action == "hold" and not observation_due:
                            continue
                        target = held
                        if action == "enter":
                            target = min(
                                limits["capital_micros"] * proposal["allocation_bps"] // 10000,
                                limits["order_micros"],
                                limits["symbol_exposure_micros"],
                            )
                        elif action == "exit":
                            target = 0
                        # Recheck the signal using prices received during preceding control calls.
                        fresh_quotes = book.available_quotes(
                            symbols, now_ms(), min(5000, limits["max_quote_age_ms"])
                        )
                        fresh_quote = next((q for q in fresh_quotes if q["symbol"] == symbol), None)
                        if fresh_quote is None:
                            metrics["last_reasons"][symbol] = "reference_quote_unavailable"
                            continue
                        if action in ("enter", "exit"):
                            fresh_at = now_ms()
                            if action == "enter" and fresh_at >= signal_validity["expires_ms"]:
                                metrics["last_reasons"][symbol] = "signal_expired_before_submission"
                                continue
                            check = strategy.propose(
                                {
                                    **context,
                                    "now_ms": fresh_at,
                                    "quote": fresh_quote,
                                    "market": book.context(symbol, fresh_at),
                                    "news": fresh_news(list(active.values()), fresh_at),
                                }
                            )
                            if check != proposal:
                                metrics["last_reasons"][symbol] = (
                                    "proposal_changed_before_submission"
                                )
                                record_evidence(
                                    list(active),
                                    symbol,
                                    {
                                        "stage": "deferred",
                                        "action": "hold",
                                        "reason": "proposal_changed_before_submission",
                                    },
                                    fresh_at,
                                )
                                continue
                        envelope = event_envelope(
                            available_ms=now_ms(),
                            policy_revision=revision,
                            quote=fresh_quote,
                            portfolio_quotes=fresh_quotes,
                            target_micros=target,
                            strategy_fingerprint=strategy.fingerprint,
                            decision_provider=review["provider"],
                            signal=signal_validity if action == "enter" else None,
                            context={
                                "engine": "news_shock",
                                "action": action,
                                "reason": proposal["reason"],
                                "gate_reason": reason,
                                "observation_only": action == "hold",
                                "news_id": selected["news_id"] if selected else None,
                                "news_observed_ms": selected["observed_ms"] if selected else None,
                                "news_published_ms": selected["published_ms"] if selected else None,
                            },
                        )
                        entry_id = None
                        if action == "enter":
                            if not record_evidence(
                                [selected["news_id"]],
                                symbol,
                                {
                                    "stage": "submission",
                                    "action": action,
                                    "event_id": envelope["event_id"],
                                    "policy_revision": revision,
                                },
                                now_ms(),
                            ):
                                metrics["last_reasons"][symbol] = "evidence_unavailable"
                                continue
                            entry_id = candidate_entry_id
                            if not journal.claim_entry(entry_id, envelope):
                                continue
                        commit_start = time.perf_counter_ns()
                        ack = await transport.publish(envelope)
                        committed = await transport.drain()
                        collect_receipts(committed)
                        metrics["last_paper_commit_ms"] = (
                            time.perf_counter_ns() - commit_start
                        ) / 1_000_000
                        if selected:
                            metrics["last_observed_news_to_commit_ms"] = (
                                now_ms() - selected["observed_ms"]
                            )
                        state = await asyncio.to_thread(store.status, ACTOR)
                        if entry_id and state["paper"]["cursor"] >= ack.seq:
                            journal.entry_completed(entry_id)
                        if action == "exit":
                            filled = any(
                                item["receipt"]["outcome"] == "simulated_fill" for item in committed
                            )
                            exit_deferred_until[symbol] = now_ms() + (250 if filled else 60_000)
                    if observation_due:
                        next_observation = time.monotonic() + (
                            10 if any(state["paper"]["positions"].values()) else 60
                        )
                except asyncio.CancelledError:
                    raise
                except Exception as error:
                    state_name = "deferred"
                    metrics["last_defer_reason"] = type(error).__name__
                    # Feed faults must not produce a tight loop of durable control reads.
                    if started >= next_refresh:
                        next_refresh = time.monotonic() + 5
                if time.monotonic() >= next_heartbeat:
                    try:
                        await heartbeat(state_name)
                    except Exception:
                        pass
                    next_heartbeat = time.monotonic() + 10
                changed.clear()
                # Coalesce quote bursts without accumulating work or model calls.
                await sleep_or_stop(stopped, max(0.05, 0.25 - (time.monotonic() - started)))
    finally:
        stopped.set()
        for task in [*tasks, *reviews.values()]:
            task.cancel()
        await asyncio.gather(*tasks, *reviews.values(), return_exceptions=True)
        try:
            await heartbeat("stopped")
        finally:
            await connection.close()
            journal.close()
            if evidence is not None:
                evidence.close()
            if snapshots is not None:
                snapshots.close()
            lock.close()
