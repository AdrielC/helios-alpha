# Production readiness

Helios proves deterministic stream mechanics. It does not yet authorize, route, or reconcile live
orders. Treat the table below as an evidence ledger, not a maturity label.

## Evidence in the repository

| Area | Current state |
|---|---|
| Scan composition | Static, typed, allocation-aware hot path |
| Conditional inference | Keyed, bounded, atomic hypothesis runtime with deterministic deadlines and validated restore |
| Service ownership | Single-owner engine or bounded Tokio actor; explicit mutex adapter for integration |
| Event ordering | Capacity-bounded with typed late and overflow outcomes |
| Online statistics | Stable moments, compensated sums, scaled norms, log probabilities, validated snapshots |
| Streaming forecasts | Atomic guarded Kalman adapter with explicit numerical rejection |
| Restart | Versioned checkpoint, fingerprint, offset, watermark, fallible restore |
| Replay | Incremental, batch, and checkpoint-resume equivalence tests |
| Trading vertical | Research proving ground with simulated execution |
| Broker and risk | Not implemented as a production control plane |

## Missing before live capital

### Transactional processing

Choose one source, checkpoint store, and sink protocol. Prove every crash boundary. Use a shared
transaction when the infrastructure permits it; otherwise, require stable idempotency identities.

For hypothesis services, `process_and_snapshot` prevents in-process callers from interleaving
between a transition and its snapshot. It is not a storage transaction. Persist the source
position, snapshot, and output outbox together before acknowledging the source. A source driver
with strict delivery requirements should own its `HypothesisEngine` directly.

### Service scope and backpressure

Partition keyed state so each engine has one owner. Inject the service as a typed constructor
dependency. When multiple Tokio tasks need access, use the bounded actor handle and monitor mailbox
depth, response latency, worker termination, active-key capacity, timer capacity, and rejection
counts. Use the `Arc<Mutex<_>>` adapter only at an integration boundary, and never perform network,
disk, broker, or model I/O while holding its lock.

### Risk isolation

Keep order authorization outside the research signal process. Enforce position, notional, concentration, stale-data, kill-switch, and venue limits in an independently deployable control plane.

### Market semantics

Use venue-grade session calendars, corporate-action handling, symbol master data, and clock synchronization. A weekday calendar is a test utility, not a venue model.

### Costs and execution

Model spread, fees, slippage, queue position, partial fills, latency, and capacity. Rare-event conditions can invalidate calm-market cost assumptions precisely when the signal is strongest.

### Observability

Expose source lag, watermark lag, pending reorder depth, rejected input, open buckets, checkpoint age, restore outcomes, signal counts, and sink acknowledgements. Alert on invariant violations, not only process liveness.

For physical-event strategies, also expose source quality flags, publication-to-receipt latency,
model version, revision count, probability-space conversion, and every numerical rejection. A
non-finite forecast must stop the affected hypothesis transition without changing durable state.

### Deployment proof

Run deterministic shadow mode, restart drills, corrupt-checkpoint tests, backpressure tests, and staged incident exercises before enabling order authorization.

## Capital admission gate

Do not enable capital until the system can answer, from durable evidence:

1. Which input prefix produced this decision?
2. Which operator and policy versions were active?
3. Which state was restored after the last failure?
4. Which risk control authorized the order?
5. Which market and cost assumptions supported the expected return?
6. Which source revisions and availability timestamps formed the physical forecast?
7. Did the strategy pass shadow execution using real publication latency and source outages?

For the implemented durable shard and its remaining admission gates, read
[Durable hypothesis execution on Golem](./golem-cloud).
