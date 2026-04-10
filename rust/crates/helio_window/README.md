# helio_window

**Operational** windowing on top of [`helio_scan`]: ring buffers, aggregators, and scans (`RollingWindowScan`, `SessionWindowScan`, `ForwardHorizonScan`, …). Semantic frequency and bounds live in **`helio_time`**.

## Sample-driven vs time-keyed vs session-keyed

| Mechanism | Eviction / closure rule | Notes |
|-----------|-------------------------|--------|
| **`WindowState`**, **`RollingAggregatorScan`**, **`RollingFoldScan`**, **`rolling_mean_scan`** | **Sample count** (`WindowSpec` with `Frequency::Samples` via `sample_capacity()`) | Default rolling path |
| **`SessionWindowScan`** | **Session** boundaries (`FlushReason::SessionClose`, bar session ids) | Session-keyed, not arbitrary wall-clock |
| **`TimeBucketAggregatorScan<T>`** (`signal_pipeline`) | **`T: TimeBucketSample`** (`time_ns`, `mean_sample`); **wall-clock** bucket width in ns; emits **on rollover** or `flush` | Default tick: [`PriceTick`](crate::PriceTick). Compose with `Then` + `Arr` + `EmaScan` + `SequentialDiffScan::<f64>` — see `tests/tick_bucket_ema_pipeline.rs` |
| **Fixed-time / calendar window expiry** | Not implemented as automatic buffer eviction yet | `WindowSpec` may still *describe* fixed/calendar semantics for config-forward APIs; operationally, treat as future work unless a scan explicitly implements it |

If you need **time-keyed** expiry (wall clock, business calendar, watermarks), that belongs in new or extended scans — not inferred from `WindowSpec` alone.

## Fold vs incremental summaries

- **`WindowState` + `EvictingWindowAggregator`** — incremental insert/evict summaries.
- **`FoldWindowState`** — **O(window)** fold over the full buffer on each emit when the summary is not incrementally evictable.

## Docs

[docs/TIME_AND_WINDOWS.md](../../../docs/TIME_AND_WINDOWS.md), [docs/HELIO_RUST_WORKSPACE.md](../../../docs/HELIO_RUST_WORKSPACE.md).
