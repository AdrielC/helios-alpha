# Time, frequency, and windows (helio_time + helio_window)

This document is the **agent-facing** map for the semantic vs operational split.

## Crate boundaries

| Concern | Crate |
|---------|--------|
| Scan algebra (no domain) | `helio_scan` |
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

### Do not confuse spec with eviction

| You write in `WindowSpec` | Ring buffer / `WindowState` today |
|---------------------------|-----------------------------------|
| `Frequency::Samples(n)` | **Yes** — capacity `n`, FIFO by sample |
| `Frequency::Fixed` / `Calendar` / `Session` (extent semantics) | **Not** automatic time-keyed eviction in `WindowState` / default rolling scans — semantics are **documented intent** until `helio_window` grows matching machinery |

Session-**bar** behavior uses dedicated scans (e.g. `SessionWindowScan`) and flush signals, not `WindowSpec` alone.

## Typed frequency (optional, `helio_time::typed_freq`)

Additive helpers: `Samples<N>`, `Fixed<N, Days>`, `Sessions<N>`, etc. They convert to `Frequency` for APIs that expect runtime specs.

## Rolling operations (`helio_window`)

- `WindowBuffer<T>` — FIFO ring (evict from front).
- `WindowAggregator<T>`, `EvictingWindowAggregator<T>` — insert vs insert+evict.
- `SumCountMeanAggregator` — eviction-aware sum/count/mean.
- `WindowState<T, A>` — `WindowSpec` + buffer + evicting aggregator (sample-count trailing).
- `FoldWindowState` — O(window) fold on each emit (generic summaries).
- `RollingAggregatorScan`, `RollingFoldScan`, `rolling_mean_scan` — emit when buffer is full.

## Forward horizon

`ForwardHorizonScan` carries a documentary `WindowSpec` (default `trailing_samples(1)`). Bar-count horizons align with `Frequency::Samples`; **time-keyed** eviction (fixed/calendar/session extents) is future work.

## Non-goals (this phase)

Finger trees, full timezone/calendar resolution in-crate, Arrow, async runtimes, proc-macro frequency DSL.
