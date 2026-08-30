# Keyed hypothesis machines

A stream operator answers one repeated question. A hypothesis machine manages a question whose
answer unfolds through conditional evidence: a trigger opens an incident, follow-up models refine
it, a deadline may expire it, and the result may complete, retract, or supersede the incident.

The key is the unit of causal state. It might identify an event cluster, an instrument-specific
interpretation, an experiment arm, or a physical incident. The runtime does not need to know which.

<HypothesisAtlas />

## The division of responsibility

`KeyedHypothesisMachine<K, Model, Reason>` owns the mechanics:

- keyed lifecycle and exact evidence sequences
- atomic transition validation
- availability-time deadlines
- deterministic output order
- active and terminal capacity bounds
- supersession, retraction, closure, and completion
- snapshots and fallible, fail-closed external restore

`HypothesisModel<K>` owns the meaning:

- what evidence is valid at each conditional stage
- what state represents the current belief
- which typed action or research candidate to emit
- which deadline to schedule or cancel
- whether restored model state is internally valid

```rust
pub trait HypothesisModel<K> {
    type Evidence;
    type State: Clone;
    type Output;
    type Error;

    fn open(
        &self,
        key: &K,
        evidence: CausalEvidence<Self::Evidence>,
    ) -> Result<HypothesisTransition<Self::State, Self::Output>, Self::Error>;

    fn update(
        &self,
        key: &K,
        state: &Self::State,
        evidence: CausalEvidence<Self::Evidence>,
    ) -> Result<HypothesisTransition<Self::State, Self::Output>, Self::Error>;

    fn on_timer(
        &self,
        key: &K,
        state: &Self::State,
        timer_id: TimerId,
        at: AvailableAt,
    ) -> Result<HypothesisTransition<Self::State, Self::Output>, Self::Error>;

    fn validate(&self, key: &K, state: &Self::State) -> Result<(), Self::Error>;
}
```

The runtime calls the model synchronously. External inference, physics, data, and execution systems
are typed outputs, not hidden calls inside `update`. Their responses return as later evidence. This
keeps transition latency visible and prevents a lock from spanning external I/O.

## One transition is one proposal

The model returns a `HypothesisTransition` containing next state and an ordered effect batch. The
runtime validates the complete proposal before touching live state:

```rust
HypothesisTransition::new(next_state)
    .cancel(RESPONSE_DEADLINE)
    .emit(Action::RequestImpactModel)
    .schedule(RESPONSE_DEADLINE, next_deadline)
```

If the model rejects evidence, restored state fails validation, a timer is invalid, or the effect
batch exceeds its configured bound, the runtime emits one typed rejection and commits nothing.
Completion also clears remaining timers.

## Two clocks and one strict sequence

Every `CausalEvidence<E>` carries:

| Field | Meaning |
|---|---|
| `sequence` | exact per-key ingress order, starting at zero |
| `effective_at` | when the underlying phenomenon applies |
| `available_at` | when this process was allowed to use it |

The distinction prevents hindsight from entering a replay. Availability is also the deadline clock.
A timer at `10:04:00` must fire before the machine accepts evidence first available at `10:04:01`.
Evidence available exactly at the deadline may be processed first and cancel it, but the frontier
must not already have advanced through that instant.

Out-of-order source data belongs in a bounded reorder operator before this runtime. Once admitted,
`available_at` is monotonic globally and `sequence` is gap-free within a key.

## Lifecycle

| Input | Accepted state change | Typical use |
|---|---|---|
| `Open` | creates revision 1 from sequence 0 | admit a new incident |
| `Evidence` | advances one key and increments revision | refine a conditional branch |
| `Advance` | fires due timers, then advances the frontier | make absence or timeout causal |
| `Close` | terminal tombstone with a reason | resolution outside model logic |
| `Retract` | terminal tombstone with a reason | invalidate the hypothesis |
| `Supersede` | terminalizes one key and opens its replacement atomically | correct identity or source facts |

Terminal tombstones are bounded. Their purpose is to prevent accidental key reuse and preserve
recent lifecycle evidence, not to replace a durable audit log.

## The Rust service pattern

The closest Rust equivalent to a ZIO service is a narrow trait plus constructor injection. Rust
adds an extra advantage: ownership expresses the resource scope without a runtime dependency
container.

```rust
struct Strategy<S> {
    hypotheses: S,
}

impl<S> Strategy<S>
where
    S: HypothesisService<Input = Input, Event = Event>,
{
    async fn ingest(&self, input: Input) -> Result<Vec<Event>, S::Error> {
        self.hypotheses.process(input).await
    }
}
```

Choose the concurrency boundary deliberately:

1. Own `HypothesisEngine` inside one source partition for the lowest overhead and simplest ordering.
2. Use `spawn_hypothesis_service` when many Tokio tasks need a cloneable handle. One bounded actor
   owns the engine, so there is no global mutex and mailbox capacity provides backpressure.
3. Use `SharedHypothesisEngine` only when an existing application context requires
   `Arc<tokio::sync::Mutex<_>>`. The lock covers the pure transition and snapshot copy only.

The actor task is an owned resource. Dropping its task handle aborts it, and a graceful shutdown
returns the engine for a final checkpoint. No detached worker is allowed to leak silently.

## Restart and the real commit boundary

The snapshot contains active records, terminal tombstones, the accepted frontier, and the latest
input availability. The derived timer queue is rebuilt and checked on restore. Model state is
validated through `HypothesisModel::validate` before the engine resumes.

`process_and_snapshot` serializes transition and snapshot under one actor command, so another caller
cannot interleave between them. It does not make storage transactional. The source driver still
needs this order:

1. Process an input and collect events plus the exact post-transition snapshot.
2. Atomically persist source position, versioned snapshot, and output outbox, or use stable
   idempotency identities where a shared transaction is impossible.
3. Publish outbox records.
4. Acknowledge the source only after the durable commit succeeds.

A canceled or disconnected caller can always make an RPC acknowledgement ambiguous after a worker
has committed. For strict source semantics, let the source driver own the engine directly instead
of treating the actor as a remote transaction coordinator.

## Boundedness and cost

`HypothesisConfig` bounds active keys, terminal keys, timers per key, effects per transition, and
timer fires per frontier advance. Common direct transitions use inline event buffers. The model
borrows current state and returns an owned next state, so a normal evidence update does not clone
model state inside the runtime. A frontier with no due deadline updates in place. A frontier with
due deadlines stages a bounded state copy so any model or effect failure rolls the complete
frontier advance back.

The Criterion suite covers one hot key, 1,024 interleaved keys, an idle frontier with 4,096 future
timers, and a frontier that fires and completes 4,096 keys. Treat those as reproducible mechanical
measurements, then benchmark the complete strategy with real state and sinks.

## What remains above this crate

- branch probabilities, priors, likelihoods, and calibration
- event clustering and identity resolution before `Open`
- external model execution and response correlation
- durable source, snapshot store, and outbox transactions
- distributed sharding and partition migration
- research validation, risk authority, and order execution

The complete hypothetical conditional chain is executable in
`rust/crates/helio_hypothesis/examples/conditional_shock.rs`.

The motivating space-weather chain is executable in
`rust/crates/helio_hypothesis/examples/space_weather.rs` and verified by
`rust/crates/helio_hypothesis/tests/space_weather_reference.rs`. Read the
[space-weather reference guide](../guide/space-weather-reference) for its causal and production
boundaries.
