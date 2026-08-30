# Helios Alpha

**Composable, replayable stream research for event-driven strategies.**

Helios Alpha is a Rust-first substrate and research lab for quant researchers who need to turn
late, bursty, or rare events into causal features and inspectable decisions. It gives you generic
state machines for ordering, windows, online statistics, Bayesian updates, keyed conditional
hypotheses, checkpointing, and replay. Trading policy is composed on top. It is not baked into the
kernel.

Space weather is the reference strategy. A solar observation opens a typed incident; propagation,
Earth-intersection, infrastructure, and market assessments arrive as later causal evidence; the
runtime emits a research candidate and stops before order authority. The same machinery applies to
other event shocks without putting space-weather or trading vocabulary in the core crates.

[Explore the Event Atlas](https://adrielc.github.io/helios-alpha/) ·
[Open Helios Control](https://helios-control-kappa.vercel.app/) ·
[Build a keyed hypothesis machine](https://adrielc.github.io/helios-alpha/concepts/hypothesis-machines) ·
[Build a 10-minute signal](https://adrielc.github.io/helios-alpha/guide/compose-a-strategy) ·
[Build a Thompson portfolio](https://adrielc.github.io/helios-alpha/guide/build-a-thompson-portfolio) ·
[Audit production readiness](https://adrielc.github.io/helios-alpha/operations/production-readiness) ·
[Inspect capital admission](https://adrielc.github.io/helios-alpha/operations/capital-admission) ·
[Choose the first data and broker path](https://adrielc.github.io/helios-alpha/operations/market-data-path) ·
[Inspect the Robinhood boundary](https://adrielc.github.io/helios-alpha/operations/robinhood) ·
[Operate the OMS](https://adrielc.github.io/helios-alpha/operations/oms) ·
[Choose the messaging plane](https://adrielc.github.io/helios-alpha/concepts/messaging-planes) ·
[Review the Golem Cloud architecture](https://adrielc.github.io/helios-alpha/operations/golem-cloud)

> **Status:** research infrastructure with an executable, fail-closed capital-control reference.
> The repository has an implemented but uncertified Robinhood Crypto adapter, no production
> evidence ledger, does not claim profitable alpha, and does not have permission to trade live
> capital.

## What you can compose

```text
event source
    │
    ▼
availability gate ──► bounded event-time reorder ──► watermark-closed buckets
                                                            │
                                                            ▼
                                              mergeable online statistics
                                                            │
                                      ┌─────────────────────┴─────────────────────┐
                                      ▼                                           ▼
                              deterministic rule                     Bayesian posterior
                                                                                  │
                                                                                  ▼
                                                               constraints, then keyed draw
                                                                                  │
                                                                                  ▼
                                                                       research candidate
```

The same primitives can express a 10-minute bucketed difference, a rolling variance, a clustered
arrival intensity, or a constrained portfolio of event-response horizons. Prices, sensors, filings,
news, and space-weather observations are all ordinary inputs when they satisfy the same timing and
likelihood contracts.

## The execution model

Helios uses Rust types and values as dependency injection:

- A `Scan` owns one typed state transition and emits zero or more values into an injected `Emit`
  sink. The hot path does not require a `Vec` allocation.
- Projectors, reducers, gates, clocks, and stores are explicit constructor arguments or generic
  parameters. There is no reflection-based container and no hidden service locator.
- `FlushableScan` separates watermarks, session close, shutdown, and end-of-input from domain data.
- `SnapshottingScan` separates runtime state from a stable persistence contract.
- Iterator, batch, async stream, channel, and ZMQ adapters stay outside the core algebra.

Static dispatch is the default. Dynamic dispatch remains an application choice at a plugin boundary,
not a tax paid by every observation.

## Conditional hypotheses that survive restarts

`helio_hypothesis` manages independent conditional inference chains keyed by incident, cluster, or
research identity. An injected model supplies state, evidence, outputs, and validation. The runtime
supplies exact sequences, revisions, deterministic availability-time deadlines, atomic effects,
bounded state, supersession, retraction, completion, and fallible snapshot restore.

For async applications, a typed `HypothesisService` is the Rust equivalent of a narrow ZIO service.
The preferred shared implementation is a bounded actor whose worker exclusively owns the mutable
engine. A deliberate `Arc<tokio::sync::Mutex<_>>` adapter exists for application contexts that need
it, but the lock never spans external I/O. Read
[Keyed hypothesis machines](https://adrielc.github.io/helios-alpha/concepts/hypothesis-machines)
for the lifecycle and durable commit boundary.

## A 10-minute stream in Rust

This pipeline reorders up to 4,096 observations, closes fixed 10-minute buckets on watermarks, and
computes count, mean, and variance without retaining the full bucket:

```rust
use helio_time::SecondWallBucket;
use helio_window::{F64MomentsReducer, OrderedBucketPipeline};

fn value(input: &Observation) -> f64 {
    input.value
}

let pipeline = OrderedBucketPipeline::try_new(
    4_096,
    SecondWallBucket::ten_minutes(),
    F64MomentsReducer::new(value as fn(&Observation) -> f64),
)?;
```

`F64MomentsReducer` uses Welford updates. Independent partitions combine with the
Chan-Golub-LeVeque recurrence, so variance is stable without `Σx² - (Σx)²/n`. Fix the partition and
merge tree when bitwise replay matters because floating-point addition is order-sensitive.

The full walkthrough covers availability, watermarks, typed late-data outcomes, and checkpoint
commit order: [Build a restartable 10-minute event signal](https://adrielc.github.io/helios-alpha/guide/compose-a-strategy).

## Replayable Bayesian decisions

`helio_stats` includes constant-space Gamma-Poisson arrival models, mergeable
Normal-Inverse-Gamma effect models, and a constrained Thompson selector. Every draw is derived from
a full strategy fingerprint and decision identity:

```rust
use helio_stats::{ScalarPosterior, StrategyFingerprint, ThompsonKey};

let fingerprint = StrategyFingerprint::from_bytes(pipeline_sha256);
let key = ThompsonKey::new(fingerprint, decision_id, arm_id);

let first = posterior.try_draw(key)?;
let replay = posterior.try_draw(key)?;
assert_eq!(first, replay);
```

If the fingerprint comes from `helio_backtest`, decode its canonical report field once with
`StrategyFingerprint::try_from_hex(&report.fingerprint_hex)?` before entering the decision loop.

Sampler version 2 hashes all 256 bits of the pipeline fingerprint with the decision ID, arm ID, and
version before initializing ChaCha8. Restarts do not depend on mutable thread-local RNG history.
Hard constraints run before any draw, and every rejection, sample, and selection can be emitted to
an injected audit sink.

The posterior does not define utility, constraints, delayed-feedback attribution, or order authority.
Those remain visible research decisions.

## Restart means state plus position

A valid recovery point binds:

1. A versioned operator snapshot.
2. The exact source offset represented by that snapshot.
3. The accepted watermark or equivalent event-time frontier.
4. A fingerprint of code, configuration, schemas, partitioning, and statistical parameters.

Restore validates external state before it returns to the hot path. Corrupt statistics, invalid
watermarks, incompatible fingerprints, over-capacity reorder queues, and non-representable counts
fail closed.

`AtomicCommitBundle` now provides executable reference semantics for the missing commit boundary:
one transaction advances a contiguous source prefix, stores the matching checkpoint, and appends
stable output identities to an outbox. `drain_outbox` requires an idempotent sink. Fault tests cover
failure before commit, a lost commit response, and a lost sink acknowledgement. A production store
still has to map that contract to its own serializable transaction.

## Workspace map

| Crate | Owns | Deliberately does not own |
|---|---|---|
| `helio_scan` | State machines, composition, emit sinks, controls, persistence seams | Markets, transports, business policy |
| `helio_hypothesis` | Keyed conditional lifecycle, deadlines, atomic model effects, snapshots, typed actor service | Domain meaning, durable transactions, execution authority |
| `helio_golem` | Atomic source-offset batches, deterministic invocation identities, validated shard snapshots | Golem SDK types, event-shock meaning, cloud credentials |
| `helio_time` | Frequencies, interval bounds, bucket grids, causal availability | Buffers and eviction machinery |
| `helio_window` | Bounded reorder, bucket reduction, rolling and session state | Signal meaning |
| `helio_stats` | Stable moments, compensated sums, scaled norms, log probabilities, Bayesian state, keyed Thompson draws, Hawkes intensity | Priors, objectives, alpha claims |
| `helio_event` | An event-shock proving ground and simulated strategy vertical | Broker authorization |
| `helio_execution` | Fixed-point orders, pre-trade risk, cost and capacity, broker reconciliation, incidents, operational readiness, capital admission | Research signal meaning, broker credentials, production evidence |
| `helio_oms` | Versioned order lifecycle, exact fills, replay-safe commands, event cursors, FIX 4.4 mapping, external OMS contract | FIX sockets and credentials, venue certification, transport authority |
| `helio_robinhood` | Official Robinhood Crypto signing, limit orders, lifecycle polling, cancellation, and decimal normalization | Credentials, rate scheduling, paper trading, equities and options, broker certification |
| `helio_backtest` | Fixed clocks, fingerprints, guarded Kalman research, replay harnesses | Live execution guarantees |
| `helios_signald` | Optional ZMQ integration | Kernel abstractions |
| `helio_bench` | Criterion workloads and baselines | Runtime dependencies |

Dependencies point from applications toward the small substrate crates. The substrate has no
trading vocabulary. See the [complete crate map](https://adrielc.github.io/helios-alpha/reference/crates).

## Quick start

### Test the Rust substrate

```bash
cd rust
cargo test
```

Run the focused statistical suite and benchmarks:

```bash
cd rust
cargo test -p helio_stats
cargo bench -p helio_bench --bench online_stats -- --noplot
```

Run the typed space-weather reference and its restart proof:

```bash
cd rust
cargo run -p helio_hypothesis --example space_weather
cargo test -p helio_hypothesis --test space_weather_reference
```

Run the capital-control crash and fault matrix:

```bash
cd rust
cargo test -p helio_scan -p helio_time -p helio_execution -p helio_oms -p helio_robinhood
cargo test -p helio_robinhood --all-features
cargo clippy -p helio_execution -p helio_oms -p helio_time -p helio_robinhood --all-targets --all-features -- -D warnings
```

The end-to-end paper test loses both a commit acknowledgement and a broker acknowledgement, then
proves that reconciliation produces one accepted order. Live dispatch additionally requires all
mandatory, unexpired evidence and a ready operational snapshot. See
[Capital admission](https://adrielc.github.io/helios-alpha/operations/capital-admission).

The complete support boundary is documented in
[Trade space weather without hiding the causal chain](https://adrielc.github.io/helios-alpha/guide/space-weather-reference).

`helios_signald` additionally needs `libzmq` and a C++ toolchain.

### Prove the Golem restart boundary

The `golem` application wraps the generic `helio_golem` driver in a real Golem Rust agent. Its
reference model follows a trigger through likelihood and market assessments, but the durable driver
contains no trading or event-shock vocabulary.

```bash
cd golem
golem build --yes
bash tests/golem_local_smoke.sh
```

The smoke test deploys both durable agent types to an isolated local Golem server. It proves
hypothesis offset resume plus OMS command de-duplication, exact partial-fill state, simulated agent
crashes, full server restart, and event-cursor resume. CI pins the Golem CLI binary and checks its
SHA-256 digest before running the same proof. Read the
[Golem deployment guide](https://adrielc.github.io/helios-alpha/operations/golem-cloud) for the
implemented boundary and the remaining cloud, certified-broker, deployment, and shadow evidence.

### Run the docs locally

```bash
npm ci
npm run docs:dev
```

Helios Control is a separate application in `apps/operator`. It shows strategies, processing
stages, candidate signals, active orders, held positions, risk state, and source freshness from a
deterministic demo port. Configure `window.__HELIOS_OPERATIONS__` with same-origin read URLs to
replace that fixture. Optional command-session and command URLs attach a separate protected
mutation boundary. Perspective 5.3 loads only when the operator opens Data Explorer, keeping the
initial overview independent of its WebAssembly payload and keeping the console out of the docs
build.

```bash
npm run operator:dev
npm run operator:build
npm run operator:check-performance
```

Read the [Helios Control deployment boundary](apps/operator/README.md) before connecting an
operations service.

GitHub Actions builds and publishes the VitePress site from `main`:
[adrielc.github.io/helios-alpha](https://adrielc.github.io/helios-alpha/).

### Run the example research vertical

The Python package is an empirical example built around public space-weather events and market
data. It demonstrates causal cuts, exchange-session alignment, feature generation, and event-study
artifacts. It is one vertical, not the identity of the Rust substrate.

```bash
python -m venv .venv
source .venv/bin/activate
pip install -e ".[dev]"

helios-pipeline \
  pipeline.start_date=2024-01-01 \
  pipeline.end_date=2024-01-31 \
  pipeline.as_of_date=2024-01-31
```

Use `uv sync` and `uv run helios-pipeline ...` for a lockfile-driven environment. Live API tests
are marked separately with `pytest -m integration`.

## Performance contract

The design removes common accidental costs, but it does not substitute architecture claims for a
benchmark:

- Online moment, covariance, conjugate Bayesian, and Hawkes updates are constant-space.
- The core emit path does not require per-observation heap allocation.
- Reorder and rolling state are explicitly bounded.
- Batch acceleration is opt-in and must prove equivalence with one-at-a-time execution.
- Statistical state rejects non-finite arithmetic and counts beyond exact `f64` integer precision.
- Neumaier-compensated sums retain their correction across checkpoints.
- Scaled sum-of-squares avoids intermediate overflow and underflow in norms.
- Conditional probabilities remain authoritative in log space after ordinary probability underflow.
- The guarded Kalman scan rejects poison input atomically and validates restored forecast state.
- Criterion workloads cover online updates, deterministic merges, checkpoint cadence, and windows.
- Keyed hypothesis benchmarks cover one hot key, 1,024 interleaved keys, and 4,096 deadline fires.

One release-mode Criterion quick pass on an Apple M3 Pro measured:

| Workload | Reference result |
|---|---:|
| Welford moment update | 107 million observations/s |
| Neumaier-compensated sum | 145 million observations/s |
| Scaled sum-of-squares norm | 127 million observations/s |
| Normal-Inverse-Gamma update | 108 million observations/s |
| SHA-256-keyed Gamma-Poisson draw | 431 ns, or 2.32 million draws/s |
| Keyed hypothesis update, one active key | about 46 ns, or 21.8 million updates/s |
| Keyed hypothesis update, 1,024 active keys | about 66 ns, or 15.1 million updates/s |
| Deadline fire and completion, 4,096-key frontier | about 334 ns each, or 3.00 million fires/s |

These are local microbenchmarks, not portable promises. Reproduce them with the benchmark command
above and retain history on the hardware that will run the strategy.

Measure the complete strategy with its real payloads, sinks, checkpoint cadence, and disorder
distribution. Latency from a microbenchmark is not end-to-end capacity.

## Research standard

For every event strategy, write down:

- what happened and when it became knowable
- what starts the clock and which watermark closes the feature
- the exposure, control construction, feedback delay, and transaction-cost model
- the prior or estimator, falsifier, holdout, and multiple-testing policy
- the exact fingerprint and source offsets required to replay the result

Rare events make leakage, dependence, regime changes, and selection bias more dangerous, not less.
The [evidence standard](https://adrielc.github.io/helios-alpha/research/evidence-standard) and
[Bayesian event portfolio](https://adrielc.github.io/helios-alpha/research/bayesian-event-portfolios)
notes separate implemented mechanics from claims that still need data.

## Honest boundaries

- Rich `WindowSpec` semantics exceed what every sample-count ring currently enforces. Use the
  time-keyed window paths when wall-clock expiry is required.
- The in-memory atomic store proves protocol semantics and crash behavior. It is not a production
  database adapter.
- Conjugate independent-arm posteriors are online primitives, not a replacement for hierarchical
  modeling across correlated horizons.
- The Python event study is a worked research vertical. Its source availability and empirical
  assumptions must be revalidated for each run.
- The risk, cost, gateway, readiness, incident, and admission state machines are implemented. Live
  capital remains closed until a separately deployed authority has a certified broker adapter and
  current production evidence for every mandatory gate.

Start with the [Event Atlas](https://adrielc.github.io/helios-alpha/), then follow one typed value from
availability through replay before attaching a strategy rule.
