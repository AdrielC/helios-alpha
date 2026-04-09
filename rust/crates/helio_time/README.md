# helio_time

**Semantic** time layer: frequency, interval bounds, bucket/window specs, and causality helpers (`Timed`, `AvailableAt`). Pair with **`helio_window`** for ring buffers and aggregators.

## Defaults

- **Intervals:** [`Bounds::LEFT_CLOSED_RIGHT_OPEN`] — `[start, end)`.
- **Causality:** bucket/event interval ≠ [`AvailableAt`]. See [`availability`](src/availability.rs) and `available_at_bucket_close`.

## Runtime vs typed frequency

- **Runtime / config:** [`Frequency`], [`BucketSpec`], [`WindowSpec`] (serde).
- **Static (optional):** [`typed_freq`](src/typed_freq.rs) — `Samples<N>`, `Fixed<N, Days>`, `Sessions<N>`.

## Docs

Repository overview: [docs/HELIO_RUST_WORKSPACE.md](../../../docs/HELIO_RUST_WORKSPACE.md).
