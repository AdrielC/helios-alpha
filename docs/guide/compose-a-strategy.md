# Compose a strategy

The substrate should not know that a value is a price, that a threshold means buy, or that an event came from a market. Those are injected decisions. The reusable core owns ordering, state transitions, control boundaries, and recovery.

## Start with the event contract

Give an observation its event-time coordinate and implement the bucket-time and watermark-time traits:

```rust
use helio_window::{BucketTimed, WatermarkTime};
use helio_time::SecondWallBucket;

#[derive(Debug, Clone, Copy)]
struct Observation {
    event_time: i64,
    value: f64,
}

impl WatermarkTime for Observation {
    fn event_time(&self) -> i64 {
        self.event_time
    }
}

impl BucketTimed<SecondWallBucket> for Observation {
    fn bucket_time(&self, _grid: &SecondWallBucket) -> i64 {
        self.event_time
    }
}
```

## Inject the reduction policy

`F64MomentsReducer` projects an observation into a floating-point value. `OrderedBucketPipeline` composes bounded event-time reorder with generic bucket reduction.

```rust
use helio_time::SecondWallBucket;
use helio_window::{F64MomentsReducer, OrderedBucketPipeline};

fn project_value(input: &Observation) -> f64 {
    input.value
}

let pipeline = OrderedBucketPipeline::try_new(
    4_096,
    SecondWallBucket::ten_minutes(),
    F64MomentsReducer::new(project_value as fn(&Observation) -> f64),
)?;
```

The reducer is compile-time dependency injection. Its mutable state belongs to the pipeline state and therefore participates in snapshots. Its configuration remains an ordinary Rust value.

## Drive event time explicitly

Input alone cannot prove that a bucket is complete. A watermark can:

```rust
use helio_scan::{FlushReason, FlushableScan, Scan, VecEmitter};

let mut state = pipeline.init();
let mut output = VecEmitter::new();

pipeline.step(&mut state, observation, &mut output);
pipeline.flush(
    &mut state,
    FlushReason::Watermark(watermark),
    &mut output,
);
```

Late arrivals, overflow, invalid watermarks, bucket closes, and reducer errors remain typed outputs. Do not turn these into log lines and silently continue.

## Add signal logic at the edge

Treat a signal as another scan that consumes bucket summaries. Compose it with `Scan::then` when the types line up, or write a small adapter scan when policy needs context.

The important separation is:

```text
generic substrate        research-owned policy
---------------------    -----------------------------
order and watermark  ->  event definition
bucket reduction     ->  feature projection
online state         ->  calibration and threshold
snapshot and offset  ->  strategy identity/fingerprint
```

## Prove equivalence before trusting throughput

For every strategy composition, test the same input through:

1. One-item incremental stepping.
2. Opaque batch adapters.
3. Checkpoint, restore, and resume from the recorded offset.
4. End-of-input and watermark flushes.

The output sequence should match exactly for the same ordering and merge tree. Floating-point partitioning and merge order belong in the pipeline fingerprint.
