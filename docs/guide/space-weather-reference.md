# Trade space weather without hiding the causal chain

Space weather is the reference strategy for Helios Alpha. It is demanding enough to exercise the
general system: heterogeneous observations arrive at different times, physical impact is
conditional, forecasts are revised, relevant markets depend on the affected infrastructure, and
the final candidate still needs an independent capital decision.

The reference is executable. It is not a claim that the supplied probabilities predict returns.

## The supported chain

| Stage | Evidence available to the process | Typed output | State owner |
|---|---|---|---|
| solar trigger | GOES X-ray or proton observation, source timestamp, receipt timestamp, quality flags | request propagation assessment | source adapter and reorder buffer |
| propagation | CME analysis, Earth-intersection probability, arrival interval, model version | request infrastructure assessment | keyed hypothesis machine |
| near-Earth confirmation | active L1 solar-wind measurements and their quality flags | revise, supersede, or retract the incident | keyed hypothesis machine |
| infrastructure impact | calibrated probability by sector and geography | request market assessment | injected impact model |
| market assessment | expected net return, uncertainty, capacity, costs, and availability time | research candidate | injected market model |
| capital decision | portfolio limits, freshness, liquidity, kill switches, and broker state | order intent or rejection | separate risk and execution service |

The Rust model emits the first five stages. It has no order action. This is deliberate.

NOAA publishes operational X-ray, proton, geomagnetic, and real-time solar-wind products. Its
R, S, and G scales separate radio-blackout, solar-radiation, and geomagnetic severity. NASA CCMC
DONKI provides useful CME analyses and modeled impact records, but DONKI itself directs users to
NOAA SWPC for the official United States forecast.

## What “support” means today

| Capability | Status | Evidence |
|---|---|---|
| historical flare, CME, proton, Kp, Dst, and market research | implemented | Python ingest and event-study vertical |
| causal event time and observation availability | implemented | `EffectiveAt`, `AvailableAt`, bounded reorder, and causal gates |
| conditional incident lifecycle | implemented | open, update, deadline, retract, supersede, complete |
| exact restart | implemented | versioned snapshots plus checkpoint-resume equivalence tests |
| tiny conditional probabilities | implemented | `LogProbability` retains the authoritative log value after display probability underflows |
| stable online moments | implemented | Welford updates and deterministic Chan merges |
| guarded sums and norms | implemented | Neumaier compensation and scaled sum-of-squares scans |
| guarded streaming forecast | implemented | atomic `GuardedKalmanLocalLevelScan` with fallible restore |
| production-shaped GOES, L1, Kp, and DONKI shadow adapters | implemented for single-host shadow | strict HTTPS normalization, append-only SQLite revisions, atomic checkpoints, and a validated operator projection |
| distributed scientific-source durability | not implemented | the local journal still needs persistent-volume, backup/restore, and acknowledged transport deployment proof |
| versioned forecast observation contract | implemented | exact series order, source identities, freshness budgets, and raw-manifest SHA-256 are validated in Python, Rust, and TypeScript |
| calibrated propagation, impact, and market models | not implemented | the executable example uses synthetic values |
| live capital and broker authority | intentionally outside | production admission gate remains closed |

This is enough to prove that the abstractions can express the strategy. It is not enough to trade
the strategy with live capital.

## Run the typed reference

```bash
cd rust
cargo run -p helio_hypothesis --example space_weather
cargo test -p helio_hypothesis --test space_weather_reference
```

The test proves four properties:

1. checkpoint and restore produce the same event sequence as uninterrupted execution;
2. a conditional chain below ordinary `f64` probability range retains a finite log probability;
3. a non-finite market forecast is rejected without changing live hypothesis state.
4. an arrival interval that is not strictly in the future is rejected without changing state.

## Numerical policy

Floating point cannot make every physical or statistical quantity representable. The runtime can
make failure explicit and keep invalid arithmetic out of durable state.

| Risk | Mechanism | Failure behavior |
|---|---|---|
| cancellation in long sums | Neumaier-compensated `CompensatedSum` | non-finite input and unrepresentable total are typed errors |
| overflow or underflow in squared norms | LAPACK-style `ScaledSumSquares` | stable norm when representable, explicit error when the final square is not |
| small variance beside a large offset | Welford local update and Chan partition merge | invalid variance state is rejected atomically |
| conditional probability underflow | natural-log `LogProbability` | comparisons retain log probability even when display probability is zero |
| poisoned forecast input | `GuardedKalmanLocalLevelScan` | rejects input and preserves the previous checkpointable state |
| poisoned smoother input | `GuardedEmaScan` | rejects input, alpha, or restored state without mutation |
| corrupted restored state | `FallibleRestoreScan` | resume stops before processing new evidence |

Do not compare or rank very small hypotheses using `LogProbability::probability()`. Use
`ln_probability()` until a display or external API requires ordinary probability space.

## Causal correction in the research vertical

The candidate-time Solar Shock Index no longer weights observed Dst around the predicted arrival
window. That value is future information at the precursor cut. It remains an outcome for
retrospective evaluation. The candidate score now uses only trigger-side features and reports
missing inputs explicitly. A live adapter must attach a real availability timestamp to each
feature rather than infer availability from the measurement timestamp.

## The neutrino question

The runtime can admit a future neutrino-derived precursor because evidence types are injected. The
shipped reference does not assert that a neutrino burst is an operationally calibrated precursor
to a solar flare. The initial trigger uses established solar and heliospheric observations. Any
new precursor needs source provenance, detection latency, false-positive calibration, and
out-of-sample evidence before it can open the same hypothesis chain.

## Production gates

Before live capital, add all of the following:

- deploy the shadow journal on monitored persistent storage and prove backup, restore, outage, and
  acknowledged-transport recovery;
- identity resolution for revised and duplicated solar incidents;
- versioned, calibrated propagation and infrastructure models with reliability diagrams;
- purged walk-forward market studies that include alert latency, revisions, costs, and capacity;
- shadow execution through the same outbox and risk boundary used in production;
- freshness limits, exposure caps, circuit breakers, and human-visible incident traces.

The system should fail closed when any required source is stale, any probability or forecast is
non-finite, a source revision cannot be reconciled, or the candidate has crossed its arrival or
market-data freshness boundary.

See [Run the scientific shadow](/operations/space-weather-shadow) for the executable source and
operator handoff.

## Operational sources

- [NOAA SWPC notifications timeline](https://www.swpc.noaa.gov/products/notifications-timeline)
- [NOAA SWPC real-time solar wind](https://www.swpc.noaa.gov/products/real-time-solar-wind)
- [NOAA space weather scales](https://www.swpc.noaa.gov/noaa-scales-explanation)
- [NOAA NCEI GOES X-ray instruments](https://www.ncei.noaa.gov/products/goes-1-15/space-weather-instruments)
- [NASA CCMC DONKI](https://ccmc.gsfc.nasa.gov/tools/DONKI/)
