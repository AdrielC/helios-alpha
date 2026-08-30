# Robinhood boundary

Robinhood is viable for one narrow production slice today: spot crypto limit orders through the
official Crypto Trading API. Helios now implements that protocol in `helio_robinhood`. The adapter
is not broker-certified and capital admission remains closed.

Robinhood also offers Agentic Trading for equities, options, and crypto through an MCP server. That
surface is useful for supervised research workflows, but it is not yet the durable execution port.
Its public documentation does not specify the idempotency, pagination, polling, error, and recovery
contract that Helios requires at an automated capital boundary.

## Capability decision

| Surface | Instruments | Helios status | Why |
|---|---|---|---|
| Crypto Trading API | Spot crypto | Implemented, uncertified | Official signed REST API, caller-supplied UUID, order lookup, executions, and cancellation |
| Agentic Trading MCP | Long equities, options, crypto | Experimental only | Broad and convenient, but public docs do not expose enough recovery semantics for the broker port |
| Unofficial consumer-account automation | Any UI feature | Rejected | Robinhood prohibits third-party trading APIs without written authorization outside documented surfaces |

The Crypto API does not make Robinhood a complete venue for the space-weather strategy. It can
trade crypto reactions to an event shock. An infrastructure-equity basket needs a separately
certified equities route, potentially Agentic Trading after contract certification or another
broker adapter.

## What the adapter owns

`helio_robinhood` implements the generic `BrokerPort` and `BrokerLifecyclePort` contracts:

- canonical Ed25519 signing over the exact documented request message;
- injected clock, signer, and transport with no environment or service locator access;
- USD crypto-pair validation and limit orders expressed as decimal strings from integer micros;
- canonical UUID `client_order_id` values for Robinhood idempotency validation;
- lookup-before-retry across a bounded number of order pages;
- a fail-closed result when the page bound prevents proof that an order is absent;
- normalized pending, open, partial-fill, fill, cancellation, and failure states;
- deterministic execution identities and decimal fill values that never pass through `f64`;
- cancellation by stable client identity after resolving Robinhood's order identity;
- one-megabyte response bounds, HTTPS-only native transport, timeouts, and no redirects; and
- redacted debug output for accounts, API keys, signatures, bodies, and private-key material.

The portable protocol and signer compile for `wasm32-wasip2`. Native HTTP is an optional feature.
This preserves the Golem architecture: a WASI host can mediate outbound HTTP while the same adapter
logic owns validation, signing, normalization, and reconciliation.

## Order and reservation lifecycle

```text
research candidate
      │
      ▼
risk authority reserves worst-case capacity
      │
      ▼
capital admission checks current production evidence
      │
      ▼
durable gateway journals UUID before transmission
      │
      ▼
Robinhood Crypto limit order
      │
      ├── response lost ──► lookup UUID before any retry
      │
      ▼
poll order lifecycle and executions
      │
      ▼
authoritative portfolio snapshot covers terminal order
      │
      ▼
risk authority releases matching reservation
```

Reservations do not disappear merely because Robinhood reports a terminal state. The portfolio
adapter must first produce a snapshot that includes the order's exposure, position, and daily-order
accounting. It then passes that exact identity to `refresh_portfolio_covering`. This prevents both
permanent capacity leaks and unsafe early release against stale account state.

## What remains before one dollar

The repository tests protocol behavior without credentials or network writes. Certification must be
performed against the exact Robinhood account and deployment:

1. Create a dedicated Crypto API key, keep it outside the strategy process, and prove rotation and
   revocation. Never place keys in CI or repository secrets available to pull requests.
2. Put the gateway journal, order poller, and risk authority behind separate Golem capabilities.
   The research component receives none of them.
3. Add a shared request scheduler for Robinhood's documented baseline and burst rate limits. The
   adapter intentionally reports availability; it does not sleep inside the deterministic kernel.
4. Record request timestamp, body digest, client UUID, response status, Robinhood order ID, every
   lifecycle observation, and every execution without storing authentication headers.
5. Fault-test timeout before send, timeout after accept, pagination exhaustion, HTTP 429, malformed
   JSON, unknown state, partial fill, cancel ambiguity, key revocation, and process restart.
6. Reconcile orders, executions, holdings, buying power, and daily order count to the authoritative
   account snapshot. Exercise the coverage-based reservation release path.
7. Run shadow decisions with real data and no submission. Then require explicit approval for one
   predeclared, minimum-size live crypto canary because the official API does not document a paper
   environment.
8. Store the resulting logs, digests, account scope, reviewer, observed result, and expiry as the
   broker-certification evidence required by capital admission.

Steps 1 and 7 cross credential and capital boundaries. They are deliberately not automated by this
repository.

## Agentic Trading quarantine

The official endpoint is `https://agent.robinhood.com/mcp/trading`, tied to a dedicated Agentic
Trading account. It exposes account, portfolio, market-data, order review, placement, and
cancellation tools. Helios should not wrap those tools as `BrokerPort` until a certification harness
proves:

- a caller-controlled immutable client identity or an equivalent deduplication contract;
- deterministic lookup after an unknown placement outcome;
- complete order and execution lifecycle reads;
- cancellation recovery and rate-limit behavior;
- stable typed semantic errors distinct from transport errors; and
- a safe test or canary protocol for the exact account.

Until then, Agentic Trading can assist a human researcher. It cannot silently inherit the authority
of the durable order gateway.

## Official contracts

- [Robinhood Crypto Trading API](https://docs.robinhood.com/)
- [Agentic Trading overview](https://robinhood.com/us/en/support/articles/agentic-trading-overview/)
- [Trading with your agent](https://robinhood.com/us/en/support/articles/trading-with-your-agent/)
- [Third-party connections](https://robinhood.com/us/en/support/articles/third-party-connections/)
