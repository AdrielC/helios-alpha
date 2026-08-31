# OMS event relay

`helio_oms_relayd` moves already committed account events from Golem into NATS JetStream. It is a
projection relay, not an order bus. It cannot submit, cancel, replace, or authorize an order.

## Admission contract

The process starts only when all three boundaries are available and match configuration:

- `OmsAccountAgent(account)` returns bounded event pages after a monotonic cursor;
- `ProjectionCursorAgent(account, projection)` returns the same account and projection identities;
- the JetStream stream has the exact bounded durability policy requested by the process.

Production NATS credentials should be able to publish to `helios.oms.v1.>` and read stream metadata.
They should not be able to alter streams. Keep `HELIOS_NATS_ALLOW_STREAM_CREATE` unset after an
administrator provisions the stream.

## Delivery sequence

```text
Golem OMS event page
        │ validate every envelope, identity, and cursor
        ▼
JetStream publish with Nats-Msg-Id = event_id
        │ await server persistence acknowledgement
        ▼
Golem projection cursor compare-and-set advance
```

A crash before the acknowledgement leaves the cursor unchanged. A crash after acknowledgement but
before cursor advancement also leaves it unchanged, so the event is retried with the same identity.
JetStream's de-duplication window turns that retry into a duplicate acknowledgement. Consumers must
still commit their own projection and de-duplicate by event identity before acknowledging delivery.

## Configuration

| Variable | Required | Meaning |
|---|---|---|
| `HELIOS_ACCOUNT_ID` | yes | Durable OMS account identity |
| `HELIOS_RELAY_PROJECTION_ID` | no | Cursor identity, default `nats-oms-events-v1` |
| `HELIOS_GOLEM_MODE` | no | `local`, `cloud`, or `custom` |
| `HELIOS_GOLEM_APP` | no | Golem application, default `helios-alpha` |
| `HELIOS_GOLEM_ENVIRONMENT` | no | Golem environment |
| `GOLEM_TOKEN` | cloud/custom | Golem credential, never logged |
| `HELIOS_GOLEM_URL` | custom | Custom Golem endpoint |
| `NATS_URL` | no | NATS endpoint, default local port 4222 |
| `NATS_TOKEN` | production | NATS credential, never logged |
| `HELIOS_NATS_STREAM` | no | Stream name, default `HELIOS_OMS_V1` |
| `HELIOS_NATS_REPLICAS` | yes in production | Required replica count, normally 3 |
| `HELIOS_NATS_MAX_BYTES` | no | Hard stream byte limit |
| `HELIOS_NATS_MAX_MESSAGES` | no | Hard stream message limit |
| `HELIOS_NATS_MAX_AGE_SECONDS` | no | Event retention horizon |
| `HELIOS_NATS_DUPLICATE_WINDOW_SECONDS` | no | Server de-duplication horizon |
| `HELIOS_NATS_ALLOW_STREAM_CREATE` | bootstrap only | `1` permits creation, otherwise verify only |

Run a local development relay with one replica:

```bash
cd rust
HELIOS_ACCOUNT_ID=paper-account \
HELIOS_GOLEM_MODE=local \
NATS_URL=nats://127.0.0.1:4222 \
HELIOS_NATS_REPLICAS=1 \
HELIOS_NATS_ALLOW_STREAM_CREATE=1 \
cargo run -p helio_relay --features native --bin helio_oms_relayd
```

The process retries transient port failures without moving the cursor. Structural faults such as a
cursor gap, foreign account, unsupported schema, or mismatched stream policy terminate the process
so an operator must investigate.

## Proof

The deterministic suite injects publisher and cursor failures around the acknowledgement boundary.
The live smoke starts a pinned NATS server, provisions the bounded stream, publishes one event twice,
and verifies the second acknowledgement is marked duplicate with the original stream sequence. The
Golem smoke independently proves cursor replay, simulated agent crash, and full server restart.

```bash
cd rust
cargo test -p helio_relay --no-default-features
cargo check -p helio_relay --features native
NATS_SERVER_BIN=/path/to/verified/nats-server \
  bash crates/helio_relay/tests/nats_server_smoke.sh
```

This is implementation evidence, not production-cluster evidence. Before capital, retain a drill
artifact from the deployed replica topology, credentials, retention policy, publisher outage,
consumer redelivery, alerting, and rollback.
