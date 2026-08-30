# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Stack

Delegated: VitePress for the documentation application, published through GitHub Pages. The computational substrate remains the existing Rust workspace with the existing Python research package.

## Users

The primary users are quant researchers composing, inspecting, validating, and replaying event-driven strategies. They need to move from an event hypothesis to an explicit streaming pipeline without losing causal timing, statistical state, or restart semantics.

Secondary users are Rust contributors implementing new scan, statistical, windowing, and persistence primitives, and operators responsible for checkpointed pipelines.

## Product Purpose

Helios Alpha is a research system for expressing event and rare-event trading hypotheses as composable, restartable streaming computations. Success means researchers can understand the temporal and statistical contract of a strategy, compose generic Rust operators into it, replay it deterministically, and distinguish a sound execution substrate from evidence that a strategy has alpha.

## Positioning

Helios Alpha treats strategy computation as typed state machines rather than a graph of trading-specific callbacks. Static Rust composition, injected reducers, explicit event time and watermarks, mergeable statistics, versioned snapshots, and source-position-aware restoration form one mechanism that can support rare-event research without baking a particular market, signal, or broker into the substrate.

## Operating Context

Researchers inspect architecture and API documentation, compose scans and reducers in Rust, use Python for research and data preparation, run deterministic replays and benchmarks, and evaluate event-shock strategies against controls. The documentation must lead from a strategy question to the correct primitives, then to validation and operational boundaries.

## Capabilities and Constraints

- Domain-free `Scan`, `FlushableScan`, `SnapshottingScan`, and `FallibleRestoreScan` abstractions with static composition.
- Stable online and parallel moments, covariance, rolling removal, and an exponential Hawkes intensity state.
- Bounded event-time reordering, typed late and overflow outcomes, generic bucket reduction, and ordered bucket pipelines.
- Versioned and fingerprinted checkpoints that retain stream offsets and validate restored payloads.
- An event-shock proving ground with deterministic replay, treatment/control generation, and simulated execution.
- A separately built operator application for strategies, pipeline stages, signals, active orders, held positions, risk state, and source freshness, with independent read and authenticated command ports.
- A lazy Perspective WebAssembly explorer for ad hoc grouping, filtering, and export without adding the analytical engine to the overview's initial load.
- The current repository is a research and execution substrate, not a claim of profitable alpha and not a live broker-connected trading system.
- Demonstration event streams and performance examples must be labeled synthetic when they are not sourced observations.
- The command frontend is implemented, but live trading still requires its durable server, broker integration, venue-grade risk controls, execution-cost modeling, observability, and deployment proof.

## Brand Commitments

The product name is Helios Alpha. Its voice is technically direct, statistically honest, and explicit about causality, uncertainty, and the boundary between implemented mechanics and unproven trading claims. Rare events are the initial proving ground, while the primitives remain useful for event streams generally.

## Evidence on Hand

- The Rust crates, tests, Criterion benchmarks, and rustdoc in `rust/`.
- Architecture and time/window documentation in `docs/`.
- Python research code, tests, notebooks, and configuration in the repository.
- Deterministic replay and checkpoint-resume tests in `rust/crates/helio_event`.
- No customer claims, production trading results, or verified alpha claims are available and none may be fabricated.

## Product Principles

1. Make causality and availability visible in every strategy.
2. Keep the substrate generic and inject domain decisions at composition boundaries.
3. Treat restart, ordering, rejection, and overflow behavior as part of the algorithm.
4. Use numerically stable online state and deterministic merge order.
5. Separate a production-capable mechanism from evidence that a strategy works.
