#!/usr/bin/env python3
"""Verify local Golem/JetStream recovery using a separate simulated account."""

from __future__ import annotations

import asyncio
import json
import os
import subprocess
import time
import uuid

import nats
from nats.js.api import AckPolicy, ConsumerConfig

from helios_alpha.control.golem_store import GolemControlStore, read_private_json
from helios_alpha.control.local import request
from helios_alpha.control.models import PolicyUpdate
from helios_alpha.control.store import DEFAULT_ROOT
from helios_alpha.signals.publisher import ensure_stream


async def verify():
    os.umask(0o077)
    root = DEFAULT_ROOT
    initial = request(root, "status")
    request(root, "stop")
    proof_id = "proof-" + uuid.uuid4().hex
    main_store = GolemControlStore(root)
    main_before = main_store.settings("observer")
    store = GolemControlStore(root)
    store.account = proof_id
    store.phantom_id = None
    bootstrap = read_private_json(root / "control-bootstrap.json")
    bootstrap["limits"] = main_before["policy"]["limits"]
    bootstrap["clients"]["proof"] = [
        "status:read",
        "risk:read",
        "risk:write",
        "audit:read",
        "execution:pause",
        "runtime:write",
        "decisions:reserve",
    ]
    store.initialize(bootstrap)
    paper_config = read_private_json(root / "runtime.json")["paper"]
    store.command("proof", "initialize_paper", paper_config)
    store.command("proof", "enable_paper_signal_deadlines")
    policy = store.policy("proof")
    store.update_policy(
        "proof",
        PolicyUpdate(
            expected_revision=policy["revision"],
            request_id="proof-revision",
            limits=policy["limits"],
            reason="isolated recovery verification",
        ),
    )
    assert store.command("proof", "reserve_decision", {"request_id": "proof-call"})["send_allowed"]
    config = read_private_json(root / "nats-client.json")
    stream_name = "HELIOS_PROOF_" + uuid.uuid4().hex[:12].upper()
    subject = "helios.proof." + proof_id
    connection = None
    try:
        connection = await nats.connect(config["url"], token=config["token"])
        js = connection.jetstream(timeout=5)
        await ensure_stream(js, stream_name, [subject], max_bytes=1024 * 1024)
        payload = {"id": proof_id, "simulation": True}
        encoded = json.dumps(payload).encode()
        first = await js.publish(subject, encoded, headers={"Nats-Msg-Id": proof_id})
        duplicate = await js.publish(subject, encoded, headers={"Nats-Msg-Id": proof_id})
        assert duplicate.duplicate and duplicate.seq == first.seq
        consumer = await js.pull_subscribe(
            subject,
            durable="proof",
            stream=stream_name,
            config=ConsumerConfig(
                durable_name="proof", ack_policy=AckPolicy.EXPLICIT, max_ack_pending=1, ack_wait=1
            ),
        )
        message = (await consumer.fetch(1, timeout=5))[0]
        now_ms = time.time_ns() // 1_000_000
        step = dict(
            source_id=stream_name,
            source_sequence=message.metadata.sequence.stream,
            expected_cursor=0,
            expected_policy_revision=2,
            event_id=proof_id,
            available_ms=now_ms,
            quote=dict(
                symbol="BTC-USD", bid_micros=100_000_000, ask_micros=100_010_000, observed_ms=now_ms
            ),
            portfolio_quotes=[],
            target_micros=30_000_000,
            observation_only=False,
            signal={
                "input_digest": "a" * 64,
                "available_ms": now_ms,
                "expires_ms": now_ms + 120_000,
            },
        )
        receipt = store.command("proof", "paper_step", step)
        assert receipt["outcome"] == "simulated_fill", receipt
        before = store.status("proof")
        # Deliberately leave the message unacknowledged after the durable effect.
        command = [
            store.config["executable"],
            "--disable-app-manifest-discovery",
            "--profile",
            "local",
            "--config-dir",
            str(root / "golem-cli"),
            "agent",
            "simulate-crash",
            store.agent_address,
        ]
        subprocess.run(command, capture_output=True, check=True, timeout=20)
        assert store.status("proof")["paper"] == before["paper"]
        assert store.command("proof", "paper_step", step) == receipt
        await connection.close()
        connection = None
        request(root, "down")
        for _ in range(250):
            if not (root / "supervisor.sock").exists():
                break
            await asyncio.sleep(0.1)
        else:
            raise RuntimeError("supervisor did not stop")
        subprocess.run(
            [os.sys.executable, "-m", "helios_alpha.control.local", "up"],
            capture_output=True,
            check=True,
            timeout=20,
        )
        for _ in range(60):
            try:
                after = store.status("proof")
                break
            except Exception:
                await asyncio.sleep(0.5)
        else:
            raise RuntimeError("Golem did not recover")
        assert after["paper"] == before["paper"]
        assert after["policy_revision"] == 2
        assert after["decision_budget"]["reserved_calls"] == 1
        assert main_store.settings("observer") == main_before
        assert not store.command("proof", "reserve_decision", {"request_id": "proof-call"})[
            "send_allowed"
        ]
        connection = await nats.connect(config["url"], token=config["token"])
        js = connection.jetstream(timeout=5)
        consumer = await js.pull_subscribe_bind("proof", stream=stream_name)
        redelivery = (await consumer.fetch(1, timeout=5))[0]
        assert redelivery.metadata.sequence.stream == first.seq
        assert redelivery.metadata.num_delivered >= 2
        assert store.command("proof", "paper_step", step) == receipt
        await redelivery.ack_sync(timeout=5)
        proof = dict(
            schema_version=1,
            verified_ms=time.time_ns() // 1_000_000,
            simulation_only=True,
            account=proof_id,
            publish_deduplicated=True,
            golem_crash_survived=True,
            full_service_restart_survived=True,
            unacknowledged_message_redelivered=True,
            duplicate_fill_suppressed=True,
            policy_and_decision_budget_preserved=True,
            main_settings_preserved=True,
            simulated_fill_count=1,
            live_orders=0,
            paid_api_calls=0,
        )
        (root / "recovery-proof.json").write_text(json.dumps(proof, indent=2) + "\n")
        print(json.dumps(proof, indent=2))
    finally:
        if connection is not None:
            await connection.close()
        if initial["runtime_requested"]:
            request(root, "run")


asyncio.run(verify())
