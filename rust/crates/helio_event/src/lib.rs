//! **Application / flagship workload** on [`helio_scan`], [`helio_time`], and [`helio_window`]:
//! classic event-study harness **and** the **event-shock trading vertical** (`EventShock` → gate →
//! filter → align → signal → execution → metrics; see [`EventShockVerticalScan`] and
//! `replay_event_shock` binary).
//!
//! **Substrate vs this crate:** `helio_scan`, `helio_time`, `helio_window` are reusable machinery;
//! `helio_event` is where a concrete **signal-to-trade** path and reporting CLI live. Research-only
//! names ([`TreatmentSelectorScan`], [`CausalEventStudyPipeline`]) may move if the crate splits later.
//!
//! ## Stable vs experimental (API stance)
//!
//! **Stable-ish** for integrators: [`EventShock`], [`EventShockVerticalScan`], [`TradeResult`],
//! [`build_vertical_replay_with_calendar`], [`merge_session_for_shock`],
//! [`validate_bar_sessions_vs_shock_calendar`], replay collectors in [`crate::event_shock_replay`], and
//! the `replay_event_shock` binary.
//!
//! **Experimental / research-heavy** (may move or change): [`CausalEventStudyPipeline`],
//! [`TreatmentSelectorScan`], fold and cluster wiring for classic event studies.
//!
//! The kernel stays in **`helio_scan`**; this crate may use bars, sessions, and causal tagging.

mod event_shock;
mod event_shock_control;
mod event_shock_execution;
mod event_shock_ingest;
mod event_shock_lead;
mod event_shock_metrics;
mod event_shock_replay;
mod event_shock_run_config;
mod event_shock_services;
mod event_shock_strategy;
mod event_shock_vertical;
mod replay_event_shock_cli;
mod fold;
mod pipeline;
mod sampler;
mod selector;
mod types;

pub use event_shock::*;
pub use event_shock_control::*;
pub use event_shock_execution::*;
pub use event_shock_ingest::*;
pub use event_shock_lead::*;
pub use event_shock_metrics::*;
pub use event_shock_replay::*;
pub use event_shock_run_config::*;
pub use event_shock_services::*;
pub use event_shock_strategy::*;
pub use event_shock_vertical::*;
pub use replay_event_shock_cli::*;
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
