# Operator capability map

Helios must be able to operate as a standalone OMS and as an evidence and control layer over an
external OMS. This map defines the product surface required for both modes. It is an acceptance
inventory, not a claim that every item is implemented.

The institutional comparison set is Charles River Trader, Bloomberg AIM, Trading Technologies,
and the FIX order and post-trade workflows. Their public product material establishes the expected
scope: complete order lifecycle, multi-asset execution, compliance, allocation, reconciliation,
post-trade operations, transaction-cost analysis, and persistent audit history.

## Operating model

The operator application has three primary workspaces:

1. **Evidence** answers what happened, what information was available, and what changed after the
   decision.
2. **Trade** manages orders, executions, allocations, exceptions, and post-trade obligations.
3. **Control** manages risk, capital authority, incidents, connectivity, and system health.

Navigation must preserve one organization, workspace, account, portfolio, strategy, instrument,
and time-window context across those workspaces. A selected entity opens the same durable record
from any view.

## Synchronized evidence timeline

The signature Helios surface is a configurable set of time series tied to one event-time axis. It
must support:

- stacked and overlaid market, model, source, portfolio, risk, execution, and derived series;
- shared crosshair, zoom, pan, range selection, keyboard scrubbing, and explicit time zone;
- raw value, normalized, basis-point, percentage, and z-score views without mutating source data;
- separate observed, available, decision-cut, command, venue, and local-processing timestamps;
- order, acknowledgement, reject, cancel, replace, fill, bust, correction, alert, deployment, and
  model-version markers;
- a series catalog with search, units, provenance, freshness, lineage, and stable identifiers;
- saved layouts scoped to user, team, strategy, account, and incident;
- point-in-time inspection that distinguishes evidence available at the decision from late arrival;
- replay against versioned models, reference data, calendars, risk policy, and deployment digests;
- export of the selected interval, series definitions, markers, and evidence manifest;
- bounded rendering with downsampling or aggregation that never changes stored truth.

On a narrow viewport, the timeline remains the primary surface. Ledgers and evidence move into
explicit panes. Horizontal scrolling is valid for time series and wide ledgers, but the application
page itself must not acquire accidental horizontal overflow.

## Complete OMS inventory

Each row must eventually have an owner, implementation status, conformance test, and evidence
artifact.

| Area | Required capability | Current Helios status |
| --- | --- | --- |
| Identity and entitlement | organization, account, portfolio, sleeve, desk, trader, service identity, roles, command authority, segregation of duties | Partial read model and command session |
| Instrument and reference data | security master, symbology, venue and broker identifiers, tick and lot rules, currencies, settlement conventions, corporate actions | Not implemented |
| Trading calendar | venue sessions, holidays, auctions, halts, expiry, good-till dates, time-zone and daylight-saving rules | Calendar primitives exist; venue certification required |
| Order capture | single, basket, program, staged, held, scheduled, import, rebalance, strategy-generated, and API orders | Single market and limit ticket only |
| Order semantics | side, quantity, price, order type, TIF, capacity, open or close, short-sale and locate, client tags, strategy, portfolio, owner | Partial |
| Parent and child hierarchy | block, parent, child, slice, route, algo, crossing, internal transfer, and multi-day order series | Core order aggregate only; UI and hierarchy model required |
| Pre-trade controls | limits, buying power, exposure, concentration, restricted lists, locate, fat-finger, duplicate, self-match, compliance, capital admission | Independent risk and capital seam exist; policy suite required |
| Approval workflow | maker-checker, escalations, overrides, reason capture, expiry, delegation, and electronic evidence | Command review seam only |
| Routing and execution | broker and venue eligibility, route selection, FIX or venue SDK, algos, broker wheel, throttles, price discovery, quotes, RFQ | Injection seams exist; certified session and routing services required |
| Lifecycle control | submit, acknowledge, reject, hold, release, partial fill, cancel, mass cancel, cancel reject, replace, expire, suspend, resume, unknown, reconcile | Core lifecycle implemented for the documented subset |
| Fill processing | execution identity, partial fills, liquidity, fees, commissions, taxes, busts, corrections, manual fills, average price, drop copy | Deduplication and canonical fills implemented; corrections and economics required |
| Allocation | pre-allocation, post-allocation, allocation rules, rounding, account eligibility, average price, give-up, step-out, allocation breaks | Not implemented |
| Positions and cash | fill-derived positions, start-of-day, adjustments, realized and unrealized P&L, cash, financing, borrow, multi-currency, FX translation | Read projection is partial |
| Reconciliation | order, fill, position, cash, fee, drop-copy, broker statement, internal ledger, break assignment, tolerance, resolution | State flag only; workflow and evidence required |
| Post-trade | confirmation, affirmation, matching, settlement instruction, settlement status, fail management, registration, collateral handoff | Not implemented |
| Compliance and surveillance | pre-trade, post-execution, end-of-day rules, breach workflow, regulatory tags, reporting, historical testing | Not implemented |
| TCA and best execution | arrival, decision, implementation shortfall, spread capture, slippage, delay, participation, capacity, broker and route comparison | Research models and UI required |
| Audit and records | immutable command and event history, actor, reason, before and after, source and venue messages, model and policy version, export and retention | Event envelopes exist; operator record and retention proof required |
| Exception management | severity, ownership, acknowledgement, snooze policy, investigation, linked entities, runbook, resolution, recurrence | Alert read model exists; workflow is partial |
| Operational control | kill switch, cancel-all, flatten, strategy pause, source isolation, failover, incident declaration, recovery drill | Command contract is partial; live proof required |
| Connectivity | FIX session health, sequence gaps, resend, drop copy, market data, broker and venue state, NATS relay, Golem worker, clock health | Interfaces and selected components exist; integrated proof required |
| Search and reporting | global entity search, saved filters, workspaces, exports, scheduled reports, regulatory extracts, operational metrics | Perspective explorer is partial |
| Configuration | accounts, brokers, routes, calendars, limits, compliance rules, allocations, reference data, feature flags, deployment version | Not implemented as operator surfaces |

Asset-class and venue additions may extend this inventory. They may not weaken idempotency,
reconciliation, capital admission, timestamp provenance, or audit requirements.

## AI-native operator assistance

AI is a bounded capability behind an injected `InvestigationPort`. It is not a global chat box and
does not share the command port.

Permitted operations include:

- build a cited timeline for a selected order, fill, incident, or interval;
- identify missing, late, conflicting, or stale observations;
- suggest relevant series from the registered catalog and explain why;
- compare the decision state with replay, counterfactual, benchmark, or control windows;
- summarize an exception, allocation break, reconciliation break, or execution outlier;
- draft a saved workspace, filter, report, incident note, or command rationale for operator review;
- surface uncertainty, unavailable evidence, and alternative explanations.

Every AI result must carry the snapshot sequence, time window, entity identifiers, evidence
references, model identifier, generation timestamp, and uncertainty or limitation statement. An
operator must be able to open every cited record. Results are append-only evidence when attached
to an incident or order.

The assistant may never submit, replace, cancel, allocate, reconcile, flatten, change limits, clear
a breach, activate capital, or acknowledge an alert. It may draft a request for the ordinary typed
command-review path. Authorization, risk evaluation, idempotency, durable recording, and receipt
validation remain deterministic services.

## Data and port boundaries

The production and demonstration applications use the same views. Only composition changes:

- `OperationsPort` supplies versioned snapshots and resumable live updates;
- `TimeSeriesPort` queries bounded windows and streams keyed deltas;
- `EntityHistoryPort` pages immutable order, execution, allocation, and audit records;
- `WorkspacePort` loads and saves user and team layouts;
- `InvestigationPort` returns cited, non-authoritative analysis;
- `CommandPort` remains separately authenticated and fail-closed.

The demo adapters implement those contracts with synthetic, labeled data. Demo branches do not
belong in view logic and production views must not infer authority from an adapter name.

## Sources used to set the comparison bar

- [Charles River Trader](https://www.crd.com/solutions/charles-river-trader/)
- [Bloomberg AIM](https://professional.bloomberg.com/products/trading/order-management-system/aim/)
- [Trading Technologies order book](https://library.tradingtechnologies.com/trade/order-management/order-book/reference-order-book/order-book-reference/)
- [Trading Technologies audit trail](https://library.tradingtechnologies.com/trade/order-management/audit-trail/description-audit-trail/audit-trail-overview/)
- [FIX order and post-trade specifications](https://staging.fixtrading.org/online-specification/)
