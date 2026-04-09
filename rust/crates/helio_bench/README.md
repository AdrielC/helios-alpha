# helio_bench

Criterion benchmarks for **`helio_scan`**, **`helio_window`**, and **`helio_event`**. Not a default workspace member: build explicitly so normal `cargo test` stays lean.

## Run

From `rust/`:

```bash
cargo bench -p helio_bench
```

Compile only (CI-friendly):

```bash
cargo bench -p helio_bench --no-run
```

## Suites

| Bench file | Focus |
|------------|--------|
| `rolling_windows.rs` | `rolling_mean_scan`, `RollingWindowScan`, `RollingFoldScan` |
| `scan_kernel.rs` | `Then`, `Map`, `ZipInput` overhead |
| `event_study.rs` | `CausalEventStudyPipeline` over synthetic `ReplayRecord` stream |
| `execution_modes.rs` | single `step` vs `step_batch` vs `run_iter`; rolling `step` vs `step_batch_optimized` |
| `scan_pressure.rs` | emit fan-out (0/1/4/16), deep `Map`/`filter_map`, `Persisted` + checkpoint every 64 |

## Criterion pin (**intentional technical debt**)

`criterion` is pinned to **=0.4.0** in `Cargo.toml` so resolution stays compatible with older Cargo / CI images (Criterion **0.5.1+** can pull `clap` builds that need a newer toolchain).

**This is a visible compromise, not a permanent design choice.** Revisit the pin when CI’s minimum Rust/Cargo moves forward; do not let it fossilize unmentioned.

## Turning results into expectations (next step)

Benchmarks are only useful if regressions hurt. Prefer to add, over time:

- **Smoke budgets** in CI: e.g. `cargo bench -p helio_bench --no-run` (compile gate) plus optional `--bench scan_kernel` with `-- --quick` against stored baselines if you adopt Criterion’s baselines or a small custom threshold harness.
- **Documented targets** in this file (throughput floors or “no more than X× slower than memcpy” style) for: `Then` / `ZipInput` overhead, checkpoint cadence, `step` vs opaque `step_batch`, and rolling hot paths.

Until those exist, treat numbers as exploratory, not release gates.
