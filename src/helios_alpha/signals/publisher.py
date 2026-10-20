from __future__ import annotations

import re

from helios_alpha.signals.schema import HeliosSignalV1

SIGNAL_STREAM = "HELIOS_SIGNALS_V1"


async def ensure_stream(js, name: str, subjects: list[str], *, max_bytes=128 * 1024 * 1024):
    from nats.js.api import DiscardPolicy, RetentionPolicy, StorageType, StreamConfig
    from nats.js.errors import NotFoundError

    expected = StreamConfig(
        name=name,
        subjects=subjects,
        retention=RetentionPolicy.LIMITS,
        storage=StorageType.FILE,
        num_replicas=1,
        max_msgs=200_000,
        max_bytes=max_bytes,
        max_age=14 * 86400,
        max_msg_size=65536,
        discard=DiscardPolicy.NEW,
        duplicate_window=600,
        deny_delete=True,
        deny_purge=True,
    )
    try:
        info = await js.stream_info(name)
    except NotFoundError:
        info = await js.add_stream(config=expected)
    for field in (
        "subjects",
        "retention",
        "storage",
        "num_replicas",
        "max_msgs",
        "max_bytes",
        "max_age",
        "max_msg_size",
        "discard",
        "duplicate_window",
        "deny_delete",
        "deny_purge",
        "no_ack",
    ):
        if getattr(info.config, field) != getattr(expected, field):
            raise ValueError("JetStream configuration does not match durability bounds")
    return info


class SignalPublisher:
    """Acknowledged JetStream publisher. Await connect/publish/close.

    Retries must reuse the same immutable envelope (including ID and timestamp).
    Broker-side deduplication is bounded; consumers must persist their own cursor.
    A timeout has an unknown publish outcome, never a success.
    """

    def __init__(self, connection, js):
        self._connection = connection
        self._js = js

    @classmethod
    async def connect(cls, url="nats://127.0.0.1:4222", *, token: str | None = None):
        import nats

        connection = await nats.connect(
            url, token=token, connect_timeout=3, max_reconnect_attempts=3
        )
        try:
            js = connection.jetstream(timeout=5)
            await ensure_stream(js, SIGNAL_STREAM, ["helios.signal.>"])
            return cls(connection, js)
        except BaseException:
            await connection.close()
            raise

    async def publish(self, signal: HeliosSignalV1) -> dict:
        if not re.fullmatch(r"[A-Za-z0-9_-]+(?:\.[A-Za-z0-9_-]+)*", signal.topic_suffix):
            raise ValueError("invalid signal subject")
        if not re.fullmatch(r"[A-Za-z0-9_-]{1,160}", signal.signal_id):
            raise ValueError("invalid signal identity")
        if not signal.emitted_at_utc:
            raise ValueError("signal needs an immutable emission timestamp before publishing")
        topic = f"helios.signal.{signal.topic_suffix}.{signal.kind.value}"
        ack = await self._js.publish(
            topic,
            signal.to_json_bytes(),
            headers={"Nats-Msg-Id": signal.signal_id},
        )
        if ack.stream != SIGNAL_STREAM:
            raise ValueError("unexpected JetStream acknowledgement")
        return {"stream": ack.stream, "sequence": ack.seq, "duplicate": ack.duplicate}

    async def close(self) -> None:
        await self._connection.drain()

    async def __aenter__(self):
        return self

    async def __aexit__(self, *args):
        await self.close()
