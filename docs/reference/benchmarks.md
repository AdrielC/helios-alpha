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
| Neumaier-compensated sum | about 145 million samples/s |
| Scaled sum-of-squares norm | about 127 million samples/s |
| Online covariance update | about 131 to 137 million pairs/s |
| Normal-Inverse-Gamma update | about 105 million samples/s |
| Keyed Gamma-Poisson posterior draw | about 221 ns per draw, or 4.5 million draws/s |
| Keyed hypothesis update, one active key | about 46 ns, or 21.8 million updates/s |
| Keyed hypothesis update, 1,024 active keys | about 66 ns, or 15.1 million updates/s |
| Frontier advance, 4,096 future timers and none due | about 2.9 ns per advance |
| Deadline fire and completion, 4,096 active keys | about 334 ns each, or 3.00 million fires/s |
| OMS submit plus acknowledgement across 10,000 orders | about 392,000 commands/s, or 2.55 µs per command |
| OMS exact fill accounting, 4,096 fills on one order | about 602,000 fills/s, or 1.66 µs per fill |
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

The hypothesis figures came from a Criterion `--quick` release run on an Apple M3 Pro with Rust
1.94. The idle-frontier result isolates the in-place fast path and excludes source, sink, snapshot,
transport, and state teardown costs. Reproduce the suite with:

```bash
cargo bench -p helio_bench --bench hypothesis_machine -- --noplot
cargo bench -p helio_bench --bench oms_lifecycle -- --noplot
```

The OMS figures are from the same Apple M3 Pro class of local machine and use the in-memory
reference implementation. They include fixed-point accounting, identity checks, aggregate
versioning, and event-envelope creation. They exclude Golem persistence, NATS publication, FIX
session I/O, venue latency, and operator projection work.

See [Event-shock benchmarks](../EVENT_SHOCK_BENCHMARKS) for the vertical workload and current manual thresholds.
