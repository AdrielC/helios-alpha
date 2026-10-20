"""Continuous private-strategy paper runtime: JetStream inputs, Golem authority."""

from __future__ import annotations

import argparse
import asyncio
import fcntl
import hashlib
import json
import os
import re
import signal
import time
from pathlib import Path

import httpx
import nats
from nats.errors import TimeoutError as NatsTimeout
from nats.js.api import AckPolicy, ConsumerConfig, DeliverPolicy

from helios_alpha.control.golem_store import GolemControlStore, read_private_json
from helios_alpha.control.store import DEFAULT_ROOT, canonical, private_root
from helios_alpha.signals.publisher import ensure_stream

from .decisions import review_event
from .market import HOUR, SYMBOLS, candles, quote
from .strategy import PrivateStrategy

STREAM = "HELIOS_CRYPTO_REFERENCE_V1"
SUBJECT = "helios.crypto.reference.v1.>"
ACTOR = "crypto-runtime"
MAX_SIGNAL_AGE_MS = 120_000


def validate_signal(signal: dict, available_ms: int) -> None:
    if (
        not isinstance(signal, dict)
        or set(signal) != {"input_digest", "available_ms", "expires_ms"}
        or not isinstance(signal["input_digest"], str)
        or re.fullmatch(r"[a-f0-9]{64}", signal["input_digest"]) is None
        or any(
            type(signal[key]) is not int or not 0 <= signal[key] <= 2**64 - 1
            for key in ("available_ms", "expires_ms")
        )
        or signal["available_ms"] > available_ms
        or not 0 < signal["expires_ms"] - signal["available_ms"] <= MAX_SIGNAL_AGE_MS
    ):
        raise ValueError("invalid signal validity")


def parse_event(data: bytes) -> dict:
    if len(data) > 65536:
        raise ValueError("event exceeds bound")
    value = json.loads(data)
    if not isinstance(value, dict) or value.get("schema_version") != 1:
        raise ValueError("unsupported market event")
    required = {
        "schema_version",
        "event_id",
        "available_ms",
        "policy_revision",
        "quote",
        "portfolio_quotes",
        "target_micros",
        "strategy_fingerprint",
        "decision_provider",
    }
    if not required <= set(value) or set(value) - required - {"context", "signal"}:
        raise ValueError("invalid market event shape")
    if "context" in value and (
        not isinstance(value["context"], dict) or len(canonical(value["context"])) > 8192
    ):
        raise ValueError("invalid market context")
    for field in ("available_ms", "policy_revision", "target_micros"):
        if type(value[field]) is not int or value[field] < 0:
            raise ValueError("invalid market event number")
    if value.get("signal") is not None:
        validate_signal(value["signal"], value["available_ms"])
    body = {k: v for k, v in value.items() if k != "event_id"}
    if hashlib.sha256(canonical(body).encode()).hexdigest() != value["event_id"]:
        raise ValueError("market event digest mismatch")
    return value


async def run(root: Path) -> None:
    os.umask(0o077)
    root = private_root(root)
    if read_private_json(root / "runtime.json").get("engine") == "news_shock":
        from .fast_runtime import run as run_fast

        await run_fast(root)
        return
    lock = (root / "crypto-runtime.lock").open("a+")
    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    config = read_private_json(root / "runtime.json")
    if config.get("schema_version") != 1 or config.get("mode") != "paper":
        raise ValueError("this runtime supports paper mode only")
    symbols = config["symbols"]
    if not symbols or len(symbols) != len(set(symbols)) or not set(symbols) <= SYMBOLS:
        raise ValueError("invalid runtime symbols")
    interval = config["poll_seconds"]
    if type(interval) is not int or not 30 <= interval <= 300:
        raise ValueError("invalid polling interval")
    strategy = PrivateStrategy(Path(config["strategy_path"]), Path(config["strategy_config_path"]))
    store = GolemControlStore(root)
    await asyncio.to_thread(store.command, ACTOR, "initialize_paper", config["paper"])
    initialized = await asyncio.to_thread(store.command, ACTOR, "enable_paper_signal_deadlines")
    if initialized.get("signal_deadline_version") != 1:
        raise ValueError("paper control component requires a signal-deadline update")
    connection_config = read_private_json(root / "nats-client.json")
    connection = await nats.connect(
        connection_config["url"],
        token=connection_config["token"],
        connect_timeout=3,
        max_reconnect_attempts=3,
    )
    js = connection.jetstream(timeout=5)
    await ensure_stream(js, STREAM, [SUBJECT])
    # nats-py 2.12 omits `created` from StreamInfo; use the documented management reply.
    reply = await connection.request(f"$JS.API.STREAM.INFO.{STREAM}", b"{}", timeout=5)
    created = json.loads(reply.data)["created"]
    if not isinstance(created, str) or not 10 <= len(created) <= 64:
        raise ValueError("invalid JetStream creation identity")
    source_id = STREAM + ":" + created
    subscription = await js.pull_subscribe(
        SUBJECT,
        durable="crypto-paper-v1",
        stream=STREAM,
        config=ConsumerConfig(
            durable_name="crypto-paper-v1",
            ack_policy=AckPolicy.EXPLICIT,
            deliver_policy=DeliverPolicy.ALL,
            ack_wait=90,
            max_ack_pending=1,
        ),
    )
    stopped = asyncio.Event()
    for sig in (signal.SIGINT, signal.SIGTERM):
        asyncio.get_running_loop().add_signal_handler(sig, stopped.set)
    cache: dict[str, tuple[int, list]] = {}

    async def heartbeat(status):
        await asyncio.to_thread(store.heartbeat, ACTOR, "crypto-shadow", status)

    async def drain():
        processed = 0
        while not stopped.is_set():
            try:
                messages = await subscription.fetch(batch=1, timeout=0.2)
            except NatsTimeout:
                return processed
            for message in messages:
                event = parse_event(message.data)
                state = await asyncio.to_thread(store.status, ACTOR)
                paper = state["paper"]
                if paper["source_id"] not in (None, source_id):
                    raise ValueError("JetStream identity changed; reconciliation required")
                sequence = message.metadata.sequence.stream
                if sequence <= paper["cursor"]:
                    await message.ack_sync(timeout=5)
                    continue
                policy = await asyncio.to_thread(store.policy, ACTOR)
                payload = dict(
                    source_id=source_id,
                    source_sequence=sequence,
                    expected_cursor=paper["cursor"],
                    expected_policy_revision=policy["revision"],
                    event_id=event["event_id"],
                    available_ms=event["available_ms"],
                    quote=event["quote"],
                    portfolio_quotes=event["portfolio_quotes"],
                    target_micros=min(
                        event["target_micros"], policy["limits"]["symbol_exposure_micros"]
                    ),
                    observation_only=event["policy_revision"] != policy["revision"],
                    signal=event.get("signal"),
                )
                receipt = await asyncio.to_thread(store.command, ACTOR, "paper_step", payload)
                # The source position and fill have now committed together in Golem.
                await message.ack_sync(timeout=5)
                processed += 1
                print(json.dumps({"event": "paper_input_committed", **receipt}), flush=True)

    try:
        async with httpx.AsyncClient(timeout=8, follow_redirects=False, trust_env=False) as client:
            while not stopped.is_set():
                cycle_start = time.monotonic()
                try:
                    await drain()  # reconcile redelivery before admitting a fresh observation
                    policy = await asyncio.to_thread(store.policy, ACTOR)
                    status = await asyncio.to_thread(store.status, ACTOR)
                    end = int(time.time()) // HOUR * HOUR
                    for symbol in symbols:
                        if cache.get(symbol, (None,))[0] != end:
                            cache[symbol] = (
                                end,
                                await candles(client, symbol, end - 240 * HOUR, end),
                            )
                    quotes = await asyncio.gather(*[quote(client, symbol) for symbol in symbols])
                    decisions = {}
                    signals = {}
                    for symbol in symbols:
                        decision_input = {
                            "symbol": symbol,
                            "bars": cache[symbol][1],
                            "now_ms": end * 1000,
                            "paper": status["paper"],
                        }
                        input_digest = hashlib.sha256(
                            canonical(decision_input).encode()
                        ).hexdigest()
                        available = time.time_ns() // 1_000_000
                        decisions[symbol] = strategy.decide(decision_input)
                        signals[symbol] = {
                            "input_digest": input_digest,
                            "available_ms": available,
                            "expires_ms": available + MAX_SIGNAL_AGE_MS,
                        }
                    # Event review runs before obtaining the execution reference quotes below.
                    reviews = {}
                    for symbol in symbols:
                        if decisions[symbol]["unusual_event"]:
                            reviews[symbol] = await review_event(
                                root,
                                store,
                                config["decisions"],
                                {
                                    "symbol": symbol,
                                    "bar_end_ms": end * 1000,
                                    "strategy_fingerprint": strategy.fingerprint,
                                    "recent_bars": cache[symbol][1][-3:],
                                },
                            )
                    if reviews:
                        quotes = await asyncio.gather(
                            *[quote(client, symbol) for symbol in symbols]
                        )
                    for q in quotes:
                        symbol = q["symbol"]
                        target = (
                            policy["limits"]["capital_micros"]
                            * decisions[symbol]["allocation_bps"]
                            // 10000
                        )
                        target = min(target, policy["limits"]["symbol_exposure_micros"])
                        review = reviews.get(symbol, {"provider": "local", "veto_new_risk": False})
                        if review["veto_new_risk"]:
                            held = (
                                status["paper"]["positions"].get(symbol, 0)
                                * q["bid_micros"]
                                // 1_000_000
                            )
                            target = min(target, held)
                        event = dict(
                            schema_version=1,
                            available_ms=time.time_ns() // 1_000_000,
                            policy_revision=policy["revision"],
                            quote=q,
                            portfolio_quotes=quotes,
                            target_micros=target,
                            strategy_fingerprint=strategy.fingerprint,
                            decision_provider=review["provider"],
                            signal=signals[symbol],
                        )
                        event["event_id"] = hashlib.sha256(canonical(event).encode()).hexdigest()
                        ack = await js.publish(
                            "helios.crypto.reference.v1." + symbol,
                            canonical(event).encode(),
                            headers={"Nats-Msg-Id": event["event_id"]},
                        )
                        if ack.stream != STREAM:
                            raise ValueError("unexpected stream acknowledgement")
                    count = await drain()
                    await heartbeat(
                        {
                            "state": "running",
                            "mode": "paper",
                            "reference_source": "coinbase",
                            "committed_this_cycle": count,
                            "strategy_fingerprint": strategy.fingerprint,
                            "decisions_enabled": config["decisions"]["enabled"],
                        }
                    )
                except asyncio.CancelledError:
                    raise
                except Exception as error:
                    # No raw source data, strategy exceptions, or credential values in logs.
                    print(
                        json.dumps({"event": "cycle_deferred", "error_type": type(error).__name__}),
                        flush=True,
                    )
                    try:
                        await heartbeat(
                            {
                                "state": "degraded",
                                "reason": type(error).__name__,
                                "new_submissions": "deferred",
                            }
                        )
                    except Exception:
                        pass
                delay = max(1, interval - (time.monotonic() - cycle_start))
                try:
                    await asyncio.wait_for(stopped.wait(), timeout=delay)
                except TimeoutError:
                    pass
    finally:
        try:
            await heartbeat({"state": "stopped", "mode": "paper"})
        finally:
            await connection.close()
            lock.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=DEFAULT_ROOT)
    args = parser.parse_args()
    asyncio.run(run(args.root))


if __name__ == "__main__":
    main()
