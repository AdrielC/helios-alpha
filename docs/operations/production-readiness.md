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
| Costs and capacity | Checked fixed-point notional, spread, fees, latency slippage, square-root impact, and participation ceiling | Monotonicity, capacity, rounding, and overflow tests |
| Operations | Injected metrics sink plus readiness policy over lag, checkpoints, outbox age, reconciliation, clocks, calendar coverage, incidents, and kill switch | Complete metric-set, all-blocker, and incident-transition tests |
| Capital admission | Mandatory, expiring evidence ledger produces a private live authorization only when operations are ready | Missing, failed, expired, weakened-policy, and successful-admission tests |
| Portable deployment | `helio_execution` contains no native broker SDK and compiles for WASI | CI checks `wasm32-wasip2` |

Run the focused proof locally:

```bash
cd rust
cargo test -p helio_scan -p helio_time -p helio_execution
cargo clippy -p helio_execution -p helio_time --all-targets -- -D warnings
cargo check --target wasm32-wasip2 -p helio_execution --no-default-features
```

The integration test `capital_path` crosses both ambiguous boundaries: it loses the atomic commit
acknowledgement, retries the identical transaction, loses the broker acknowledgement after
acceptance, reconciles by client order identity, and observes one paper order.

## What remains external

The library cannot manufacture production evidence. These items remain closed until they are
performed against the selected deployment and counterparties:

- certify one real broker adapter, account mode, order-type subset, callback contract, and rate
  limits;
- calibrate spread, fees, latency, impact, and capacity by venue, instrument, regime, and shock
  severity;
- generate and refresh the venue schedule through the production distribution path;
- prove symbol-master and corporate-action handling for every traded instrument;
- run a clock-synchronization alert drill and an on-call incident exercise;
- complete Golem restart recovery with the production component and storage configuration;
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
      idempotent order gateway ── lookup before retry ── broker adapter
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
