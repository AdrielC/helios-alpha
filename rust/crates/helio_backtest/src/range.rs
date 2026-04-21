use serde::{Deserialize, Serialize};

use crate::{BacktestError, Result};

/// Inclusive start and inclusive end in **UTC epoch seconds** (datetime range for the run).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpochRange {
    pub start_epoch_sec: i64,
    pub end_epoch_sec: i64,
}

impl EpochRange {
    pub fn new(start_epoch_sec: i64, end_epoch_sec: i64) -> Result<Self> {
        if start_epoch_sec > end_epoch_sec {
            return Err(BacktestError::InvalidEpochRange(
                start_epoch_sec,
                end_epoch_sec,
            ));
        }
        Ok(Self {
            start_epoch_sec,
            end_epoch_sec,
        })
    }

    /// Width in seconds (0 when start == end).
    #[inline]
    pub fn span_secs(&self) -> u64 {
        (self.end_epoch_sec - self.start_epoch_sec).max(0) as u64
    }
}
