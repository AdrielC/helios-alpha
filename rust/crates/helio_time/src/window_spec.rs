use serde::{Deserialize, Serialize};

use crate::{Bounds, Frequency};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WindowDirection {
    Trailing,
    Leading,
    Centered,
}

/// High-level rolling / horizon window description.
///
/// - **Sample-count** [`Frequency::Samples`] — used by `helio_window` ring-buffer rolling (`WindowState`, `RollingAggregatorScan`).
/// - **Fixed wall span** [`Frequency::Fixed`] on a **trailing** spec — time-keyed eviction in `helio_window::time_keyed`.
/// - **Session / calendar** frequencies — semantic categories; session-keyed eviction uses `helio_window::session_keyed` (not implicit in `WindowState`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WindowSpec {
    Trailing { size: Frequency, bounds: Bounds },
    Leading { size: Frequency, bounds: Bounds },
    Centered { size: Frequency, bounds: Bounds },
}

impl WindowSpec {
    pub fn trailing_samples(n: u32) -> Self {
        Self::Trailing {
            size: Frequency::Samples(n),
            bounds: Bounds::LEFT_CLOSED_RIGHT_OPEN,
        }
    }

    /// Capacity for a ring buffer when `size` is [`Frequency::Samples`]; otherwise `None`.
    pub fn sample_capacity(&self) -> Option<usize> {
        let freq = match self {
            WindowSpec::Trailing { size, .. }
            | WindowSpec::Leading { size, .. }
            | WindowSpec::Centered { size, .. } => size,
        };
        freq.as_samples().map(|n| n as usize)
    }

    pub fn bounds(&self) -> Bounds {
        match self {
            WindowSpec::Trailing { bounds, .. }
            | WindowSpec::Leading { bounds, .. }
            | WindowSpec::Centered { bounds, .. } => *bounds,
        }
    }
}
