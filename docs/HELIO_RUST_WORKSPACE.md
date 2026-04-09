# Rust workspace (helio scan stack)

Cargo workspace root: **`rust/Cargo.toml`**. Crates live under **`rust/crates/`**.

## Crates

| Crate | Role |
|-------|------|
| **`helio_scan`** | Cold kernel: `Scan`, `FlushableScan`, `SnapshottingScan`, combinators, checkpoint seam. **No bars, sessions, or market types.** |
| **`helio_time`** | Causal time contracts: `Timed<T>`, `AvailableAt`, `ObservedAt`, `EffectiveAt`; scans `AvailabilityGateScan`, `SessionAlignScan`. |
| **`helio_window`** | Reusable windowing: `RollingWindowScan`, `SessionWindowScan`, `ForwardHorizonScan`, `EventClusterScan`, `WatermarkFinalizeScan`, `LagScan`, `DedupScan`, `JoinLatestScan`. |
| **`helio_event`** | Event-study domain: `TreatmentEvent`, `ControlEvent`, `CausalEventStudyPipeline`, `MatchedControlSampler`, `EventStudyFoldScan`; integration test **`replay_determinism`**. |
| **`helios_signald`** | ZMQ subscriber binary (system `libzmq` required). |

## Default members

`default-members` includes **`helio_scan`**, **`helio_time`**, **`helio_window`**, **`helio_event`** so `cargo test` in `rust/` does not build ZMQ. Build the daemon explicitly:

```bash
cd rust
cargo build --release -p helios_signald
```

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

## Further reading

- [HELIO_SCAN.md](HELIO_SCAN.md) — kernel design.
