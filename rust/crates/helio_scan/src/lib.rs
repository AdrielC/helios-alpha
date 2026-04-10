//! # helio_scan
//!
//! **Composable scan machines** over ordered streams: on each input, update hidden state and emit
//! zero or more outputs. Optional **flush** at control boundaries, **snapshot/restore**, and
//! **checkpoint + offset** for deterministic resume.
//!
//! ## Design slogan
//!
//! Scans are **restartable**, **flushable**, **causality-aware** state machines over ordered
//! streams. **Composition preserves structure**. State is **inspectable**, **snapshotable**, and
//! **resumable by offset**.
//!
//! ## User guide
//!
//! The narrative design doc lives in the repo at `docs/HELIO_SCAN.md`. This crate root re-exports
//! the public API; run `cargo doc -p helio_scan --no-deps` from the `rust/` workspace for rustdoc.
//!
//! ## Core ideas
//!
//! - **[`Scan`]** — `init`, `step(state, input, emit)`, plus [`Scan::then`] (pipeline) and
//!   [`Scan::and`] (same-input fan-out). The emit parameter is an [`Emit`] sink so combinators can
//!   adapt output types without allocating a `Vec` per step.
//! - **[`FlushableScan`]** — `flush(state, signal, emit)` with [`FlushReason`] (session close,
//!   checkpoint, watermark, shutdown, …). Keeps **domain inputs** separate from **control**.
//! - **[`SnapshottingScan`]** — serializable [`Snapshot`](SnapshottingScan::Snapshot) distinct from
//!   raw runtime state when you need stable persistence.
//! - **Combinators** — [`Map`], [`FilterMap`], [`Then`] (pipeline), [`ZipInput`] (fan-out on same
//!   input), **arrow-style** [`Arr`], [`Split`], [`Merge`], [`Choose`], [`Fanin`], [`First`], [`Second`]
//!   ([`arrow`] module) and [`scan_then!`] for nested `Then`.
//!   Composed state uses **named structs** ([`ThenState`], [`ZipInputState`]) instead of tuple soup.
//! - **[`Focus`]** — minimal typed paths into composed state ([`ThenLeft`], [`ThenRight`],
//!   [`ZipInputA`], [`ZipInputB`]).
//! - **Persistence** — [`Checkpoint`], [`SnapshotStore`], [`Persisted`] wrapper that snapshots on
//!   [`FlushReason::Checkpoint`].
//! - **[`Runner`]** — owns `(machine, state)` and forwards `step` / `flush` / `step_batch`.
//! - **[`ScanBatchExt`]** — default `step_batch` = ordered `step`; **[`BatchOptimizedScan`]**
//!   for lawful fused batches (opt-in).
//! - **Runners** ([`runners`]) — `run_iter` / `run_batch`, `run_slice`, `run_receiver`, optional
//!   `run_stream` (Tokio MPSC, feature `stream`).
//!
//! ## Where domain logic lives
//!
//! Rolling windows, horizons, clustering, and event-study pipelines live in **`helio_window`** and
//! **`helio_event`**. **`helio_scan`** stays a cold, generic algebra.
//!
//! ## Non-goals (current version)
//!
//! Async-first runtime, distributed execution, Arrow kernels, proc-macro optics, and full Kafka
//! exactly-once beyond checkpoint+offset skeletons.

mod arrow;
mod batch;
mod batch_opt;
mod combinator;
mod control;
mod emit;
#[cfg(test)]
mod execution_tests;
mod flush_batch;
mod focus;
#[cfg(test)]
mod kernel_tests;
mod persist;
mod runner;
mod runners;
mod scan;

#[macro_use]
mod macros;

pub use arrow::*;
pub use batch::*;
pub use batch_opt::*;
pub use combinator::*;
pub use control::*;
pub use emit::*;
pub use flush_batch::*;
pub use focus::*;
pub use persist::*;
pub use runner::*;
pub use runners::*;
pub use scan::*;
