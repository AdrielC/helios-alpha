# Rust workspace (helio scan stack)

Cargo workspace root: **`rust/Cargo.toml`**. Crates live under **`rust/crates/`**.

## Crates

| Crate | Role |
|-------|------|
| **`helio_scan`** | Cold kernel: `Scan`, `FlushableScan`, `SnapshottingScan`, combinators, checkpoint seam. **No bars, sessions, or market types.** |
| **`helio_time`** | **Semantics:** `Frequency`, `Bounds` (default `[start,end)`), `BucketSpec`, `WindowSpec`, `Anchor`, `TimeWindow`; `Timed<T>` / `AvailableAt`; optional `typed_freq`; `AvailabilityGateScan`, `SessionAlignScan`. |
| **`helio_window`** | **Operations:** `WindowBuffer`, `WindowAggregator` / `EvictingWindowAggregator`, `WindowState`, `FoldWindowState`; scans (`RollingWindowScan`, `RollingAggregatorScan`, `RollingFoldScan`, `SessionWindowScan`, `ForwardHorizonScan`, …). |
| **`helio_event`** | Domain proving ground: classic event-study (`TreatmentEvent`, `CausalEventStudyPipeline`, `EventStudyFoldScan`) **and** generic **event-shock vertical** (`EventShock`, `EventShockVerticalScan`, `replay_event_shock` binary). Integration tests: **`replay_determinism`**, **`event_shock_vertical_determinism`**, **`solar_demo_smoke`**. May split later (generic machinery vs analysis) if it grows. |
| **`helios_signald`** | ZMQ subscriber binary (system `libzmq` required). |
| **`helio_bench`** | Criterion benchmarks (not in `default-members`; run with `cargo bench -p helio_bench`). |

## Default members

`default-members` includes **`helio_scan`**, **`helio_time`**, **`helio_window`**, **`helio_event`** so `cargo test` in `rust/` does not build ZMQ. Build the daemon explicitly:

```bash
cd rust
cargo build --release -p helios_signald
```

## Benchmarks

```bash
cd rust
cargo bench -p helio_bench
# CI-style compile check:
cargo bench -p helio_bench --no-run
```

See `rust/crates/helio_bench/README.md`. Event-shock vertical baselines and manual thresholds: [EVENT_SHOCK_BENCHMARKS.md](EVENT_SHOCK_BENCHMARKS.md).

## Solar / normalized event-shock demo (CLI)

End-to-end path: load events + daily bars → vertical pipeline → `trades.csv`, `summary.csv`, `report.md`.

**Solar-normalized CSV** columns: `id,available_at,impact_start,impact_end,severity,confidence` (maps to `EventKind::Solar` via `load_solar_event_shocks_csv`).

```bash
cd rust
cargo run -p helio_event --bin replay_event_shock -- \
  --events fixtures/event_shock/solar_events.csv \
  --bars fixtures/event_shock/bars.csv \
  --events-format solar \
  --out /tmp/event_shock_demo
```

Generic events (with `kind` / `scope`): use `--events-format csv` and `fixtures/event_shock/events.csv`.

## Golden-path replay tests

`helio_event/tests/replay_determinism.rs` exercises:

- Mid-stream **snapshot** + **restore** vs uninterrupted run (**identical** `ForwardHorizonOutput` sequence).
- **`Persisted`** checkpoint vs manual snapshot equality.
- **`FlushReason::SessionClose`** → **`ForwardHorizonOutput::Incomplete`** without updating the complete-only fold.
- **`EventStudyFoldScan`** emission count vs completed horizons.

## Causal pipeline semantics

`CausalEventStudyPipeline` applies:

1. **`TreatmentSelectorScan`** — availability gate on `Timed<TreatmentEvent>`.
2. **`ForwardHorizonScan`** — immediate horizon spawn on each selected treatment (next bar attaches).
3. **`EventClusterScan`** — runs **in parallel** on the same selected treatments for snapshot/diagnostics; **cluster outputs on `step` are discarded** so labeling is not blocked until a cluster closes. Use `ClusteredTreatmentScan` + `ClusterToHorizonScan` directly if you need **cluster-finalized** horizon ids.

## Roadmap (substrate is wide enough)

1. **One flagship workload** — replayable causal **event-shock** (or event-study) backtest using all of `helio_scan`, `helio_time`, `helio_window`, `helio_event`: checkpoint/restore, rolling/session logic, clustering, complete vs incomplete outcomes, deterministic reproduction.
2. **Time-keyed expiry for real** — operational support for fixed-time and session-*extent* window eviction (today many rolling paths are **sample-count-only**; `WindowSpec` can describe more than what ring buffers enforce).
3. **Benchmark budgets** — turn `helio_bench` numbers into documented thresholds or CI smoke checks so regressions are visible (see `rust/crates/helio_bench/README.md`).

## Further reading

- [HELIO_SCAN.md](HELIO_SCAN.md) — kernel design.
- [TIME_AND_WINDOWS.md](TIME_AND_WINDOWS.md) — frequency, bounds, buffers, aggregators.
