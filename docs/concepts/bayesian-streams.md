# Bayesian streams

A Bayesian streaming operator owns two things: a compact sufficient statistic and the prior that interprets it. Helios keeps those concerns explicit so a checkpoint contains the full runtime state and the pipeline fingerprint identifies the model configuration.

The implementation lives in `helio_stats`. It is domain-free. A return, latency, sensor measurement, or event count can use the same state machine when its likelihood assumptions fit.

## Choose the state from the quantity

| Quantity | Model | Checkpoint state | Merge operation |
|---|---|---|---|
| Event arrivals per unit exposure | Gamma-Poisson | event count, exposure | checked sums |
| Continuous effect with unknown mean and variance | Normal-Inverse-Gamma | count, mean, M2 | Chan merge |
| Clustered event intensity | Exponential Hawkes | last time, excitation, count | ordered update only |

Conjugacy is useful here because each update is constant-space and no historical sample vector is required. It is not a claim that the model is well specified.

## Arrival rates

For a Gamma prior with shape `α₀` and rate `β₀`, observing `k` events during exposure `e` gives:

```text
αₙ = α₀ + k
βₙ = β₀ + e
E[rate | data] = αₙ / βₙ
```

```rust
use helio_stats::GammaPoisson;

let model = GammaPoisson::try_new(1.0, 60.0)?;
let mut state = model.init();
state.try_observe(3, 300.0)?;

let posterior = model.try_posterior(&state)?;
assert_eq!(posterior.alpha(), 4.0);
assert_eq!(posterior.beta(), 360.0);
```

Exposure is explicit. Three events in five minutes and three events in five hours are not the same observation.

## Continuous effects

`NormalInverseGammaState` reuses the same stable Welford and Chan sufficient statistics as `OnlineMoments`. Given sample count `n`, sample mean `x̄`, and centered sum of squares `M2`:

```text
λₙ = λ₀ + n
μₙ = (λ₀μ₀ + nx̄) / λₙ
αₙ = α₀ + n / 2
βₙ = β₀ + M2 / 2 + λ₀n(x̄ - μ₀)² / (2λₙ)
```

The state can update one observation at a time or merge deterministic partitions. The fixed partition order still belongs in the fingerprint because floating-point addition is not associative.

## Replayable posterior draws

Ordinary Thompson sampling consumes mutable random state. That makes a restart depend on how many draws happened before the checkpoint. Helios instead derives every stream from a complete key:

```text
32-byte strategy fingerprint + decision ID + arm ID + sampler version
```

The current sampler is ChaCha8, version 2. SHA-256 derives its seed from the complete identity above, including every bit of the strategy fingerprint. The seed derivation and distribution versions are replay semantics. A future change must increment the sampler version and invalidate incompatible fingerprints.

```rust
use helio_stats::{ScalarPosterior, StrategyFingerprint, ThompsonKey};

let fingerprint = StrategyFingerprint::from_bytes(pipeline_sha256);
let key = ThompsonKey::new(fingerprint, decision_id, arm_id);
let first = posterior.try_draw(key)?;
let replay = posterior.try_draw(key)?;
assert_eq!(first, replay);
```

`StrategyFingerprint::try_from_hex` accepts the canonical 64-character digest emitted by the
backtest harness, so the same identity can cross the research and streaming layers without
truncation.

Do not truncate the pipeline digest or persist ambient thread RNG state. Persist the decision identity and posterior version that produced the choice.

## Constraints run first

`try_select_thompson` takes an injected feasibility function and an `Emit` sink. It rejects candidates before drawing, allocates no candidate collection, and keeps input order on exact ties. Arm IDs must be unique within each decision; the allocation-free selector does not scan ahead to detect duplicates.

This separation matters. The posterior answers what the model currently believes. The gate answers whether a candidate is permitted into the research choice set. Neither authorizes an order.

## What remains research-owned

- the likelihood and prior
- the utility transform
- how related frequencies partially pool
- delayed-outcome attribution
- liquidity, turnover, and risk constraints
- the falsifier and out-of-sample evaluation

Use these primitives to make those choices inspectable and replayable, not to hide them.
