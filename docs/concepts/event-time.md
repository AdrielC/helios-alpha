# Event time, watermarks, and windows

Three times often coexist in a streaming strategy:

| Time | Meaning |
|---|---|
| Event time | When the underlying observation occurred |
| Availability time | When the strategy was allowed to know it |
| Processing time | When this machine happened to receive it |

Conflating them creates lookahead and nondeterministic replay.

## Half-open buckets

Helios uses half-open intervals by default: `[start, end)`. An event at exactly `end` belongs to the next bucket.

Bucket membership does not imply availability. A ten-minute bucket cannot normally be consumed before its right edge is known to be complete.

## Watermarks

A watermark `W` asserts that the source does not expect new on-time input at or before `W`. `EventTimeReorderScan` drains ready values in `(event_time, arrival_sequence)` order. `OrderedBucketPipeline` then closes buckets whose exclusive end is at or before the watermark.

Capacity is explicit. When pending input exceeds its bound, overflow is a typed result, not an unbounded-memory surprise.

## Window semantics stay separate from storage

`helio_time` describes what a frequency or window means. `helio_window` implements particular storage and eviction mechanics.

- `Frequency::Samples(n)` maps cleanly to a fixed-capacity FIFO ring.
- Fixed trailing time windows use caller-provided time keys.
- Session windows require a trading-calendar provider.
- Calendar frequencies are not silently treated as fixed seconds.

Read [Time and windows](../TIME_AND_WINDOWS) for the detailed support matrix.
