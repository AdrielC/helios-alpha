# Researching rare events

Rare events are a demanding proving ground because the sample is small, the timing is ambiguous, overlap is common, and naive selection creates impressive but fragile charts.

## Why the system is feasible

A fast reaction path is useful when an observation has a well-defined availability time and a plausible mechanism connecting it to a market response. The streaming substrate can then:

1. Admit the event only when it became knowable.
2. Map it to the correct venue session.
3. Apply a lead-time and scope filter.
4. Open a forward observation horizon.
5. Generate a candidate signal through injected policy.
6. Replay the same state transitions after a restart.

That is feasible and valuable. It does not prove the event earns excess return after costs.

## Statistical model

Treat rare-event work as three linked models:

### Event occurrence

Model how events arrive and cluster. A Hawkes intensity can be one feature, provided its parameters are fitted and validated rather than guessed.

### Conditional response

Estimate the distribution of outcomes around the event under explicit alignment, exclusion, and overlap rules. Report uncertainty, not only the mean path.

### Decision and execution

Translate an estimated response into a decision under costs, latency, liquidity, position limits, and alternative opportunities. This layer is intentionally not part of the scan kernel.

## Controls

Controls should match the treatment set on pre-event information and respect spacing rules. Possible designs include calendar-matched controls, stratified resampling, and model-based counterfactuals. The right choice depends on the event mechanism and available sample.

## Common failure modes

- Defining an event with information published after the supposed reaction time.
- Searching many thresholds and reporting only the winner.
- Ignoring overlapping event windows.
- Treating clustered events as independent samples.
- Using a single global volatility regime.
- Omitting transaction costs and execution latency.
- Calling a conditional-intensity feature a prediction without out-of-sample calibration.

The [evidence standard](./evidence-standard) is deliberately stricter than the implementation standard.
