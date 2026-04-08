from __future__ import annotations

from helios_alpha.signals.schema import HeliosSignalV1
from helios_alpha.timekeeping import Clock, SystemClock


class SignalPublisher:
    """
    ZMQ PUB socket. Install: ``pip install helios-alpha[execution]``.

    Multipart message: ``[topic, json_utf8]`` so Rust/Python SUB can prefix-match.
    """

    def __init__(
        self,
        bind_address: str = "tcp://127.0.0.1:7779",
        *,
        topic_prefix: str = "helios.signal",
        clock: Clock | None = None,
    ) -> None:
        try:
            import zmq
        except ImportError as e:
            msg = "Install pyzmq: pip install helios-alpha[execution]"
            raise ImportError(msg) from e
        self._ctx = zmq.Context.instance()
        self._sock = self._ctx.socket(zmq.PUB)
        self._sock.bind(bind_address)
        self._topic_prefix = topic_prefix.rstrip(".")
        self._zmq = zmq
        self._clock = clock or SystemClock()

    @property
    def topic_prefix(self) -> str:
        return self._topic_prefix

    def publish(self, signal: HeliosSignalV1) -> None:
        ts = self._clock.now_utc().to_iso8601_string()
        payload = signal.model_dump()
        payload["emitted_at_utc"] = ts
        topic = f"{self._topic_prefix}.{signal.topic_suffix}.{signal.kind.value}"
        body = HeliosSignalV1(**payload).to_json_bytes()
        self._sock.send_multipart([topic.encode("utf-8"), body])

    def close(self) -> None:
        self._sock.close(linger=0)

    def __enter__(self) -> SignalPublisher:
        return self

    def __exit__(self, *args: object) -> None:
        self.close()
