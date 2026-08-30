# Incident response

The first response to uncertain execution state is to stop new risk. Recovery begins only after
source, state, outbox, risk reservations, and broker orders agree.

## Automatic halt conditions

Activate the kill switch and close capital admission when any of these occur:

- a source offset and checkpoint disagree;
- an output identity is committed with different content;
- a broker acknowledgement cannot be reconciled by client order identity;
- market data is stale or the clock offset exceeds policy;
- venue-calendar coverage is missing or its digest fails;
- fixed-point cost or notional arithmetic overflows;
- position, exposure, capacity, or order-count limits are breached;
- an operational invariant alert fires; or
- the on-call operator cannot determine current broker exposure.

## Severity

| Severity | Definition | Initial action |
|---|---|---|
| SEV-1 | Unknown or unintended live exposure, duplicate-order risk, or failed kill switch | Stop new orders, notify broker and incident commander, reconcile immediately |
| SEV-2 | Production control degraded with exposure still known | Close admission, isolate the component, begin bounded recovery |
| SEV-3 | Research, shadow, or non-capital degradation | Preserve evidence, repair during the declared response window |

## Recovery sequence

1. Open an incident with one immutable identity and timestamp.
2. Activate the independent kill switch. Do not rely on stopping the strategy process alone.
3. Freeze deployment and policy changes unless the incident commander records an emergency change.
4. Capture source offsets, checkpoints, outbox rows, risk reservations, gateway journal, broker open
   orders, positions, cash, clock state, calendar version, metrics, and logs.
5. Reconcile every pending `client_order_id` against broker state before any retry or cancellation.
6. Compare the durable source prefix with the checkpoint and output set. Quarantine any identity
   conflict.
7. Correct or roll back the failing component. Restore into paper or shadow mode first.
8. Replay the affected prefix and compare decisions, risk outcomes, and broker simulations.
9. Resolve the incident with the observed cause, scope, corrective action, and retained artifacts.
10. Regenerate every invalidated admission artifact. Reopen live capital only through the ordinary
    admission path.

Acknowledgement is not resolution. A healthy process, empty queue, or successful redeploy is not
proof that exposure is reconciled.

## Required drills

Before capital, rehearse at least:

- crash before and after the atomic commit;
- broker accept followed by timeout;
- broker unavailable before acceptance;
- stale market data and clock skew;
- expired or corrupt venue schedule;
- outbox backlog and mailbox backpressure;
- risk-policy rollback and kill-switch activation;
- Golem worker and server restart; and
- deployment rollback while source events continue.

The exercise artifact must include timestamps, alert delivery, acknowledgement time, recovery time,
every state comparison, and unresolved follow-up work. It expires according to capital policy.
