# helio_hypothesis

`helio_hypothesis` is a generic, keyed runtime for conditional inference that unfolds over time.
Each key owns independent model state, an exact evidence sequence, a revision, and a bounded set of
availability-time deadlines. The runtime owns lifecycle and recovery. An injected
`HypothesisModel` owns domain meaning.

The crate contains no trading, astronomy, sensor, or infrastructure vocabulary. Those concepts
belong in model implementations above this layer.

## What the runtime guarantees

- Every accepted transition is atomic. Invalid model state or effects leave live state unchanged.
- Evidence records both when a phenomenon was effective and when the system could first use it.
- Per-key sequence numbers have no gaps. Revisions cover evidence, timers, and lifecycle changes.
- Timers fire deterministically by `(available_at, key, timer_id)`.
- Evidence cannot jump past an unfired deadline. Advance the frontier first.
- Active keys, terminal tombstones, timers, effects, and fires per frontier advance are bounded.
- Supersession closes one key and opens its replacement as one transition.
- Snapshots exclude derived timer indexes. `try_restore` rebuilds them and validates every invariant.

## Inject a model

```rust
use helio_hypothesis::{
    CausalEvidence, HypothesisModel, HypothesisTransition, TimerId,
};
use helio_time::AvailableAt;

struct Model;

impl HypothesisModel<String> for Model {
    type Evidence = Evidence;
    type State = State;
    type Output = Action;
    type Error = ModelError;

    fn open(
        &self,
        key: &String,
        evidence: CausalEvidence<Self::Evidence>,
    ) -> Result<HypothesisTransition<Self::State, Self::Output>, Self::Error> {
        // Validate the trigger, initialize state, request follow-up work,
        // and schedule a deadline as one proposed transition.
        todo!()
    }

    fn update(
        &self,
        key: &String,
        state: &Self::State,
        evidence: CausalEvidence<Self::Evidence>,
    ) -> Result<HypothesisTransition<Self::State, Self::Output>, Self::Error> {
        todo!()
    }

    fn on_timer(
        &self,
        key: &String,
        state: &Self::State,
        timer_id: TimerId,
        at: AvailableAt,
    ) -> Result<HypothesisTransition<Self::State, Self::Output>, Self::Error> {
        todo!()
    }

    fn validate(&self, key: &String, state: &Self::State) -> Result<(), Self::Error> {
        todo!()
    }
}
# enum Evidence {}
# struct State;
# enum Action {}
# enum ModelError {}
```

External physics, ML, enrichment, or venue calls should be emitted as typed `Output` values. Their
responses return later as `Evidence`. The model transition stays pure and fast, and the runtime
never holds a lock across external I/O.

Run the complete illustrative chain:

```bash
cargo run -p helio_hypothesis --example conditional_shock
```

The example is hypothetical and demonstrates control flow, not scientific calibration.

## Typed service injection

Rust does not need a reflection container to get ZIO-style typed capabilities. Inject narrow
traits through constructors and let ownership define the resource scope.

| Need | Preferred shape |
|---|---|
| One partition or worker | `HypothesisEngine<K, Model, Reason>` owned by that worker |
| Many async callers | cloneable `HypothesisServiceHandle` backed by one bounded actor |
| Existing shared application context | `SharedHypothesisEngine` with `Arc<tokio::sync::Mutex<_>>` |
| Test double | an ordinary type implementing `HypothesisService` |

Enable the adapters with `features = ["service"]`. The actor is preferred for shared access: one
task owns mutable state, the mailbox applies backpressure, and dropping the owned task aborts it.
The mutex adapter exists for integration, but holds its lock only for the pure in-memory
transition.

`SnapshottingHypothesisService::process_and_snapshot` returns events and the exact
post-transition snapshot from one serialized command. A durable driver still must atomically store
that snapshot with its source position and output outbox before acknowledging the source.

## Performance contract

The direct `Scan` path uses inline `SmallVec` event buffers for common transitions and an injected
`Emit` sink, so it does not require a heap allocation for each observation. Models borrow current
state and return an owned next state, so normal evidence updates do not clone model state inside the
runtime. State is stored in ordered maps for deterministic replay. Frontier advances with no due
timer update in place and do not clone active hypotheses. A frontier with due timers stages a
bounded copy so a failed timer transition can roll back the entire advance.

Measure with:

```bash
cargo bench -p helio_bench --bench hypothesis_machine -- --noplot
```

Use real key, state, output, checkpoint, and sink sizes before setting a latency budget.

## Explicit boundaries

This crate does not provide distributed sharding, a durable source transaction, a durable outbox,
broker connectivity, risk authorization, or exactly-once effects. It provides the deterministic
state machine that those systems can drive and persist.
