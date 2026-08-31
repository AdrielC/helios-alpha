# Messaging planes

Helios does not force market data, durable workflow events, and venue sessions through one bus.
They have different failure semantics. One transport everywhere would make at least one path worse.

## The decision

| Plane | Default | Why |
|---|---|---|
| Authoritative workflow state | Golem agents | Sequential account and hypothesis owners, durable invocation history, restart recovery |
| Durable service events | NATS JetStream | Pull consumers, acknowledgements, bounded batches, de-duplication IDs, replication, and operational tooling |
| High-rate edge telemetry | In-process Rust first, Zenoh when distribution is required | Efficient pub/sub and query semantics from sensors and edge networks to the data center |
| Local research compatibility | ZeroMQ | Small existing bridge for experiments, with no durability claim |
| Venue order protocol | FIX or broker-native API | Counterparty protocol, not an internal message bus |

The short answer is **NATS for the operational event plane**. Zenoh is an optional data-plane tool,
not the OMS journal. Golem is the source of truth for durable order and hypothesis state.

## Command and event direction

```text
operator / strategy / risk service
               │ typed command + idempotency key
               ▼
       Golem account OMS agent
               │ state and event commit
               ├────────► FIX sidecar or broker adapter ─────► venue
               │
               └────────► relay ─────► NATS JetStream
                                         │
                         ┌───────────────┼───────────────┐
                         ▼               ▼               ▼
                   operator read     positions       audit store
                      model          projector
```

Order commands do not enter NATS and hope that exactly one consumer acts. The caller invokes the
account owner with a stable command identity. The owner commits one versioned transition. A relay
then publishes that committed event with its `event_id` as `Nats-Msg-Id`. Consumers acknowledge
only after their own projection commit and still de-duplicate by event identity and aggregate
version.

This is at-least-once delivery with exactly-once domain effects. It does not pretend the network
offers a global exactly-once transaction.

`helio_relay::OmsEventRelay` implements that boundary. It validates the complete bounded batch
before publishing anything, rejects foreign accounts, schema changes, duplicate identities, cursor
gaps, and inconsistent batch cursors, then handles each event in this order:

1. Serialize the committed envelope.
2. Publish with the envelope identity as `Nats-Msg-Id`.
3. Await the JetStream publish acknowledgement.
4. Compare-and-set advance `ProjectionCursorAgent(account, projection)`.

If step 4 is interrupted, the next run starts from the old cursor and republishes the same identity.
JetStream returns a duplicate acknowledgement and the cursor can advance without producing a second
stored message. Commands never flow through this relay.

## NATS contract

`helio_oms::OmsEventEnvelope` defines the event boundary. Subjects are deterministic:

```text
helios.oms.v1.account.<encoded-account>.order.<encoded-order>.event
```

Account and order tokens are hex encoded before entering the subject, so an identifier cannot
inject `.`, `*`, or `>` semantics. The envelope contains:

- schema version;
- monotonic account event cursor;
- stable event identity;
- account and client order identity;
- aggregate version;
- commit time;
- canonical OMS event.

JetStream publishers must inspect every publish acknowledgement. Pull consumers should use bounded
fetches or continuous pull consumption, explicit acknowledgements, durable consumer names, and a
finite redelivery policy. A dead-letter stream records events that exceed that policy.

Primary references: [NATS concepts](https://docs.nats.io/learn/),
[JetStream pull consumers](https://docs.nats.io/learn/jetstream/pull-consumers), and
[advanced publishing and de-duplication](https://docs.nats.io/learn/jetstream/advanced-publishing).

The production relay verifies the existing stream's subjects, byte and message limits, maximum age,
de-duplication window, file storage, replica count, retention, discard policy, and delete/purge
guards before admitting work. Stream creation is disabled unless explicitly enabled for bootstrap.

## Where Zenoh fits

Zenoh becomes useful when the source is geographically distributed, intermittently connected, or
too hot to turn every observation into a workflow invocation. Examples include observatories,
weather stations, satellite ground feeds, and dense market telemetry.

Use Zenoh to move normalized observations or local aggregates into regional ingestion. Persist the
immutable source record before advancing a Helios source offset. Do not use an unpersisted Zenoh
sample as the only evidence for an order.

Primary references: [Zenoh overview](https://zenoh.io/docs/overview/what-is-zenoh/),
[transport reliability](https://zenoh.io/docs/manual/quic/), and
[access control](https://zenoh.io/docs/manual/access-control/).

## Migration from the existing bridge

`helios_signald` remains a useful local ZeroMQ subscriber. It is now classified as a research
compatibility adapter. It can feed a durable ingress writer, but it is not an operational queue,
order authority, replay log, or source of truth.

Move one boundary at a time:

1. Keep the current JSON signal schema for research clients.
2. Normalize source data into the immutable source protocol.
3. Invoke Golem by stable source interval or OMS command identity.
4. Publish committed service events through JetStream.
5. Add Zenoh only for a measured edge or telemetry need.
