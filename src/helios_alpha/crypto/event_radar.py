"""Continuous public event observations; durable research signals, no order adapter."""

from __future__ import annotations

import argparse
import asyncio
import fcntl
import json
import os
import signal
import sqlite3
import time
from pathlib import Path

import httpx

from helios_alpha.control.golem_store import GolemControlStore, read_private_json
from helios_alpha.control.store import DEFAULT_ROOT, ControlError, canonical, private_root
from helios_alpha.shadow.operator_projection import write_operator_projection
from helios_alpha.signals.publisher import ensure_stream

from .event_journal import EventJournal, validate_config
from .event_sources import discover, observe

STREAM = "HELIOS_EVENT_SIGNALS_V1"
SUBJECT = "helios.events.signals.v1"
# Use the existing Golem research slot; shadow and execution own separate slots.
COMPONENT = "crypto-research"
DEFAULT_CONFIG = Path(__file__).resolve().parents[3] / "config/events/radar-v1.json"


def clock():
    return time.time_ns() // 1_000_000


def recent_news(root):
    path = root / "news-intake.sqlite3"
    if not path.is_file() or path.is_symlink():
        return []
    with sqlite3.connect(path.as_uri() + "?mode=ro", uri=True) as db:
        return [
            {
                key: item.get(key)
                for key in (
                    "news_id",
                    "source",
                    "title",
                    "url",
                    "published_ms",
                    "observed_ms",
                    "eligible",
                    "intake_reason",
                )
            }
            for (wire,) in db.execute("SELECT data FROM news ORDER BY observed_ms DESC LIMIT 12")
            for item in [json.loads(wire)]
        ]


def read_event_radar(store, client):
    if "status:read" not in store.capabilities(client):
        raise ControlError("event inspection permission required")
    path = store.root / "event-radar-status.json"
    if not path.exists():
        return {"schema_version": 1, "online": False, "execution_authority": False}
    data = read_private_json(path)
    if data.get("schema_version") != 1 or data.get("execution_authority") is not False:
        raise ControlError("invalid event observation")
    return {**data, "online": 0 <= clock() - data["observed_ms"] <= 15000}


async def run(root, *, prepare=False, once=False):
    os.umask(0o077)
    root = private_root(root)
    lock = (root / "event-radar.lock").open("a+")
    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    config_path = root / "event-radar-config.json"
    if not config_path.exists():
        write_operator_projection(
            config_path, validate_config(json.loads(DEFAULT_CONFIG.read_text()))
        )
    config = validate_config(read_private_json(config_path))
    journal = EventJournal(root / "event-radar.sqlite3")
    stopped = asyncio.Event()
    health = {
        provider: {"state": "starting", "last_ok_ms": None, "markets": 0, "quotes": 0}
        for provider in ("polymarket", "kalshi")
    }
    transport = {"state": "starting", "subject": SUBJECT}
    authority = {"state": "starting"}
    command = None
    tasks, connection = [], None

    def snapshot():
        result = journal.snapshot(clock(), config)
        try:
            result["news"] = recent_news(root)
        except (sqlite3.Error, ValueError, OSError):
            result["news"] = []
        result.update(
            sources=[{"id": key, **value} for key, value in health.items()],
            transport=transport,
            golem=authority,
            last_command=command,
        )
        write_operator_projection(root / "event-radar-status.json", result)
        return result

    async def wait(seconds):
        try:
            await asyncio.wait_for(stopped.wait(), timeout=seconds)
        except TimeoutError:
            pass

    async def provider_loop(client, provider):
        due = 0
        while not stopped.is_set():
            started = time.monotonic()
            try:
                if started >= due:
                    items = await discover(client, provider, config, clock())
                    journal.register(provider, items, clock())
                    due = time.monotonic() + config["discovery_seconds"]
                items = journal.watched(provider)
                sem = asyncio.Semaphore(3)

                async def read(item):
                    async with sem:
                        try:
                            data = await observe(client, item)
                            journal.record(item, data, config)
                            return True
                        except (httpx.HTTPError, ValueError, KeyError, TypeError):
                            return False

                results = await asyncio.gather(*(read(item) for item in items))
                successes = sum(results)
                health[provider] = {
                    "state": "receiving" if successes else "no_quotes",
                    "last_ok_ms": clock() if successes else health[provider].get("last_ok_ms"),
                    "markets": len(items),
                    "quotes": successes,
                }
            except Exception as error:
                health[provider] = {
                    **health[provider],
                    "state": "degraded",
                    "error": type(error).__name__,
                }
            if once:
                return
            await wait(max(1, config["poll_seconds"] - (time.monotonic() - started)))

    async def publish_loop():
        import nats

        nonlocal connection
        while not stopped.is_set():
            try:
                if connection is None or connection.is_closed:
                    nc = read_private_json(root / "nats-client.json")
                    connection = await nats.connect(
                        nc["url"], token=nc["token"], connect_timeout=3, max_reconnect_attempts=3
                    )
                    await ensure_stream(
                        connection.jetstream(timeout=5),
                        STREAM,
                        [SUBJECT],
                        max_bytes=32 * 1024 * 1024,
                    )
                for event in journal.pending():
                    ack = await connection.jetstream(timeout=5).publish(
                        SUBJECT, canonical(event).encode(), headers={"Nats-Msg-Id": event["id"]}
                    )
                    if ack.stream != STREAM:
                        raise ValueError("unexpected event acknowledgement")
                    journal.delivered(event["id"], ack.seq)
                transport.update(state="ready")
                transport.pop("error", None)
            except Exception as error:
                transport.update(state="degraded", error=type(error).__name__)
                if connection is not None:
                    await connection.close()
                connection = None
            await wait(2 if transport["state"] == "ready" else 15)

    async def heartbeat_loop():
        from .runtime import ACTOR

        while not stopped.is_set():
            try:
                store = GolemControlStore(root)
                await asyncio.to_thread(
                    store.heartbeat,
                    ACTOR,
                    COMPONENT,
                    {
                        "state": "running",
                        "mode": "research",
                        "engine": "event_radar",
                        "reference_source": "prediction_markets",
                        "reference_channel": "public_rest",
                        "feed_connected": any(
                            source["state"] == "receiving" for source in health.values()
                        ),
                        "active_news": 0,
                        "last_reasons": {},
                        "registered_markets": len(journal.watched()),
                        "signal_outbox": transport["state"],
                        "execution_authority": False,
                    },
                )
                authority.update(state="recorded", last_ok_ms=clock())
                authority.pop("error", None)
            except Exception as error:
                authority.update(state="degraded", error=type(error).__name__)
            await wait(10)

    async def status_loop():
        nonlocal config, command
        next_trim = 0
        while not stopped.is_set():
            request_path = root / "event-radar-request.json"
            if request_path.exists():
                request = {}
                try:
                    request = read_private_json(request_path)
                    if (
                        type(request.get("enabled")) is not bool
                        or request.get("expected_revision") != config["revision"]
                        or not 0 <= clock() - request.get("requested_ms", 0) <= 30000
                    ):
                        raise ValueError("event settings changed or request expired")
                    candidate = validate_config(
                        {
                            **config,
                            "revision": config["revision"] + 1,
                            "enabled": request["enabled"],
                        }
                    )
                    write_operator_projection(config_path, candidate)
                    config = candidate
                    command = {"id": request["id"], "state": "applied"}
                except Exception:
                    command = {"id": request.get("id", ""), "state": "rejected"}
                request_path.unlink(missing_ok=True)
            if time.monotonic() >= next_trim:
                journal.trim(clock(), config["retention_days"])
                next_trim = time.monotonic() + 3600
            snapshot()
            await wait(2)

    try:
        snapshot()
        if prepare:
            return
        for sig in (signal.SIGTERM, signal.SIGINT):
            asyncio.get_running_loop().add_signal_handler(sig, stopped.set)
        async with httpx.AsyncClient(
            timeout=8,
            follow_redirects=False,
            trust_env=False,
            headers={"User-Agent": "HeliosAlphaResearch/0.1"},
        ) as client:
            tasks = [asyncio.create_task(provider_loop(client, provider)) for provider in health]
            if once:
                await asyncio.gather(*tasks)
                snapshot()
                return
            tasks += [
                asyncio.create_task(publish_loop()),
                asyncio.create_task(heartbeat_loop()),
                asyncio.create_task(status_loop()),
            ]
            stop_task = asyncio.create_task(stopped.wait())
            tasks.append(stop_task)
            done, _ = await asyncio.wait(tasks, return_when=asyncio.FIRST_COMPLETED)
            # A failed worker must stop the process so its supervisor can recover it.
            for task in done:
                task.result()
    finally:
        for task in tasks:
            task.cancel()
        await asyncio.gather(*tasks, return_exceptions=True)
        if connection is not None:
            await connection.close()
        journal.close()
        lock.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=DEFAULT_ROOT)
    parser.add_argument("--prepare", action="store_true")
    parser.add_argument("--once", action="store_true")
    args = parser.parse_args()
    asyncio.run(run(args.root, prepare=args.prepare, once=args.once))


if __name__ == "__main__":
    main()
