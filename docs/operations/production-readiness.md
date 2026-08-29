# Production readiness

The core is productionizable in the sense that its contracts are explicit, bounded, testable, and restartable. The repository is not yet a production trading system.

## Current readiness

| Area | Current state |
|---|---|
| Scan composition | Static, typed, allocation-aware hot path |
| Event ordering | Capacity-bounded with typed late and overflow outcomes |
| Online statistics | Stable updates, deterministic merge utility, validated snapshots |
| Restart | Versioned checkpoint, fingerprint, offset, watermark, fallible restore |
| Replay | Incremental, batch, and checkpoint-resume equivalence tests |
| Trading vertical | Research proving ground with simulated execution |
| Broker and risk | Not implemented as a production control plane |

## Required before live capital

### Transactional processing

Choose a source, checkpoint store, and sink protocol. Prove crash behavior at every boundary. Use shared transactions where available or stable idempotency identities where they are not.

### Risk isolation

Keep order authorization outside the research signal process. Enforce position, notional, concentration, stale-data, kill-switch, and venue limits in an independently deployable control plane.

### Market semantics

Use venue-grade session calendars, corporate-action handling, symbol master data, and clock synchronization. A weekday calendar is a test utility, not a venue model.

### Costs and execution

Model spread, fees, slippage, queue position, partial fills, latency, and capacity. Rare-event conditions can invalidate calm-market cost assumptions precisely when the signal is strongest.

### Observability

Expose source lag, watermark lag, pending reorder depth, rejected input, open buckets, checkpoint age, restore outcomes, signal counts, and sink acknowledgements. Alert on invariant violations, not only process liveness.

### Deployment proof

Run deterministic shadow mode, restart drills, corrupt-checkpoint tests, backpressure tests, and staged incident exercises before enabling order authorization.

## Go-live gate

Do not enable capital until the system can answer, from durable evidence:

1. Which input prefix produced this decision?
2. Which operator and policy versions were active?
3. Which state was restored after the last failure?
4. Which risk control authorized the order?
5. Which market and cost assumptions supported the expected return?
