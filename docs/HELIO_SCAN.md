# helio_scan: scan algebra for ordered streams

This document describes the **`helio_scan`** Rust library: a small **composable state-machine substrate** for research and execution pipelines over **ordered observations** (bars, events, mixed streams). It is intentionally **not** a bag of indicators or a monolithic backtester. The goal is a **typed scan algebra**: step, optional multi-emission, **flush** at control boundaries, **snapshot/restore**, and **checkpoint + offset** for resume.

Code lives under **`rust/crates/helio_scan/`**. Workspace layout: [HELIO_RUST_WORKSPACE.md](HELIO_RUST_WORKSPACE.md).

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

Run `cargo doc -p helio_scan --no-deps --open` from `rust/` for the full API.

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

## Where example / window scans live

**`helio_scan`** stays domain-agnostic. Reference window machines (`ForwardHorizonScan`, `EventClusterScan`, rolling windows, …) live in **`helio_window`**. Event-study wiring and replay tests live in **`helio_event`**.

Kernel-only unit tests are in **`helio_scan/src/kernel_tests.rs`**.

## Building and testing

From repository root:

```bash
cd rust
cargo test
cargo doc -p helio_scan --no-deps --open
```

The workspace **`default-members`** lists **`helio_scan`**, **`helio_time`**, **`helio_window`**, **`helio_event`** (not **`helios_signald`**), so a bare `cargo test` inside `rust/` does not require ZMQ.

To build the signal daemon (needs system **libzmq** and a C++ toolchain, as in CI):

```bash
cd rust
cargo build --release -p helios_signald
```

## Relationship to Python and `helios_signald`

- **Python** remains the natural home for Hydra config, notebooks, and heavy dataframe workflows.
- **`helios_signald`** is a thin ZMQ subscriber stub for the live JSON signal path (see [EXECUTION_AND_SIGNALS.md](EXECUTION_AND_SIGNALS.md)).
- **`helio_scan`** and friends are the **engine substrate** for Rust-side pipelines: deterministic scans, shared logic between backtest and live, and optional **checkpointed** runners over Kafka/Redis-style sources.

Nothing in `helio_scan` depends on ZMQ or Python.

## Roadmap (kernel)

Reasonable next layers **in `helio_scan` only**:

- More combinators: **merge**, **branch**, **fold** sink adapters, **stateful_map**.
- Transport-agnostic snapshot encoding seam (serde today; bincode/postcard later at the store).

Explicit **non-goals** for the kernel: async-first APIs, market/session types, Arrow-native kernels, proc-macro optics, full Kafka exactly-once beyond **checkpoint + offset** skeletons.

## Files

| Path | Role |
|------|------|
| `rust/Cargo.toml` | Workspace manifest |
| `rust/crates/helio_scan/` | Library crate |
| `rust/crates/helio_scan/src/lib.rs` | Crate root + re-exports |
| `rust/crates/helio_scan/src/emit.rs` | `Emit`, `VecEmitter`, bridge adapters |
| `rust/crates/helio_scan/src/scan.rs` | Core traits |
| `rust/crates/helio_scan/src/control.rs` | `FlushReason`, `Checkpoint`, … |
| `rust/crates/helio_scan/src/combinator.rs` | `Map`, `FilterMap`, `Then`, `ZipInput` |
| `rust/crates/helio_scan/src/focus.rs` | `Focus`, `ThenLeft`, … |
| `rust/crates/helio_scan/src/persist.rs` | `SnapshotStore`, `Persisted`, … |
| `rust/crates/helio_scan/src/runner.rs` | `Runner` |
| `rust/crates/helio_scan/src/kernel_tests.rs` | Kernel-only tests |
