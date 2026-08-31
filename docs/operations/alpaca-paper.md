# Alpaca paper execution

Helios has one real paper-trading vertical slice. `helio_operatord` can consume authenticated
Alpaca stock market data, execute reviewed paper orders through independent risk and the standalone
OMS, reconcile asynchronous order updates, and project orders, fills, positions, exposure, and
source health into Helios Control.

This path is for operational proof. It is not evidence of alpha and it cannot route live orders.
The runtime constructs only `AlpacaConfig::paper()`. Invalid or incomplete configuration falls back
to a read-only command executor.

## What crosses the boundary

```text
Alpaca IEX/SIP WebSocket                     Helios Control
          │                                       ▲
          ▼                                       │ complete snapshot + SSE
bounded causal normalizer ──► exact market ref ──► helio_operatord
                                                   │
reviewed command + sequence + CSRF                  │
          │                                        ▼
          └────────► risk reservation ──► standalone OMS ──► Alpaca paper HTTPS
                                                   ▲                 │
                                                   └── trade_updates ┘
```

The execution path uses fixed-point micros. Floating point is permitted only when a chart value is
materialized for display. Broker decimals with more than six significant fractional digits fail
instead of rounding silently.

## Generate a venue schedule

The risk authority does not infer weekdays or exchange hours. Generate a finite, content-hashed
schedule from the pinned `exchange_calendars` dependency. Choose a range that covers the complete
paper run:

```bash
python -m helios_alpha.markets.calendar_manifest \
  --exchange XNYS \
  --start 2026-08-31 \
  --end 2026-12-31 \
  --generated-at 2026-08-31T18:00:00Z \
  --output /tmp/helios-xnys.json
```

The service validates the manifest hash, session ordering, venue, and finite coverage at startup.
Outside the admitted session or schedule coverage, an order fails closed.

## Start the paper runtime

Build the operator before starting the native gateway:

```bash
npm ci
npm run operator:build

cd rust
export HELIOS_STATIC_DIR=../apps/operator/dist
export HELIOS_ALPACA_PAPER_ENABLED=1
export HELIOS_ALPACA_SYMBOLS=SPY
export HELIOS_ALPACA_FEED=iex
export HELIOS_RISK_POLICY_PATH=../config/risk/alpaca-paper.json
export HELIOS_VENUE_SCHEDULE_PATH=/tmp/helios-xnys.json
export APCA_API_KEY_ID='your-paper-key-id'
export APCA_API_SECRET_KEY='your-paper-secret'
export HELIOS_OPERATOR_SESSION_TOKEN='at-least-32-random-characters'
export HELIOS_COMMAND_CSRF_SECRET='another-independent-32-character-secret'
cargo run -p helio_operatord
```

`HELIOS_MARKET_REFERENCE_PATH` is optional and seeds references only. It does not replace the live
market-data stream or relax freshness checks. `HELIOS_ALPACA_FEED` accepts `iex`, `sip`, or
`delayed_sip`; the account must have the selected entitlement.

The two command secrets do not create a login flow. A same-origin identity proxy must install the
session value in an HttpOnly, Secure, SameSite=Strict cookie named `helios_operator_session`.
Without that cookie, the command-session and command endpoints reject every request. For a local
protocol check, call the API with an explicit test cookie; never expose the secret in browser
JavaScript or `runtime-config.js`.

## Runtime guarantees

- Credentials and HTTP request debug output are redacted; credential buffers are zeroized on drop.
- Native HTTPS follows no redirects and bounds response bodies.
- Market frames are byte and event-count bounded before normalization.
- Quote ask or bid, whichever is larger, is the conservative market-order risk reference.
- Raw trades do not become risk references because Alpaca may later correct or cancel them.
- Every submit is written to the OMS before send, then looked up by stable client order ID before a
  POST is attempted.
- An ambiguous POST outcome is resolved by client-order lookup. It is never reported as a new
  rejection or silently retried with another identity.
- Broker account and positions are refetched before risk evaluation.
- A terminal trade update is reconciled against authoritative order and FILL activity, then
  positions are refetched before the risk reservation is released.
- Broker and OMS status is labeled `matched` only for an explicit normalized state pair.
- Stale market data, stale portfolio state, closed sessions, blocked accounts, limit violations,
  and an active kill switch all stop submission.

The injected-transport suite proves exact submission, replay without a second POST, stale-reference
rejection, broker-confirmed cancellation, asynchronous fill reconciliation, position projection,
and risk-reservation release:

```bash
cd rust
cargo test -p helio_alpaca -p helio_operatord
cargo clippy -p helio_alpaca -p helio_operatord --all-targets -- -D warnings
```

These tests use recorded protocol-shaped responses. They do not use repository or CI credentials,
and they do not prove a current Alpaca account, entitlement, or network path.

## Deliberately still closed

The current process state is not durable. The reference OMS, risk reservations, market checkpoint,
operator snapshot, and idempotency registry are in memory. A process restart therefore requires a
broker reconciliation run and cannot yet claim uninterrupted operation.

The next acceptance slice must add:

1. a Golem account owner for OMS and risk state;
2. a NATS JetStream relay for replayable projections, not order truth;
3. a persisted market checkpoint and historical backfill-to-live handoff;
4. startup reconciliation of broker orders, activities, positions, and local reservations;
5. coordinated task shutdown and bounded reconnect budgets;
6. authenticated deployment, alerting, incident drills, and retained paper-run evidence;
7. reviewed replace and liquidation-plan commands before those controls are exposed.

Until that work and the [capital admission](./capital-admission) evidence are complete, this remains
a paper-only operational proving path.
