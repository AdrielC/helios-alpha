# Build a constrained Thompson portfolio

This walkthrough turns three related event-response horizons into one replayable research decision. It stops at a candidate. Execution, capital, and broker authorization stay outside this layer.

## 1. Define one arm per decision, not per chart

Suppose a detected event has 1-minute, 10-minute, and 1-hour response views. They are measurements of the same event process, so do not present every overlapping window as an independent discovery.

Start with a contract:

| Field | Example |
|---|---|
| event | typed event available at `09:39:42Z` |
| outcomes | net response at 1m, 10m, and 1h |
| feedback delay | horizon close plus source availability delay |
| utility | posterior net effect after estimated costs |
| gates | minimum liquidity, maximum turnover, tail-risk budget |
| emission | research candidate with no order authority |

For a production study, fit a hierarchy that shares information across horizons. The independent conjugate states below are the online mechanism, not a substitute for that research model.

## 2. Update mergeable effect states

```rust
use helio_stats::NormalInverseGamma;

let effect = NormalInverseGamma::try_new(
    0.0, // prior mean
    1.0, // prior mean precision
    2.0, // prior variance shape
    1.0, // prior variance scale
)?;

let mut one_minute = effect.init();
let mut ten_minutes = effect.init();
let mut one_hour = effect.init();

one_minute.try_observe(0.0018)?;
ten_minutes.try_observe(0.0042)?;
one_hour.try_observe(0.0029)?;
```

Each state contains count, mean, and `M2`. A worker can update a partition and merge it into a fixed reduction tree. Snapshot validation rejects corrupt or non-finite state.

## 3. Build the decision set

```rust
use helio_scan::VecEmitter;
use helio_stats::{
    Eligibility, StrategyFingerprint, ThompsonCandidate, try_select_thompson,
};

let posterior = [
    effect.try_posterior(&one_minute)?,
    effect.try_posterior(&ten_minutes)?,
    effect.try_posterior(&one_hour)?,
];

let candidates = [
    ThompsonCandidate { id: "1m", arm_id: 1, posterior: &posterior[0] },
    ThompsonCandidate { id: "10m", arm_id: 2, posterior: &posterior[1] },
    ThompsonCandidate { id: "1h", arm_id: 3, posterior: &posterior[2] },
];

let fingerprint = StrategyFingerprint::from_bytes(pipeline_sha256);
let mut trace = VecEmitter::new();
let decision = try_select_thompson(
    fingerprint,
    decision_id,
    candidates,
    |candidate| match candidate.id {
        "1m" if expected_turnover_bps > turnover_limit_bps => {
            Eligibility::Rejected { constraint: 1 }
        }
        "1h" if tail_loss_bps > tail_loss_limit_bps => {
            Eligibility::Rejected { constraint: 2 }
        }
        _ => Eligibility::Eligible,
    },
    &mut trace,
)?;
```

The gate runs before `try_draw`. A rejected arm has no sampled value in the trace. That makes the decision auditable and prevents an attractive draw from weakening a hard constraint.

## 4. Make delayed feedback explicit

Do not update the 1-hour posterior when only ten minutes have elapsed. Persist a pending-outcome record keyed by:

```text
event ID + horizon + decision ID + availability cut
```

When the outcome becomes causally available, admit it through the normal ordered pipeline. Unrestricted feedback delay can change the statistical behavior of a bandit policy, so evaluate the actual delay distribution rather than backfilling every arm at event time.

## 5. Checkpoint a decision boundary

Persist these fields together:

- source offset and watermark
- each posterior sufficient statistic
- model prior and version in the pipeline fingerprint
- full 32-byte strategy fingerprint, decision ID, unique arm IDs, and sampler version
- candidate ordering and constraint codes
- pending delayed outcomes

On restore, validate the fingerprint before consuming the next source offset. The keyed draws do not require mutable RNG state.

## 6. Replay the explanation

The `Emit<ThompsonTrace<_>>` sink receives rejected, sampled, and selected records without forcing allocation in the selector. A production adapter can send those records to a bounded audit sink. A test can use `VecEmitter` and compare the full trace exactly.

The result is a deterministic research choice under an explicit model and explicit constraints. It is not evidence that the event effect exists. Use [Bayesian event portfolios](../research/bayesian-event-portfolios) to design the empirical layer and [Evidence standard](../research/evidence-standard) to decide what the study may claim.
