# Time, frequency, and windows (helio_time + helio_window)

This document is the **agent-facing** map for the semantic vs operational split.

## Crate boundaries

| Concern | Crate |
|---------|--------|
| Scan algebra (no domain) | `helio_scan` |
| Stable online/parallel statistics | `helio_stats` |
| **What** a window/bucket means (frequency, bounds, alignment) | `helio_time` |
| **How** to hold data and aggregate (ring buffer, traits, scans) | `helio_window` |

Do not collapse these into one blob.

## Defaults

- **Interval membership:** `Bounds::LEFT_CLOSED_RIGHT_OPEN` — `[start, end)` everywhere unless the caller sets another `Bounds`.
- **Causality:** a bucket covering `[t, t+Δ)` is **not** the same as **when** that bucket may be used. Always carry `AvailableAt` / `Timed<T>`. Helpers: `helio_time::availability::available_at_bucket_close`, `BucketTimed`.

## Runtime frequency (`helio_time`)

`Frequency` keeps **semantic categories** separate:

- `Frequency::Samples`, `Frequency::Fixed`, `Frequency::Calendar`, `Frequency::Session`

Three calendar days, three fixed 24h steps, and three sessions are **not** interchangeable.

`BucketSpec` = `freq` + `bounds` + `Anchor`.  
`WindowSpec` = trailing / leading / centered + `size: Frequency` + `bounds`.  
`WindowSpec::sample_capacity()` returns `Some(n)` only for sample-count windows — used by ring-buffer scans today.

### Wall-clock bucket width (`WallBucketGrid`)

For new pipelines, use `helio_window::BucketReduceScan<G, V, R>` with a `WallBucketGrid`, `BucketTimed` input, and injected `BucketReducer`. `F64MomentsReducer` supplies stable count/mean/variance without baking price semantics into the scan. It rejects regressed input and non-finite projected values explicitly; checkpoint flushes preserve partial buckets.

When a source can arrive out of order, use `EventTimeReorderScan` or the combined `OrderedBucketPipeline`. The reorder state is capacity-bounded, emits ready values in `(event_time, arrival_sequence)` order, and returns late/overflow records as typed outputs. Its `FallibleRestoreScan` implementation rejects over-capacity, duplicate, mistimed, or watermark-inconsistent snapshots. A watermark first drains ordered input into the bucket reducer and then closes buckets whose right edge is at or before that watermark.

`TimeBucketAggregatorScan<G, V>` remains as a legacy mean-only ordered-input adapter. It no longer closes partial buckets on checkpoint or rebalance.

### Do not confuse spec with eviction

| You write in `WindowSpec` | Operational path in `helio_window` |
|---------------------------|--------------------------------------|
| `Frequency::Samples(n)` | **`WindowState` / `RollingAggregatorScan`** — capacity `n`, FIFO by sample |
| `Frequency::Fixed` on **`Trailing`** | **`time_keyed::TimeKeyedWindowState`** — eviction by wall span \([t-\Delta,t)\) on caller-provided keys |
| `SessionStep` / session counts | **`session_keyed::SessionKeyedRollingState`** — trailing *n* **trading** sessions via `TradingCalendar` |
| `Frequency::Calendar` | **Not** auto-wired to a generic ring buffer — build or extend a scan with your calendar provider |

Session-**bar** batching still uses `SessionWindowScan` + flush signals (`SessionClose`, `EndOfInput`), not `WindowSpec` alone.

### Bucket interval vs availability

Use **`helio_time::availability`**: `wall_bucket_interval_wall_secs`, `bucket_close_instant`, `available_at_bucket_close` so **bucket close** and **availability** stay explicit for causal correctness.

## Typed frequency (optional, `helio_time::typed_freq`)

Additive helpers: `Samples<N>`, `Fixed<N, Days>`, `Sessions<N>`, etc. They convert to `Frequency` for APIs that expect runtime specs.

## Rolling operations (`helio_window`)

- `WindowBuffer<T>` — FIFO ring (evict from front).
- `WindowAggregator<T>`, `EvictingWindowAggregator<T>` — insert vs insert+evict.
- `SumCountMeanAggregator` — eviction-aware sum/count/mean.
- `RollingMomentsAggregator` / `rolling_moments_scan`: stable rolling mean/variance/stddev with explicit non-finite and numerical-rejection counts.
- `WindowState<T, A>` — `WindowSpec` + buffer + evicting aggregator (sample-count trailing).
- `FoldWindowState` — O(window) fold on each emit (generic summaries).
- `RollingAggregatorScan`, `RollingFoldScan`, `rolling_mean_scan` — emit when buffer is full.

## Mergeable statistics (`helio_stats`)

`OnlineMoments` stores `(count, mean, M2)` and supports Welford updates, rolling removal, and Chan-style partial-state merges. `merge_moments_balanced` fixes a deterministic balanced merge tree for replay. Floating-point merges are associative only in exact arithmetic, so production partitioning and merge order must remain part of the pipeline fingerprint.

`OnlineCovariance` provides mergeable marginal variances, covariance, and correlation.
`CompensatedSum`, `ScaledSumSquares`, and `LogProbability` cover cancellation, stable norms, and
conditional probability underflow. Their scan adapters, `OnlineMomentsScan`,
`OnlineCovarianceScan`, and `HawkesScan` expose guarded state through the standard `Scan` and
snapshot interfaces and reject invalid external snapshots through `FallibleRestoreScan`.

## Forward horizon

`ForwardHorizonScan` carries a documentary `WindowSpec` (default `trailing_samples(1)`). Bar-count horizons align with `Frequency::Samples`; wiring it to **time-keyed** or **session-keyed** eviction is still optional / workload-specific.

## Non-goals (this phase)

Finger trees, full timezone/calendar resolution in-crate, Arrow, async runtimes, proc-macro frequency DSL.
