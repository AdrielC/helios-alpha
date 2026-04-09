//! Event-study **domain** layer: treatment/control types, configuration, and composable scans on top
//! of [`helio_scan`], [`helio_time`], and [`helio_window`].
//!
//! The scan kernel remains in **`helio_scan`**; this crate is allowed to know about bars, sessions,
//! and causal tagging.

mod fold;
mod pipeline;
mod sampler;
mod selector;
mod types;

pub use fold::*;
pub use pipeline::*;
pub use sampler::*;
pub use selector::*;
pub use types::*;

/// Re-export canonical window machines for treatment clustering and forward outcomes.
pub use helio_window::{
    ClusteredEvent, EventClusterScan, ForwardHorizonOutcome, ForwardHorizonOutput,
    ForwardHorizonScan, RawEvent,
};
