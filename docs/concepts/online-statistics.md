# Online statistics

Streaming statistics must update one item at a time, merge partial partitions, and survive restart without changing their meaning.

## Guarded accumulation

`CompensatedSum` implements Neumaier's compensated sum, a stronger Kahan-family recurrence for
mixed-magnitude and cancellation-heavy streams. Its correction term is part of the serialized
state, so a checkpoint does not silently discard precision.

`ScaledSumSquares` uses the scaled `xLASSQ` recurrence used by numerical linear algebra libraries.
It can compute the norm of `[3e200, 4e200]` or `[3e-200, 4e-200]` without first forming squares that
overflow or underflow.

`LogProbability` stores the natural logarithm of a probability. Conditional products become sums.
The ordinary probability may eventually round to zero, but the log value remains usable for
comparison and downstream probability algebra.

All three have `helio_scan` adapters, versioned snapshots, fallible restore, atomic rejection, and
explicit non-finite or overflow errors.

## Variance: Welford locally, Chan across partitions

`OnlineMoments` stores `(count, mean, M2)`, where `M2` is the sum of squared deviations from the mean.

For one new value, Welford's recurrence avoids the catastrophic cancellation in `sum(x²) - sum(x)² / n`. For two partial states, the Chan-Golub-LeVeque merge combines their counts, means, and `M2` values.

```rust
use helio_stats::{OnlineMoments, merge_moments_balanced};

let mut left = OnlineMoments::new();
left.try_push(1.0)?;
left.try_push(2.0)?;

let mut right = OnlineMoments::new();
right.try_push(3.0)?;
right.try_push(4.0)?;

let merged = merge_moments_balanced([left, right])?;
assert_eq!(merged.mean(), Some(2.5));
```

Floating-point merging is associative only in exact arithmetic. Keep partitioning and the deterministic merge tree in the pipeline fingerprint when bit-reproducible replay matters.

## Rolling removal

`try_remove` supports bounded windows. Removal is less robust than immutable block merges for long-lived, high-dynamic-range series. Rebuild periodically from the owned ring buffer or use a block merge tree when that error profile matters.

## Forecast state

`GuardedKalmanLocalLevelScan` is the fallible streaming adapter for the local-level forecast. It
validates configuration, observations, live state, and restored snapshots. The covariance update
uses Joseph form to preserve non-negativity under rounding. A rejected observation leaves the
previous state unchanged.

`GuardedEmaScan` applies the same policy to exponential smoothing: finite alpha in `[0, 1]`, finite
input, finite restored state, fused multiply-add for the update, and no mutation after rejection.

## Covariance

`OnlineCovariance` maintains both marginal variances and the co-moment needed for covariance and correlation. It supports the same one-item update, partial merge, snapshot, and validation story.

## Hawkes intensity

`ExponentialHawkes` maintains exponential-kernel excitation in O(1) state per event:

```text
λ(t) = baseline + Σ jump × markᵢ × exp(-decay × (t - tᵢ))
```

This is an online filter, not a fitting procedure and not evidence of predictability. Fit parameters out of sample, validate residuals, and include mark and regime assumptions before using intensity as a research feature.

## Bayesian sufficient statistics

`helio_stats` also provides Gamma-Poisson arrival-rate state, Normal-Inverse-Gamma effect state, and a constrained Thompson selector with deterministic keyed draws. The Bayesian states reuse the same merge, snapshot, validation, and scan contracts described above.

Read [Bayesian streams](./bayesian-streams) for the formulas and replay contract, then [Build a constrained Thompson portfolio](../guide/build-a-thompson-portfolio) for a multi-frequency composition.
