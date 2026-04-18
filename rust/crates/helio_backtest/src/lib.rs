//! Deterministic **backtest harness**: wall vs fixed clock, pipeline fingerprints, and bounded
//! datetime ranges for repeatable runs.
//!
//! Use [`BacktestHarness::run`] with a [`FixedClock`] and explicit [`EpochRange`] so two machines
//! produce identical [`BacktestReport`] including [`BacktestReport::fingerprint_hex`].

mod clock;
mod error;
mod fingerprint;
mod harness;
mod metrics;
mod range;

pub use clock::*;
pub use error::*;
pub use fingerprint::*;
pub use harness::*;
pub use metrics::*;
pub use range::*;
