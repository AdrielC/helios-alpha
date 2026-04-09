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

## Toolchain note

`criterion` is pinned to **0.4.0** so dependency resolution stays compatible with older Cargo (Criterion 0.5.1+ can pull `clap` builds that require a newer toolchain).
