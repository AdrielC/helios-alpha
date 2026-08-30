# Crate map

| Crate | Layer | Owns | Does not own |
|---|---|---|---|
| `helio_scan` | Kernel | State machines, emit sinks, controls, composition, persistence seams | Markets, bars, sessions, transports |
| `helio_hypothesis` | Conditional runtime | Keyed lifecycle, typed model transitions, deadlines, supersession, bounded state, snapshots, actor service | Domain models, durable transactions, execution authority |
| `helio_golem` | Durable adapter kernel | Atomic offset batches, invocation identities, shard snapshots | Golem SDK types, domain evidence, cloud deployment |
| `helio_stats` | Statistics | Stable moments, compensated sums, scaled norms, log probabilities, Bayesian state, Thompson decisions, Hawkes state | Hierarchical fitting, objectives, constraints, alpha claims |
| `helio_time` | Semantics | Frequencies, interval bounds, bucket grids, availability | Operational buffers |
| `helio_window` | Operations | Reorder, bucket reduction, rolling and session state | Strategy vocabulary |
| `helio_event` | Proving ground | Event-shock model, filters, signal and simulated execution | Broker authorization |
| `helio_execution` | Capital controls | Fixed-point orders, risk reservations, costs and capacity, broker reconciliation, operational readiness, incidents, capital admission | Signal research, credentials, production evidence |
| `helio_backtest` | Harness | Clocks, fingerprints, guarded Kalman research, terminal UI | Live execution guarantees |
| `helio_backtest_wasm` | Browser adapter | Browser-hosted backtest interface | Core numerical semantics |
| `helios_signald` | Integration | Optional ZMQ signal bridge | Kernel abstractions |
| `helio_bench` | Tooling | Criterion workloads and baselines | Runtime dependencies |
| `helios_hypothesis_shard` | Golem application | Agent schema, periodic snapshots, event-shock reference model | Feed ingestion, risk authority, broker access |

## Dependency direction

```text
helio_scan        helio_time
     ↑                ↑
     └──── helio_stats
     ↑                ↑
     └──── helio_window
     ↑
     ├──── helio_hypothesis
     │         ↑       ↑
     │    helio_event  helio_golem
     │
     └──── helio_execution
                           ↑
              helios_hypothesis_shard
```

Application crates may depend on substrate crates. Substrate crates do not depend on event-shock or trading types.

For the detailed workspace notes, read [Rust workspace](../HELIO_RUST_WORKSPACE) and [Public API surface](../PUBLIC_API_SURFACE).
