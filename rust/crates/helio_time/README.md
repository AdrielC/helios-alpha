# helio_time

**Semantic** time layer: frequency, interval bounds, bucket/window specs, and causality helpers (`Timed`, `AvailableAt`). Pair with **`helio_window`** for ring buffers and aggregators.

## Semantic vs operational (read this before using `WindowSpec`)

| Layer | What it means | Enforced by |
|-------|----------------|-------------|
| **Semantic** | `Frequency`, `Bounds`, `BucketSpec`, `WindowSpec`, `Anchor`, `typed_freq` — *what* a window or bucket means in domain language | Types and docs in **this crate** only |
| **Operational** | Sample-count FIFO eviction, session-bar scans, watermark hooks — *how* data is actually dropped or finalized | **`helio_window`** (and callers) |

**Critical:** `WindowSpec` can describe `Frequency::Fixed`, `Calendar`, or `Session`, but **`WindowSpec::sample_capacity()` is only `Some` for `Frequency::Samples`**. Ring-buffer paths (`WindowState`, `RollingAggregatorScan`, etc.) are **sample-count-driven** today. Do not assume fixed-time or session-keyed expiry is implemented just because the spec type allows it — see `helio_window` and [TIME_AND_WINDOWS.md](../../../docs/TIME_AND_WINDOWS.md).

## Defaults

- **Intervals:** [`Bounds::LEFT_CLOSED_RIGHT_OPEN`] — `[start, end)`.
- **Causality:** bucket/event interval ≠ [`AvailableAt`]. See [`availability`](src/availability.rs) and `available_at_bucket_close`.

## Runtime vs typed frequency

- **Runtime / config:** [`Frequency`], [`BucketSpec`], [`WindowSpec`] (serde).
- **Static (optional):** [`typed_freq`](src/typed_freq.rs) — `Samples<N>`, `Fixed<N, Days>`, ….

## Docs

Repository overview: [docs/HELIO_RUST_WORKSPACE.md](../../../docs/HELIO_RUST_WORKSPACE.md), [docs/TIME_AND_WINDOWS.md](../../../docs/TIME_AND_WINDOWS.md).
