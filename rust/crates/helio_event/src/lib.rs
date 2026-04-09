//! Domain **proving ground** on [`helio_scan`], [`helio_time`], and [`helio_window`]: classic
//! event-study pipelines **and** generic forecastable **event-shock** machinery (`event_shock`).
//!
//! **Scope:** Internal substrate — names like [`TreatmentSelectorScan`](crate::TreatmentSelectorScan)
//! and [`CausalEventStudyPipeline`](crate::CausalEventStudyPipeline) are **research-shaped**. If the
//! crate grows further, consider splitting later into **generic event machinery** vs **event-study
//! analysis**; nothing here is forced to stay a single public package forever.
//!
//! The kernel stays in **`helio_scan`**; this crate may use bars, sessions, and causal tagging.

mod event_shock;
mod fold;
mod pipeline;
mod sampler;
mod selector;
mod types;

pub use event_shock::*;
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
