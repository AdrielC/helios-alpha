//! Temporal **semantics**: frequency, interval bounds, bucket/window specs, and causality helpers.
//!
//! ## Layers (keep separate)
//!
//! - **Frequency / bounds / specs** — this crate (`helio_time`).
//! - **Rolling buffers + aggregators + scans** — `helio_window`.
//! - **Generic scan algebra** — `helio_scan` (no market/time domain).
//!
//! ## Defaults
//!
//! - Interval membership defaults to **left-closed, right-open** [`Bounds::LEFT_CLOSED_RIGHT_OPEN`]
//!   (`[start, end)`).
//! - **Bucket interval ≠ availability**: see [`availability`] and [`Timed`].
//!
//! ## Modes
//!
//! - **Runtime**: [`Frequency`], [`BucketSpec`], [`WindowSpec`] (serde-friendly).
//! - **Typed / static** (optional): [`typed_freq`] (`Samples<N>`, `Fixed<N, Days>`, …).

mod anchor;
mod availability;
mod bounds;
mod bucket;
mod frequency;
mod gate;
mod typed_freq;
mod window_spec;

pub use anchor::*;
pub use availability::*;
pub use bounds::*;
pub use bucket::*;
pub use frequency::*;
pub use gate::*;
pub use typed_freq::*;
pub use window_spec::*;

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
