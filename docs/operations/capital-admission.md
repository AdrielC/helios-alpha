# Capital admission

Capital admission is a result, not a configuration flag. Helios issues a short-lived live
authorization only when required evidence is complete, current, content-addressed, production
scoped, and paired with a ready operational snapshot.

## The four proof planes

### 1. Atomic processing

`AtomicCommitBundle` binds one transaction identity to:

- the expected next source offset;
- the next source offset after a contiguous input prefix;
- the checkpoint whose offset equals that next source offset; and
- every output derived from that prefix, each with a stable `OutputId`.

The store commits all four records together. If the caller loses the commit response, it retries the
same transaction identity and content. The outbox then delivers through an idempotent sink. A lost
sink acknowledgement repeats the same output identity, not a new effect.

| Failure | Durable state | Required recovery |
|---|---|---|
| Before commit | No offset, checkpoint, or output advances | Retry the same input prefix |
| After commit, before response | All three advance together | Retry the identical transaction identity |
| After broker accepts, before response | Outbox remains pending | Query broker by `client_order_id` before retry |
| After sink acknowledgement | Outbox records the broker acknowledgement | Resume from committed next offset |

The in-memory store is reference semantics and a fault harness. A production adapter must map the
same invariants to one serializable database transaction or an equivalently strong source protocol.

### 2. Market and execution controls

`VenueSchedule` accepts only finite, validated schedule manifests. The manifest includes the venue,
IANA time zone, source version, coverage interval, generation time, and a SHA-256 digest over every
session. Unknown sessions and timestamps outside coverage are errors. There is no weekday fallback
in the execution path.

The Python exporter pins the
[`exchange_calendars` 4.13.2 release](https://github.com/gerrymanoim/exchange_calendars/releases/tag/4.13.2).
Its XNYS fixture preserves the 2026 Thanksgiving closure and the 13:00 ET close on the following
Friday. [NYSE hours and calendars](https://www.nyse.com/markets/hours-calendars) remain the
conformance authority for production refreshes.

`RiskAuthority` is idempotent by proposal identity. It rejects:

- stale or future market data;
- a closed session or unapproved venue;
- a mismatched trading day;
- order, gross, strategy, symbol-position, or daily-count breaches;
- disabled live mode; and
- an active kill switch.

An approval reserves capacity before a second proposal can be assessed. An ordinary portfolio
refresh leaves every reservation in place. `refresh_portfolio_covering` releases a reservation only
when the caller explicitly states that the authoritative snapshot includes that order's exposure,
position, and daily-order accounting. Unknown identities reject the whole refresh, and partial fills
remain fully reserved until covered after terminal reconciliation. `OrderGateway` separately requires
an allowed risk-policy version, a fresh risk decision, and a current production capital authorization.

### 3. Costs, slippage, and capacity

`CostModel` uses integer fixed-point arithmetic. It estimates:

```text
total = half spread + fees + latency slippage + square-root market impact
```

Participation is `order quantity / average daily quantity`. Orders above the configured
participation ceiling fail rather than receiving an optimistic estimate. Every division rounds cost
up, and overflow is a typed error.

This is a safe calculation kernel, not a universal calibration. Production evidence must fit and
validate parameters for the traded instrument, venue, order type, time of day, and shock regime.
Queue position, partial fills, auction behavior, and emergency liquidity need adapter-specific
models before those order types are enabled.

### 4. Operations and deployment

`OperationalSnapshot` closes admission on any of these conditions:

- the snapshot is stale or timestamped in the future;
- source lag, checkpoint age, or pending-outbox age above policy;
- unresolved broker reconciliation;
- clock offset above policy;
- venue-calendar coverage shorter than the required horizon;
- active kill switch; or
- open incident.

`IncidentJournal` makes open, acknowledgement, and resolution explicit. Resolution requires an
actor and a written cause or corrective action. A process restart does not resolve an incident.

`emit_operational_metrics` sends a stable eight-gauge snapshot through an injected
`ObservabilitySink`. Native services can adapt that sink to OpenTelemetry or Prometheus without
putting either SDK in the WASI control kernel. Readiness blockers are the alert dimensions, so the
capital gate and the monitoring system evaluate the same state.

CI uploads content-hashed logs for the portable control tests. Those logs can support the local
crash and fault-injection evidence kinds. They do not satisfy broker certification, deployment
verification, incident exercise, or shadow-run evidence for a live environment.

## Issuing an authorization

```rust
let policy = CapitalAdmissionPolicy::production_default(
    "capital-policy-2026-08",
    60_000_000_000,
);

let authorization = evaluate_capital_admission(
    &policy,
    &evidence_ledger,
    &operational_readiness,
    now_ns,
)?;

gateway.dispatch(&order_intent, Some(&authorization), now_ns)?;
```

`CapitalAuthorization` has private fields and no deserializer. The gateway checks its environment
and expiry. In production, admission belongs in an independently deployed authority whose evidence
store and signing boundary are not writable by the strategy process.

## Admission artifact contract

For every evidence record, retain:

- immutable artifact ID and SHA-256;
- source commit and build digest;
- target environment and component version;
- observed and expiry timestamps;
- command or drill definition;
- pass criteria and observed result;
- reviewer or automated authority; and
- links to raw logs, metrics, broker records, and incident timeline.

If the evidence is unavailable, unverifiable, expired, or scoped to staging, production admission
stays closed.
