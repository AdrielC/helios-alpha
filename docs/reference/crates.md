# Crate map

| Crate | Layer | Owns | Does not own |
|---|---|---|---|
| `helio_scan` | Kernel | State machines, emit sinks, controls, composition, persistence seams | Markets, bars, sessions, transports |
| `helio_stats` | Statistics | Moments, covariance, deterministic merge utility, Hawkes state | Parameter fitting, alpha claims |
| `helio_time` | Semantics | Frequencies, interval bounds, bucket grids, availability | Operational buffers |
| `helio_window` | Operations | Reorder, bucket reduction, rolling and session state | Strategy vocabulary |
| `helio_event` | Proving ground | Event-shock model, filters, signal and simulated execution | Broker authorization |
| `helio_backtest` | Harness | Clocks, fingerprints, Kalman research, terminal UI | Live execution guarantees |
| `helio_backtest_wasm` | Browser adapter | Browser-hosted backtest interface | Core numerical semantics |
| `helios_signald` | Integration | Optional ZMQ signal bridge | Kernel abstractions |
| `helio_bench` | Tooling | Criterion workloads and baselines | Runtime dependencies |

## Dependency direction

```text
helio_scan        helio_time
     ↑                ↑
     └──── helio_stats
     ↑                ↑
     └──── helio_window
               ↑
          helio_event
```

Application crates may depend on substrate crates. Substrate crates do not depend on event-shock or trading types.

For the detailed workspace notes, read [Rust workspace](../HELIO_RUST_WORKSPACE) and [Public API surface](../PUBLIC_API_SURFACE).
