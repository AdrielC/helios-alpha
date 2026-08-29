# Evidence standard

Helios separates four claims that are often collapsed.

| Claim | Required proof |
|---|---|
| The code implements a mechanic | Unit, property, replay, and boundary tests |
| The mechanic is fast enough | Reproducible benchmark with environment and workload |
| The event predicts an outcome | Out-of-sample statistical evidence with uncertainty |
| The strategy is tradable | Net-of-cost simulation, capacity, and live operational evidence |

Passing one row does not imply the next.

## Minimum research record

Record these beside every event study:

- Event definition and the timestamp at which it becomes actionable.
- Source revision policy and causal cut.
- Universe and venue calendar.
- Treatment, exclusion, overlap, and control-selection rules.
- All thresholds and transformations selected before the holdout.
- Sample count, effect estimate, interval, and sensitivity checks.
- Multiple-testing correction or a clear statement that the analysis is exploratory.
- Transaction-cost and latency assumptions.
- Code and data fingerprints sufficient to replay the result.

## Synthetic demonstrations

Synthetic data is appropriate for documentation, invariant tests, and throughput benchmarks. Label it at the point of use. Do not present a synthetic response curve as market evidence.

## Negative results

A negative or inconclusive event study is still useful. It constrains the mechanism, the response window, or the feasible trading hypothesis. The generic streaming substrate remains useful even when a particular event family has no alpha.
