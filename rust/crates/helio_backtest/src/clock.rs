//! Abstraction over **current instant** so backtests never call `std::time` implicitly.

/// Monotonic-ish instant for harness logic (UTC epoch seconds).
pub type EpochSec = i64;

/// Source of "now" for logging, fingerprint metadata, or adaptive logic.
pub trait Clock {
    fn now_epoch_sec(&self) -> EpochSec;
}

/// Wall clock: uses real system time (non-repeatable across machines/runs).
#[derive(Debug, Clone, Copy, Default)]
pub struct WallClock;

impl Clock for WallClock {
    fn now_epoch_sec(&self) -> EpochSec {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }
}

/// Fixed clock for **repeatable** backtests and tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixedClock(pub EpochSec);

impl Clock for FixedClock {
    fn now_epoch_sec(&self) -> EpochSec {
        self.0
    }
}
