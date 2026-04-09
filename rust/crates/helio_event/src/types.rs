use helio_scan::SessionDate;
use helio_time::Timed;
use helio_window::ClusteredEvent;
use serde::{Deserialize, Serialize};

/// Tag for values that carry explicit availability (re-export pattern).
pub type AvailabilityTagged<T> = Timed<T>;

/// Config for overlap clustering (passed to [`helio_window::EventClusterScan`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverlapConfig {
    pub max_gap_days: i64,
}

impl Default for OverlapConfig {
    fn default() -> Self {
        Self { max_gap_days: 2 }
    }
}

/// Placeholder for matched-control policy knobs.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MatchingConfig {
    pub controls_per_treatment: u32,
}

impl Default for MatchingConfig {
    fn default() -> Self {
        Self {
            controls_per_treatment: 1,
        }
    }
}

/// Scope of inference / labeling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventScope {
    FullSample,
    Session(SessionDate),
}

/// Candidate treatment before clustering (ingest-time).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TreatmentEvent {
    pub id: u32,
    /// Coarse day index for overlap geometry (e.g. event session day).
    pub day: i64,
    pub strength: f64,
    pub horizon_trading_days: u32,
}

/// Control / placebo draw.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ControlEvent {
    pub id: u32,
    pub matched_to_treatment: u32,
    pub session_day: i32,
}

/// Canonical cluster emitted upstream of forward labeling.
pub type EventCluster = ClusteredEvent;

/// Forward window result (complete horizon).
pub type ForwardOutcome = helio_window::ForwardHorizonOutcome;
