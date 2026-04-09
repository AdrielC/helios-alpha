//! Explicit **observation** vs **availability** vs **effectivity** for causal pipelines.
//!
//! This crate sits above [`helio_scan`] and supplies data contracts so scans can enforce
//! *knowability* without encoding market specifics into the scan kernel.
//!
//! [`helio_scan`]: https://docs.rs/helio_scan (or the path dependency in this workspace)

mod gate;

pub use gate::*;

use helio_scan::SessionDate;
use serde::{Deserialize, Serialize};

/// When the value was physically observed (ingest, vendor timestamp), if tracked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ObservedAt(pub i64);

/// When the value may legally enter models or execution (causal cut).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct AvailableAt(pub i64);

/// Business-time the value refers to (e.g. event physical time), if distinct from availability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EffectiveAt(pub i64);

/// Wraps a payload with temporal metadata for availability gating and session alignment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Timed<T> {
    pub value: T,
    pub observed_at: Option<ObservedAt>,
    pub available_at: AvailableAt,
    pub effective_at: Option<EffectiveAt>,
    pub session_date: Option<SessionDate>,
}

impl<T> Timed<T> {
    pub fn new(value: T, available_at: AvailableAt) -> Self {
        Self {
            value,
            observed_at: None,
            available_at,
            effective_at: None,
            session_date: None,
        }
    }
}
