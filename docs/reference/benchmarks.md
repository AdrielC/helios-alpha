# Benchmarks

Criterion benchmarks live in `rust/crates/helio_bench`. The crate is excluded from default workspace members so a normal test run stays focused.

```bash
cd rust
cargo bench -p helio_bench
```

## Current local signal

The following figures are development-machine observations from the August 2026 hardening run. They are not CI budgets and will vary by CPU, compiler, and power state.

| Workload | Observed local throughput |
|---|---:|
| Welford moments update | about 108 million samples/s |
| Online covariance update | about 131 to 137 million pairs/s |
| Rolling moments | about 49 to 51 million samples/s |
| Allocation-free `Then` composition | about 1.87 billion items/s |
| Event vertical fixture | about 5.3 ms per workload |

## How to make a benchmark decision-worthy

Record:

- Rust toolchain and optimization profile.
- CPU model, core pinning, and power mode.
- Input distribution and batch size.
- Allocation count or bytes where relevant.
- Warm-up and sample configuration.
- Baseline commit and regression threshold.
- Whether the workload includes serialization, storage, or transport.

Microbenchmarks prove local mechanical cost. They do not prove end-to-end trading latency or capacity.

See [Event-shock benchmarks](../EVENT_SHOCK_BENCHMARKS) for the vertical workload and current manual thresholds.
