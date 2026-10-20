"""Local JetStream restart and lease test; never connects to an account or shared broker."""

import asyncio
import json
import re
import shutil
import threading
import time
from types import SimpleNamespace

import pytest

pytest.importorskip("nats")

import nats
from nats.errors import TimeoutError as NatsTimeout
from nats.js.api import ConsumerConfig

from helios_alpha.control.store import canonical
from helios_alpha.crypto.paper_transport import ACK_WAIT_SECONDS, PaperTransport, event_envelope
from helios_alpha.signals.publisher import ensure_stream


@pytest.mark.integration
def test_abandoned_delivery_and_slow_commit_preserve_consumer(tmp_path):
    binary = shutil.which("nats-server")
    if binary is None:
        pytest.skip("local nats-server is not installed")

    async def check():
        server = await asyncio.create_subprocess_exec(
            binary, "--addr", "127.0.0.1", "--port", "-1",
            "--jetstream", "--store_dir", str(tmp_path / "jetstream"),
            stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.STDOUT,
        )
        connections = []
        task = None
        finish = threading.Event()
        try:
            async with asyncio.timeout(5):
                while True:
                    line = await server.stdout.readline()
                    assert line, "local test broker exited before listening"
                    match = re.search(rb"Listening for client connections on 127.0.0.1:(\d+)", line)
                    if match:
                        url = "nats://127.0.0.1:" + match[1].decode()
                        break
            old = await nats.connect(url)
            connections.append(old)
            js = old.jetstream()
            stream, subject, durable = "PAPER_TEST", "paper.test.>", "restart-test"

            async def created(connection):
                response = await connection.request(
                    f"$JS.API.CONSUMER.INFO.{stream}.{durable}", b"{}", timeout=1
                )
                return json.loads(response.data)["created"]

            await ensure_stream(js, stream, [subject], max_bytes=1024 * 1024)
            legacy = await js.pull_subscribe(
                subject, stream=stream, durable=durable,
                config=ConsumerConfig(
                    durable_name=durable, ack_wait=90, max_ack_pending=1,
                ),
            )
            for i in range(2):
                stamp = 1_800_000_000_000 + i
                event = event_envelope(
                    available_ms=stamp, policy_revision=1,
                    quote={
                        "symbol": "BTC-USD", "bid_micros": 100_000_000,
                        "ask_micros": 100_010_000, "observed_ms": stamp,
                    },
                    portfolio_quotes=[], target_micros=10_000_000,
                    strategy_fingerprint="synthetic", decision_provider="test",
                    context={"observation_only": False},
                )
                await js.publish("paper.test.BTC-USD", canonical(event).encode())
            first = (await legacy.fetch(1, timeout=1))[0]
            await first.ack_sync()
            # The server has delivered sequence 2, but the process loses it
            # before any application handler can commit or ACK it.
            await legacy.fetch(1, timeout=1)
            before = await js.consumer_info(stream, durable)
            created_before = await created(old)
            assert before.ack_floor.stream_seq == 1
            assert before.delivered.stream_seq == 2 and before.num_ack_pending == 1
            await old.close()

            current = await nats.connect(url)
            connections.append(current)
            state = {"paper": {"cursor": 1, "source_id": None}}
            started = threading.Event()
            effects = []

            def command(_actor, method, payload):
                assert method == "paper_step"
                started.set()
                assert finish.wait(15), "slow-commit test did not release its store"
                assert payload["source_sequence"] == 2 and payload["expected_cursor"] == 1
                effects.append(payload)
                state["paper"].update(cursor=2, source_id=payload["source_id"])
                return {"outcome": "simulated_fill", "cursor": 2}

            store = SimpleNamespace(
                status=lambda _: state,
                policy=lambda _: {
                    "revision": 1, "limits": {"symbol_exposure_micros": 100_000_000},
                },
                command=command,
            )
            restart_at = time.monotonic()
            transport = await PaperTransport.connect(
                current, store, stream=stream, subject=subject,
                durable=durable, max_bytes=1024 * 1024,
            )
            migrated = await transport.js.consumer_info(stream, durable)
            assert await created(current) == created_before
            assert migrated.name == before.name
            assert migrated.ack_floor == before.ack_floor
            assert migrated.delivered == before.delivered
            assert migrated.num_ack_pending == 1
            assert migrated.config.ack_wait == ACK_WAIT_SECONDS

            async def drain_until_commit():
                while not effects:
                    receipts = await transport.drain()
                    if receipts:
                        return receipts
                    await asyncio.sleep(0.05)

            task = asyncio.create_task(drain_until_commit())
            assert await asyncio.to_thread(started.wait, ACK_WAIT_SECONDS + 3)
            assert time.monotonic() - restart_at < ACK_WAIT_SECONDS + 3
            during = await transport.js.consumer_info(stream, durable)
            assert during.num_redelivered == 1
            # Keep the durable call blocked longer than the lease. Progress ACKs
            # must prevent another pull from receiving the same work.
            await asyncio.sleep(ACK_WAIT_SECONDS + 0.2)
            with pytest.raises(NatsTimeout):
                await transport.subscription.fetch(1, timeout=0.05)
            finish.set()
            assert len(await asyncio.wait_for(task, 2)) == 1
            after = await transport.js.consumer_info(stream, durable)
            assert await created(current) == created_before
            assert after.ack_floor.stream_seq == 2 and after.num_ack_pending == 0
            assert len(effects) == 1
        finally:
            finish.set()
            if task is not None:
                task.cancel()
                await asyncio.gather(task, return_exceptions=True)
            for connection in connections:
                await connection.close()
            server.terminate()
            await asyncio.wait_for(server.wait(), 5)

    asyncio.run(check())
