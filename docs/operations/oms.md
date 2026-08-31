# Order management

Helios now has a standalone OMS core and a stable seam for plugging into another OMS. The same
capital controls apply in both modes. Replacing order management never grants a strategy direct
venue authority.

The [operator capability map](./operator-capability-map.md) is the acceptance inventory for the
standalone application and external-OMS overlay. It includes the synchronized evidence timeline,
complete institutional lifecycle, post-trade operations, and bounded AI investigation contract.

## What is implemented

`helio_oms` is a platform-neutral Rust crate that compiles for native targets and WASI. It owns:

- versioned order aggregates;
- idempotent commands with conflict detection;
- pending, working, partial-fill, cancel, replace, terminal, and uncertain states;
- exact quantity and price micros with checked `u128` accumulation;
- execution identity de-duplication and fail-closed overfill checks;
- monotonic observation time while preserving the venue's original fill time;
- monotonic event cursors and versioned event envelopes;
- `OmsCommandPort`, `OmsQueryPort`, and `OmsEventSource` interfaces;
- a reusable conformance routine for external implementations;
- FIX 4.4 application-message encoding, BodyLength and CheckSum validation, order mapping, and
  ExecutionReport normalization, plus OrderCancelReject recovery for rejected cancel and replace
  requests.

The Golem application exports `OmsAccountAgent`. The account identifier is its durable identity.
Every invocation for one account runs sequentially. Its typed methods submit, acknowledge, record
fills, request and confirm cancellation, request and confirm replacement, resolve a reconciled
unknown outcome, query an order, and page committed events. Periodic snapshots contain the complete
portable OMS state.

## Standalone topology

```text
research candidate
       │
       ▼
independent RiskAuthority
       │ authorized OrderIntent
       ▼
capital admission
       │
       ▼
OmsAccountAgent(account)
       │
       ├── committed OMS events ──► NATS relay ──► operator and projections
       │
       └── outbound intent ──► capital-gated OrderGateway ──► native FIX session engine ──► venue
                                                               │
                                                               └── ExecutionReport ──► canonical OMS commands
```

The native FIX process owns the live session. The Golem agent owns the order. `OrderGateway` still
requires current capital admission before a live dispatch. If the process loses a response, it
reconciles the venue by client order identity and applies the returned report with a stable command
identity.

## External OMS topology

An external OMS implements the same three ports or receives a thin adapter:

```rust
pub trait OmsPort: OmsCommandPort + OmsQueryPort + OmsEventSource {}
```

At startup, the adapter reports `OmsCapabilities`. Helios currently requires limit orders, stable
client order IDs, cancellation, exact decimal quantities, lifecycle query, and a cursor or
equivalent event resume token. Replace support is negotiated. The adapter must pass
`verify_oms_conformance` in a non-production account before it can produce broker-certification
evidence.

Independent risk stays in Helios. An external OMS may add stricter risk, but it cannot weaken the
Helios authorization, capital-admission, or kill-switch checks.

## FIX boundary

`FixOrderMapper` produces FIX 4.4 `NewOrderSingle`, `OrderCancelRequest`, and
`OrderCancelReplaceRequest` application messages. `FixMessage` validates exact BodyLength and
CheckSum values. `FixExecutionReport` maps acknowledgements, trades, cancellations, replacements,
rejections, and expiry into idempotent OMS commands.

This is deliberately not a home-grown production session engine. A certified engine or venue SDK
must own:

- TLS and counterparty authentication;
- Logon, Logout, Heartbeat, TestRequest, and session schedules;
- persistent inbound and outbound sequence numbers;
- ResendRequest, SequenceReset, gap fill, duplicate, and poss-dup handling;
- counterparty dictionaries and venue-specific tags;
- throttles, drop copy, and certification scripts.

`FixSessionPort` is the injection seam for that engine. Its sequence and resend methods make session
responsibility visible without importing native sockets into the WASI-safe OMS core. The normative
session reference is the [FIX session layer standard](https://www.fixtrading.org/wp-content/uploads/download-manager-files/FIX_Session_Layer_June_2020.pdf).

## Recovery rules

1. A command identity is permanent. Reusing it with different contents is an incident.
2. A client order identity is permanent within its account. Reusing it with another intent is an
   incident.
3. An execution identity is permanent. A changed quantity or price is rejected instead of silently
   correcting history. Its venue order identity and original occurrence time are also part of that
   identity.
4. An ambiguous venue outcome moves to `Unknown`. Missing executions retain that state until an
   explicit `ReconcileUnknown` command records the venue's resolved status. It never becomes a new
   order.
5. The event relay resumes from its last acknowledged account cursor and publishes only committed
   events. It advances the durable cursor only after JetStream acknowledges persistence. A crash
   after publication reuses the stable event identity, so JetStream de-duplicates the retry.
6. Positions are projections of fills plus explicit adjustments. They are not inferred from order
   status text.
7. Aggregate observation time cannot move backward. A late venue report uses a new local
   observation time and retains the earlier FIX `TransactTime` on the fill event. Event-time order
   and processing-time order are never conflated.
8. A replacement is validated both when requested and when confirmed. Fills received while the
   replacement is pending can make the new quantity invalid, in which case confirmation fails
   closed and reconciliation is required.

## Evidence still required

Implemented code is not permission to trade. Before live capital, retain artifacts for:

- Golem crash and full-restart recovery of the account agent;
- NATS publisher acknowledgement loss and consumer redelivery in the production cluster;
- FIX sequence gaps, resend, duplicate reports, disconnects, and ambiguous submission;
- venue conformance for every supported order type and time in force;
- drop-copy or broker statement reconciliation to OMS fills and positions;
- load, latency, and capacity at the intended account and order rate;
- operator cancel, flatten, and kill-switch drills;
- deployment digest, health, rollback, and shadow-run results.
