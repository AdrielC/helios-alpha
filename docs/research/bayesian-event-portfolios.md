# Bayesian event portfolios

A rare-event portfolio has two separate problems:

1. estimate how often an event process arrives and what its conditional effect distribution looks like
2. allocate limited research attention or exposure while outcomes are sparse, delayed, and constrained

Thompson sampling is useful for the second problem only after the first problem is credible.

## A model stack for rare events

| Layer | Recommended starting point | Why it exists |
|---|---|---|
| arrival frequency | Gamma-Poisson | updates event rates with explicit exposure |
| arrival clustering | exponential Hawkes | represents short-run self-excitation in O(1) ordered state |
| activation | hierarchical Bernoulli or hurdle model | separates no-response events from nonzero magnitude |
| effect magnitude | robust Student-t hierarchy | tolerates heavier tails than a Gaussian effect model |
| online conjugate approximation | Normal-Inverse-Gamma | compact, mergeable unknown mean and variance state |
| regime change | Bayesian online change-point detection | limits pooling across structural breaks |
| allocation | constrained, delay-aware hierarchical Thompson sampling | samples uncertainty while respecting a feasible set |

No single row is the system. The event definition, availability contract, model diagnostics, costs, and falsifier connect them.

## Why a hierarchy matters

The 1-minute, 10-minute, and 1-hour response to one event are correlated views, not three independent experiments. Independent arms can double-count evidence and overstate certainty.

A useful hierarchy shares information across:

- event family
- instrument or sector
- response horizon
- volatility or liquidity regime
- detection confidence

Partial pooling lets sparse arms borrow strength while retaining arm-specific uncertainty. Hierarchical Bayesian bandit research shows that this structure can improve learning when tasks are related. The production question is whether the hierarchy matches the causal and operational relationships in this dataset.

Helios currently provides mergeable per-arm conjugate states and the replayable constrained selector. A full robust hierarchy belongs in the Python research layer until its inference contract is stable enough to compile into an online adapter.

## Why plain Beta-Bernoulli is too small

A success/failure posterior is appropriate only when the outcome is genuinely binary and the definition is stable. Event returns are usually continuous, heavy-tailed, cost-sensitive, and delayed. Collapsing them into a win flag discards magnitude and makes the threshold part of the model without admitting it.

Use a hurdle model when both questions matter:

```text
P(nonzero response | event, context)
×
effect magnitude | nonzero response, event, context
```

The selector should draw utility after costs and risk, or draw model parameters and pass them through an injected utility function. It should not mistake raw posterior mean for tradable edge.

## Constraints are part of the policy

Run hard feasibility checks before sampling:

- data freshness and causal availability
- instrument eligibility
- minimum liquidity
- maximum turnover
- concentration and inventory limits
- tail-risk or CVaR budget

Risk-constrained Thompson methods exist, but no paper chooses the correct business constraint for this system. Constraint definitions, estimators, and failure behavior remain research-owned and versioned.

## Delayed feedback changes the experiment

An hour-horizon outcome arrives later than a minute-horizon outcome. Updating all arms immediately in a backtest leaks future information and makes the policy easier than the live problem.

Model pending outcomes explicitly and replay their actual availability times. Delay-aware Thompson results provide theoretical guidance, but the empirical delay distribution and cancellation rules still need measurement.

## Detecting regime changes

Bayesian online change-point detection maintains a posterior over run length. It can gate or discount pooling when the current sequence no longer resembles the previous regime.

Use it as a diagnostic and state transition, not a magic alpha detector. The hazard prior, observation model, and reset policy can dominate the result. Persist run-length state and version them in the same way as any other scan.

## Where Ax belongs

[Ax](https://ax.dev/docs/tutorials/quickstart/) and BoTorch are strong tools for sequential experiment design. Put them in the Python research plane for expensive choices such as:

- event threshold and matching policy
- prior and hierarchy hyperparameters
- window lengths and response horizons
- change-point hazard
- risk-budget settings

Do not call Ax on the Rust hot path for each market observation. Export a versioned experiment contract, evaluate candidates through causal backtests, and promote the selected configuration into the Rust pipeline fingerprint. Ax also supports external generation nodes, which is the clean boundary when Helios owns candidate generation or evaluation.

## Evaluation protocol

1. Pre-register the event detector, availability cut, hierarchy, utility, and gates.
2. Split by time and event cluster, not random rows.
3. Replay delayed outcomes at their actual availability time.
4. Compare against no-pooling, no-bandit, and simple allocation baselines.
5. Report posterior calibration, regret or decision utility, turnover, CVaR, and constraint violations.
6. Stress priors, event definitions, matching rules, and regime boundaries.
7. Reserve a final untouched period and report null results.

## Primary references

- [Ax quickstart](https://ax.dev/docs/tutorials/quickstart/)
- [Ax external generation nodes](https://ax.dev/docs/tutorials/external_generation_node/)
- [BoTorch constrained Thompson sampling](https://botorch.org/docs/tutorials/scalable_constrained_bo)
- [BoTorch Thompson sampling](https://botorch.org/docs/tutorials/thompson_sampling)
- [Hierarchical Bayesian Bandits](https://proceedings.mlr.press/v151/hong22c.html)
- [Thompson Sampling with Unrestricted Delays](https://arxiv.org/abs/2202.12431)
- [Bayesian Online Changepoint Detection](https://arxiv.org/abs/0710.3742)
- [Risk-Constrained Thompson Sampling for CVaR](https://arxiv.org/abs/2011.08046)

These references motivate methods. They do not establish that a Helios event definition produces alpha.
