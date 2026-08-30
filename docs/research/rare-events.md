# Researching rare events

Rare events punish loose methodology. Samples are small, timestamps disagree, event windows
overlap, and threshold searches can manufacture impressive charts.

## Feasibility

A fast reaction path is useful only when the observation has a defensible availability time and a
plausible mechanism connecting it to the response. The runtime can then:

1. Admit the event only when it became knowable.
2. Map it to the correct venue session.
3. Apply a lead-time and scope filter.
4. Open a forward observation horizon.
5. Generate a candidate signal through injected policy.
6. Replay the same state transitions after a restart.

Those mechanics are feasible. Excess return after costs remains an empirical claim.

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
