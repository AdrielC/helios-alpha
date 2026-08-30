# Build a restartable 10-minute event signal

This walkthrough turns one research contract into a typed Rust pipeline. A value enters a
10-minute bucket, stable online moments summarize the closed bucket, and injected research policy
may emit a candidate signal.

The reusable substrate owns causality, ordering, state, and recovery. It does not know what a price is, what `2.0σ` means, or whether an output should become an order.

## 1. State the research contract

For this example:

| Contract field | Decision |
|---|---|
| Observation | A timestamped floating-point value |
| Event coordinate | Unix seconds in `event_time` |
| Availability gate | The observation must be available by the decision cut |
| Disorder budget | At most 4,096 pending observations |
| Feature | Count, mean, and variance per 10-minute bucket |
| Emission boundary | The watermark passes the bucket end |
| Strategy output | A candidate signal owned by research policy |
| Recovery identity | Snapshot version, pipeline fingerprint, source offset, and watermark |

This contract is part of the strategy. Changing the bucket grid, disorder capacity, projection, merge order, or decision rule creates a different computation and should change its fingerprint.

## 2. Give the observation both clocks

`event_time` determines ordering and bucket membership. `available_at` determines whether the observation was knowable at the decision cut.

The value that reaches `OrderedBucketPipeline` implements the event-time traits:

```rust
use serde::{Deserialize, Serialize};
use helio_time::SecondWallBucket;
use helio_window::{BucketTimed, WatermarkTime};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
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

Wrap ingress values in `helio_time::Timed<Observation>` when availability differs from event time. `AvailabilityGateScan` passes only values whose `available_at` is less than or equal to the configured cut:

```rust
use helio_scan::{Scan, VecEmitter};
use helio_time::{AvailabilityGateScan, AvailableAt, Timed};

let decision_cut = AvailableAt(1_706_704_782);
let gate = AvailabilityGateScan::<Observation>::new(Some(decision_cut));
let mut gate_state = gate.init();
let mut eligible = VecEmitter::new();

gate.step(
    &mut gate_state,
    Timed::new(observation, AvailableAt(published_at)),
    &mut eligible,
);
```

At the ingress boundary, pass only the eligible inner observations into the ordered bucket pipeline. Keep the source offset attached to the same admitted record so a checkpoint can resume from an exact position.

## 3. Inject the 10-minute reduction

`F64MomentsReducer` receives a projection function. `OrderedBucketPipeline` combines a bounded event-time reorder stage with generic bucket reduction.

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

The projection is compile-time dependency injection. Its mutable accumulator belongs to pipeline state, so it participates in snapshots. Its configuration remains an ordinary Rust value, and the hot path does not require virtual dispatch.

The moments reducer tracks `n`, mean, and `M2`. It uses Welford updates for individual observations and Chan-style merges for partitions. A fixed merge tree is required when bitwise replay matters because floating-point addition is order-sensitive.

## 4. Advance event time explicitly

Receiving an input does not prove that its bucket is complete. A watermark does.

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

For bucket `[09:30, 09:40)`, the summary is safe to finalize only when the watermark reaches the close boundary under the configured semantics. Until then, the bucket remains owned state.

The pipeline emits typed `OrderedBucketOutput` values. Ready data flows into the reducer. Late arrivals, overflow, sequence exhaustion, invalid watermarks, bucket closes, and reducer failures remain explicit outcomes. A caller must handle them instead of converting them into log-only side effects.

## 5. Put signal policy after the closed summary

A strategy rule consumes the closed bucket summary and emits zero or more candidate signals. It can be another `Scan`, composed with `Scan::then` when the types line up, or a small adapter that preserves the typed control outcomes.

Keep the ownership boundary explicit:

| Generic substrate owns | Research policy owns |
|---|---|
| Availability and ordering contracts | Event definition and inclusion criteria |
| Watermark and bucket closure | Feature projection and calibration window |
| Online state and typed failures | Threshold, direction, horizon, and controls |
| Snapshot and restore mechanics | Strategy identity and decision semantics |

A candidate signal is not an authorized trade. Capital allocation, broker routing, portfolio constraints, and kill switches sit beyond this pipeline.

## 6. Make recovery part of the result

A usable checkpoint binds four things:

1. The versioned operator snapshot.
2. The source offset represented by that snapshot.
3. The latest accepted watermark or equivalent control frontier.
4. A fingerprint of every configuration choice that changes results.

On restart, validate the snapshot and fingerprint before resuming after the recorded offset. Commit downstream effects before advancing the durable source position, or use an atomic protocol that coordinates both.

See [Restart a pipeline](./restart-a-pipeline) for the full commit order and compatibility contract.

## 7. Require equivalence before trusting throughput

Run the same ordered input through all of these paths:

1. One observation at a time.
2. The opaque batch adapters.
3. Checkpoint, restore, and resume after the recorded offset.
4. Watermark and end-of-input flushes.
5. Partitioned moments merged with the declared fixed tree.

For the same input order and merge tree, the output sequence should match exactly. Include late, overflow, empty-bucket, invalid-watermark, corrupt-snapshot, and incompatible-fingerprint cases.

## What this walkthrough proves

It proves that the mechanics can be represented as bounded, typed, replayable state transitions. It does not prove that the event definition predicts returns, survives costs, generalizes out of sample, or can safely route live orders.

Next, apply the [rare-event evidence standard](../research/evidence-standard) and audit the [production boundary](../operations/production-readiness).
