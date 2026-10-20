# helios_signald

NATS JetStream diagnostic consumer: receives `helios.signal.>` JSON from Python `SignalPublisher`.

## Build

**Runtime:** a local NATS server with JetStream enabled.

```bash
cd rust
cargo build --release -p helios_signald
# target/release/helios_signald nats://127.0.0.1:4222
```

Await `SignalPublisher.connect()` first to provision the `HELIOS_SIGNALS_V1` stream.
Await `publish()` for the server acknowledgement. Both clients connect to NATS;
set `HELIOS_NATS_TOKEN` for the Rust consumer when authentication is enabled.

## Wire format

Each message contains one JSON envelope. Subject example: `helios.signal.ssi.warning`.
`Nats-Msg-Id` carries the immutable signal ID. The durable `signal-diagnostics-v1`
consumer acknowledges after flushing stdout; a crash can repeat diagnostic output.
It does not place orders.
