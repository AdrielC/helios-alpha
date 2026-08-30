//! Durable source-offset ownership for Golem-hosted hypothesis machines.
//!
//! Golem makes an agent's in-memory state durable, but source progress still needs an explicit
//! application contract. This crate couples a [`HypothesisEngine`] to one source partition and
//! admits only contiguous, bounded batches. It stages a full batch on a cloned engine and commits
//! only when every transition is accepted.
//!
//! The crate has no dependency on the Golem SDK. That keeps the domain kernel portable to
//! `wasm32-wasip2` and makes the Golem component a thin wire adapter.

mod shard;

pub use shard::*;
