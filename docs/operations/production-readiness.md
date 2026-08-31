# Production readiness

Helios now has an executable capital-control reference path. It still does not have permission to
trade live capital. The distinction is deliberate: control mechanics live in code, while admission
depends on current evidence from the actual source, broker, environment, and on-call team.

## What is implemented

| Control | Executable mechanism | Proof in this repository |
|---|---|---|
| Source, state, and sink coordination | One commit bundle advances a contiguous source offset, stores its checkpoint, and appends stable output identities | Before-commit failure, lost commit acknowledgement, identity conflict, and lost sink acknowledgement tests |
| Venue sessions | Finite, versioned, content-hashed schedules with no weekday fallback outside coverage | Python exports XNYS sessions from pinned `exchange_calendars`; Rust validates the same fixture, Thanksgiving closure, and early close |
| Pre-trade risk | Independent, idempotent risk authority with stale-market-data, stale-portfolio, session, venue, order, gross, strategy, position, daily-count, and kill-switch checks | Limit and identity fault tests |
| Broker boundary | Stable client order identity, write-before-send journal, lookup-before-retry reconciliation, and a fault-injecting paper broker | Accept-then-timeout and unavailable-before-accept tests prove one accepted paper order |
| Order management | Portable, event-sourced lifecycle with versioned commands, exact fixed-point fills, cancel and replace states, event cursors, and an external OMS conformance contract | Replay, identity conflict, overfill, cursor, FIX framing, checksum, and execution-report tests |
| Durable OMS owner | One Golem agent per account with sequential commands, periodic snapshots, typed submit/fill/cancel/replace/reconcile methods, and bounded event reads | Local Golem deployment proves command de-duplication, exact fill state, simulated crash, full server restart, and event-cursor resume; production evidence remains required |
| Durable operational events | A bounded relay validates contiguous account batches, waits for JetStream persistence acknowledgement, publishes stable `Nats-Msg-Id` values, then compare-and-set advances a Golem-owned projection cursor | Fault tests prove no advance before acknowledgement and identical replay identity after the publish/checkpoint crash gap; a real NATS v2.14.5 smoke proves server-side de-duplication |
| Alpaca paper vertical | Closed paper origins, exact decimals, bounded market normalization, lookup-before-submit, standalone OMS, independent risk, trade-update reconciliation, and operator projections | Injected-transport tests prove one POST across replay, stale-reference rejection, confirmed cancel, asynchronous fill, position refresh, and reservation release; current credentials and network are not tested in CI |
| Scientific shadow | Strict NOAA/NASA normalization, causal receipt availability, append-only revisions, atomic snapshot/checkpoint commits, bounded atomic operator projection, and versioned forecast inputs | Unit tests cover failure semantics; a current NOAA X-ray live poll succeeded locally and the non-blocking integration job repeats that contract check |
| Robinhood Crypto adapter | Official Ed25519 request signing, exact fixed-point limit orders, bounded order lookup, fill normalization, and cancellation through injected transport and clock | Canonical-message signature verification, exact-body, pagination, unknown-outcome, redaction, native clippy, and WASI checks |
| Costs and capacity | Checked fixed-point notional, spread, fees, latency slippage, square-root impact, and participation ceiling | Monotonicity, capacity, rounding, and overflow tests |
| Operations | Injected metrics sink plus readiness policy over lag, checkpoints, outbox age, reconciliation, clocks, calendar coverage, incidents, and kill switch | Complete metric-set, all-blocker, and incident-transition tests |
| Capital admission | Mandatory, expiring evidence ledger produces a private live authorization only when operations are ready | Missing, failed, expired, weakened-policy, and successful-admission tests |
| Portable deployment | `helio_execution` contains no native broker SDK and compiles for WASI | CI checks `wasm32-wasip2` |

Run the focused proof locally:

```bash
cd rust
cargo test -p helio_scan -p helio_time -p helio_execution -p helio_oms -p helio_robinhood
cargo test -p helio_robinhood --all-features
cargo clippy -p helio_execution -p helio_oms -p helio_time -p helio_robinhood --all-targets --all-features -- -D warnings
cargo test -p helio_alpaca -p helio_operatord
cargo clippy -p helio_alpaca -p helio_operatord --all-targets -- -D warnings
cargo check --target wasm32-wasip2 -p helio_execution -p helio_oms -p helio_robinhood --no-default-features
```

The integration test `capital_path` crosses both ambiguous boundaries: it loses the atomic commit
acknowledgement, retries the identical transaction, loses the broker acknowledgement after
acceptance, reconciles by client order identity, and observes one paper order.

## What remains external

The library cannot manufacture production evidence. These items remain closed until they are
performed against the selected deployment and counterparties:

- certify the implemented Robinhood Crypto adapter against its live-only account, supported
  limit-order subset, polling contract, and rate limits, or certify another selected broker;
- run the Alpaca paper adapter against an authenticated account, retain order/fill/position
  evidence, and prove Golem-backed startup reconciliation against that account;
- calibrate spread, fees, latency, impact, and capacity by venue, instrument, regime, and shock
  severity;
- generate and refresh the venue schedule through the production distribution path;
- prove symbol-master and corporate-action handling for every traded instrument;
- run a clock-synchronization alert drill and an on-call incident exercise;
- complete Golem restart recovery with the production component and storage configuration;
- deploy a replicated NATS cluster with authenticated least-privilege accounts, verify the exact
  bounded stream policy, and drill publisher outage plus consumer redelivery;
- deploy the exact immutable artifact, verify health and rollback, and retain its digest;
- complete a predeclared shadow period with real publication latency, revisions, disconnects, and
  broker acknowledgements.

Until those artifacts are present and unexpired, `evaluate_capital_admission` returns blockers and
`OrderGateway` rejects every live order.

## Service boundary

Research produces an `OrderProposal`. It never receives broker credentials.

```text
source prefix + checkpoint + candidate outbox
                     │
                     ▼
            independent risk authority
              │ approve      │ reject
              ▼              └── durable reason
          OrderIntent
              │
              ▼
        capital admission gate ── closed unless all evidence is current
              │
              ▼
           OmsPort
        ┌─────┴─────────┐
        ▼               ▼
 built-in Golem OMS   external OMS adapter
        │               │
        └──── capital-gated OrderGateway
                         │
                         └──── FIX or broker-native execution ──── venue
```

The production risk authority and order gateway should be independently deployable and should own
their state. Sharing Rust types does not require sharing a process, lock, or credential boundary.

## Evidence before capital

Production admission requires every evidence kind below. Application code cannot weaken this set.

1. Atomic crash matrix
2. Venue-calendar conformance
3. Broker certification
4. Broker reconciliation fault injection
5. Risk-limit fault injection
6. Cost and capacity calibration
7. Observability and alert drill
8. Incident-response exercise
9. Golem restart recovery
10. Deployment verification
11. Shadow run

Each artifact carries an identity, SHA-256 digest, environment, observation time, expiry, and pass
state. Admission expires at the earliest artifact expiry or policy TTL. A live gateway also checks
the risk-policy version and the age of the risk authorization.

Read [Capital admission](./capital-admission) for the protocol and [Incident response](./incident-response)
for the operating sequence.
