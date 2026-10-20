"""Acknowledged event transport into the existing durable paper account."""

from __future__ import annotations

import asyncio
import hashlib
import json
from contextlib import asynccontextmanager
from dataclasses import replace

from nats.errors import TimeoutError as NatsTimeout
from nats.js.api import AckPolicy, ConsumerConfig, DeliverPolicy
from nats.js.errors import NotFoundError

from helios_alpha.control.store import canonical
from helios_alpha.signals.publisher import ensure_stream

from .runtime import ACTOR, STREAM, SUBJECT, parse_event

ACK_WAIT_SECONDS = 5
LEASE_REFRESH_SECONDS = 1
REDELIVERY_DELAY_SECONDS = 1


async def renew_delivery(message):
    """Keep slow durable calls leased; a crashed process releases within AckWait."""
    while True:
        await asyncio.sleep(LEASE_REFRESH_SECONDS)
        try:
            await asyncio.wait_for(message.in_progress(), timeout=1)
        except Exception:
            # A disconnected broker can expire the lease. The durable cursor,
            # not these best-effort progress messages, prevents repeated effects.
            pass


@asynccontextmanager
async def delivery_lease(message):
    renewal = asyncio.create_task(renew_delivery(message))
    try:
        yield
    except BaseException:
        renewal.cancel()
        await asyncio.gather(renewal, return_exceptions=True)
        try:
            await asyncio.wait_for(message.nak(delay=REDELIVERY_DELAY_SECONDS), timeout=1)
        except (Exception, asyncio.CancelledError):
            # A lost NAK still expires after the short lease. Never ACK an
            # unknown Golem result or replace the original processing error.
            pass
        raise
    finally:
        renewal.cancel()
        await asyncio.gather(renewal, return_exceptions=True)


def event_envelope(**fields) -> dict:
    body = {"schema_version": 1, **fields}
    body["event_id"] = hashlib.sha256(canonical(body).encode()).hexdigest()
    parse_event(canonical(body).encode())
    return body


class PaperTransport:
    @classmethod
    async def connect(
        cls,
        connection,
        store,
        *,
        stream=STREAM,
        subject=SUBJECT,
        durable="crypto-paper-v1",
        max_bytes=128 * 1024 * 1024,
    ):
        self = cls()
        self.store = store
        self.js = connection.jetstream(timeout=5)
        await ensure_stream(self.js, stream, [subject], max_bytes=max_bytes)
        response = await connection.request(f"$JS.API.STREAM.INFO.{stream}", b"{}", timeout=5)
        created = json.loads(response.data)["created"]
        if not isinstance(created, str) or not 10 <= len(created) <= 64:
            raise ValueError("invalid stream identity")
        self.stream = stream
        self.subject_prefix = subject.removesuffix(">")
        self.source_id = stream + ":" + created
        try:
            existing = await self.js.consumer_info(stream, durable)
        except NotFoundError:
            pass
        else:
            config = existing.config
            if (
                config.ack_policy != AckPolicy.EXPLICIT
                or config.deliver_policy != DeliverPolicy.ALL
                or config.max_ack_pending != 1
                or config.filter_subject != subject
                or config.filter_subjects
                or config.deliver_subject
                or config.backoff
            ):
                raise ValueError("paper consumer configuration requires reconciliation")
            if config.ack_wait != ACK_WAIT_SECONDS:
                # pull_subscribe only binds existing consumers; its supplied
                # config does not update them. Change the mutable lease in place,
                # retaining the consumer identity, cursor, and pending delivery.
                await self.js.add_consumer(
                    stream, config=replace(config, ack_wait=ACK_WAIT_SECONDS)
                )
        self.subscription = await self.js.pull_subscribe(
            subject,
            stream=stream,
            durable=durable,
            config=ConsumerConfig(
                durable_name=durable,
                ack_policy=AckPolicy.EXPLICIT,
                deliver_policy=DeliverPolicy.ALL,
                ack_wait=ACK_WAIT_SECONDS,
                max_ack_pending=1,
            ),
        )
        return self

    async def publish(self, event: dict):
        ack = await self.js.publish(
            self.subject_prefix + event["quote"]["symbol"],
            canonical(event).encode(),
            headers={"Nats-Msg-Id": event["event_id"]},
        )
        if ack.stream != self.stream:
            raise ValueError("unexpected stream acknowledgement")
        return ack

    async def drain(self, max_batch=200) -> list[dict]:
        results = []
        for _ in range(max_batch):
            try:
                messages = await self.subscription.fetch(1, timeout=0.03)
            except NatsTimeout:
                return results
            message = messages[0]
            async with delivery_lease(message):
                event = parse_event(message.data)
                state = await asyncio.to_thread(self.store.status, ACTOR)
                paper = state["paper"]
                if paper["source_id"] not in (None, self.source_id):
                    raise ValueError("stream identity changed; reconciliation required")
                sequence = message.metadata.sequence.stream
                if sequence <= paper["cursor"]:
                    await message.ack_sync(timeout=5)
                    continue
                policy = await asyncio.to_thread(self.store.policy, ACTOR)
                receipt = await asyncio.to_thread(
                    self.store.command,
                    ACTOR,
                    "paper_step",
                    dict(
                        source_id=self.source_id,
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
                        observation_only=(
                            event["policy_revision"] != policy["revision"]
                            or event.get("context", {}).get("observation_only", False)
                        ),
                        signal=event.get("signal"),
                    ),
                )
                await message.ack_sync(timeout=5)
                results.append({"receipt": receipt, "event": event})
        raise ValueError("paper backlog exceeds fast-path bound")
