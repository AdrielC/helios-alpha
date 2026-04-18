//! Deterministic **backtest harness**: wall vs fixed clock, pipeline fingerprints, and bounded
//! datetime ranges for repeatable runs.
//!
//! Use [`BacktestHarness::run`] with a [`FixedClock`] and explicit [`EpochRange`] so two machines
//! produce identical [`BacktestReport`] including [`BacktestReport::fingerprint_hex`].
//!
//! **Kalman:** [`KalmanLocalLevelScan`] is a [`helio_scan::Scan`] + [`helio_scan::SnapshottingScan`]
//! (pause/restart via snapshot). [`fit_local_level_mle`] fits `(q, r)` by **MLE** on the
//! innovation Gaussian likelihood; enable in runs via [`BacktestRunSpec::kalman`].

mod clock;
mod error;
mod fingerprint;
mod harness;
mod kalman;
mod kalman_options;
mod metrics;
mod range;

pub use clock::*;
pub use error::*;
pub use fingerprint::*;
pub use harness::*;
pub use kalman::*;
pub use kalman_options::*;
pub use metrics::*;
pub use range::*;
