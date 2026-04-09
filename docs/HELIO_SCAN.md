# helio_scan: scan algebra for ordered streams

This document describes the **`helio_scan`** Rust library: a small **composable state-machine substrate** for research and execution pipelines over **ordered observations** (bars, events, mixed streams). It is intentionally **not** a bag of indicators or a monolithic backtester. The goal is a **typed scan algebra**: step, optional multi-emission, **flush** at control boundaries, **snapshot/restore**, and **checkpoint + offset** for resume.

Code lives under `rust/helio_scan/`. The Rust workspace root is `rust/Cargo.toml` (members: `helio_scan`, `helios_signald`).

## Design slogan

Scans are **restartable**, **flushable**, **causality-aware** state machines over ordered streams. **Composition preserves structure**. State is **inspectable**, **snapshotable**, and **resumable by offset**.

## Why this exists

Most quant and event-study logic is naturally a **Mealy-style machine**: on each input you update hidden state and may emit **zero or more** outputs. Rolling windows, overlap clustering, forward labeling, portfolio accounting, and “what was knowable at time *t*” filters are all the same shape if you stop encoding them as ad hoc loops and classes.

`helio_scan` gives you:

- A **single stepping contract** (`Scan`) with an explicit **emit sink** (no per-step `Vec` allocation in the hot path by default).
- **Control semantics** separate from domain inputs (`FlushableScan` + `FlushReason`).
- **Persistence seam** (`SnapshottingScan`, `Checkpoint`, `SnapshotStore`, `Persisted` wrapper).
- **Static composition** with **named state types** (`ThenState`, `ZipInputState`, …) instead of anonymous tuple nests.
- **Typed paths into composed state** (`Focus` + canned focuses for `Then` / `ZipInput`).

## Mental model

For each scan instance:

1. **`init()`** produces initial `State`.
2. **`step(state, input, emit)`** updates `state` and calls `emit` zero or more times.
3. Optionally **`flush(state, signal, emit)`** reacts to **control boundaries** (session end, watermark, checkpoint, shutdown, …).
4. Optionally **`snapshot` / `restore`** maps runtime state to a **stable serializable form** (often ≠ raw in-memory layout).

Composition (e.g. `Then`) wires **outputs** of an upstream scan into **inputs** of a downstream scan inside the same `step`, using an internal buffer for the bridge.

## Core traits

| Trait | Role |
|--------|------|
| `Emit<T>` | Output sink; `VecEmitter` for tests. |
| `Scan` | `In`, `Out`, `State`; `init`, `step`. |
| `FlushableScan` | `Offset` type + `flush` with `FlushReason<Offset>`. |
| `SnapshottingScan` | `Snapshot: Serialize + DeserializeOwned`; `snapshot`, `restore`. |
| `VersionedSnapshot` | `const VERSION: u32` on snapshot types for future migration. |

Run `cargo doc -p helio_scan --open` from `rust/` for the full API.

## Control and checkpoints

- **`FlushReason<O>`** — why flush happened: `SessionClose`, `Checkpoint(O)`, `Watermark(O)`, `Shutdown`, `Rebalance`, `EndOfInput`, `Manual`. Different scans may ignore or honor different variants.
- **`Checkpoint<S, O>`** — bundles **`snapshot`**, **`offset`**, optional **`watermark`**, and **`CheckpointMeta`** (format version, label). **State without an offset is not a resume story**; checkpoints pair serialized state with a stream position (Kafka offset, Redis stream ID, sequence number, session+row, …).

The **`Persisted<S, Store, KeyFn>`** wrapper delegates `step`/`flush` to an inner scan and, on **`FlushReason::Checkpoint`**, writes a `Checkpoint` via **`SnapshotStore`**. Domain-specific keying is supplied by **`CheckpointKeyFn`**.

## Combinators

| Combinator | Purpose | Composed state |
|------------|---------|----------------|
| `Map` | Map outputs | Same as inner |
| `FilterMap` | Filter/map outputs | Same as inner |
| `Then` | Pipeline: left `Out` → right `In` | `ThenState<L, R>` |
| `ZipInput` | Same input to two scans; outputs tagged `ZipInputOut::A` / `::B` | `ZipInputState<A, B>` |

Extension trait **`ScanExt`** provides `.map`, `.filter_map`, `.then` on any `Scan`.

**`ZipInput`** requires **`A::In: Clone`** so both children see the same input. **`FlushableScan`** for `ZipInput` forwards **`flush`** to both sides (order: **A** then **B**).

## Focus (minimal optics)

**`Focus<T>`** exposes `get` / `get_mut` into a root state type. Canned values:

- **`ThenLeft`**, **`ThenRight`** on `ThenState`
- **`ZipInputA`**, **`ZipInputB`** on `ZipInputState`

A generic **`Compose<F, G>`** combinator for arbitrary nested focuses is **not** included: Rust’s lifetime rules on nested associated types make a fully general compose awkward without a heavier design. Prefer **explicit field access** or **stacked** `Focus` calls in tests and tooling.

## Example scans (reference implementations)

The crate ships two small machines in `examples.rs` (also exercised by unit tests):

1. **`EventClusterScan`** — cluster raw point events by **maximum gap in days**; finalize clusters on large gaps or on flush (`EndOfInput`, `Shutdown`, `SessionClose`, `Manual`).
2. **`ForwardOutcomeScan`** — interleaved **`MarketOrTreatment`** stream: treatments attach to the **next** bar; **horizon** counts bars until a **`ForwardOutcome`** is emitted.

These are **pedagogical**, not production SSI or event-study logic. They show how flush and snapshot behave under composition and checkpointing.

## Building and testing

From repository root:

```bash
cd rust
cargo test -p helio_scan
cargo doc -p helio_scan --no-deps --open
```

The workspace sets **`default-members = ["helio_scan"]`**, so a bare `cargo test` inside `rust/` exercises the library **without** building the ZMQ subscriber.

To build the signal daemon (needs system **libzmq** and a C++ toolchain, as in CI):

```bash
cd rust
cargo build --release -p helios_signald
```

## Relationship to Python and `helios_signald`

- **Python** remains the natural home for Hydra config, notebooks, and heavy dataframe workflows.
- **`helios_signald`** is a thin ZMQ subscriber stub for the live JSON signal path (see [EXECUTION_AND_SIGNALS.md](EXECUTION_AND_SIGNALS.md)).
- **`helio_scan`** is the **engine substrate** for future Rust-side pipelines: deterministic scans, shared logic between backtest and live, and optional **checkpointed** runners over Kafka/Redis-style sources.

Nothing in `helio_scan` depends on ZMQ or Python.

## Roadmap (not yet in crate)

Reasonable next layers, aligned with the same traits:

- **`WindowScan`** (or `expire` on a time key) for rolling windows, forward horizons, and watermark-driven finalization.
- **Typed time keys** on inputs (`event_time`, `available_at`, session id) as **separate types** or traits, so scans declare what time semantics they require.
- More combinators: **merge**, **branch**, **fold** over emitted outputs, **stateful_map**.
- **Profunctor-style** `dimap` / `contramap` for adapters (only if the ergonomics win is clear).

Explicit **non-goals** for the current design pass: async-first APIs, distributed execution in-crate, Arrow-native kernels, proc-macro optics, and full Kafka exactly-once semantics beyond **checkpoint + offset** skeletons.

## Files

| Path | Role |
|------|------|
| `rust/Cargo.toml` | Workspace manifest |
| `rust/helio_scan/` | Library crate |
| `rust/helio_scan/src/lib.rs` | Crate root + re-exports |
| `rust/helio_scan/src/emit.rs` | `Emit`, `VecEmitter`, bridge adapters |
| `rust/helio_scan/src/scan.rs` | Core traits |
| `rust/helio_scan/src/control.rs` | `FlushReason`, `Checkpoint`, … |
| `rust/helio_scan/src/combinator.rs` | `Map`, `FilterMap`, `Then`, `ZipInput` |
| `rust/helio_scan/src/focus.rs` | `Focus`, `ThenLeft`, … |
| `rust/helio_scan/src/persist.rs` | `SnapshotStore`, `Persisted`, … |
| `rust/helio_scan/src/runner.rs` | `Runner` |
| `rust/helio_scan/src/examples.rs` | Example scans + tests |
