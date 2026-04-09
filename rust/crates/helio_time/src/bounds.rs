use serde::{Deserialize, Serialize};

/// Whether an endpoint is included in an interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BoundType {
    Open,
    Closed,
}

/// Interval endpoint inclusion. **System default:** [`Bounds::LEFT_CLOSED_RIGHT_OPEN`] (`[start, end)`).
///
/// Left-closed, right-open avoids double-counting at boundaries when tiling the timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Bounds {
    pub left: BoundType,
    pub right: BoundType,
}

impl Bounds {
    /// Canonical default: `[start, end)` — left closed, right open.
    pub const LEFT_CLOSED_RIGHT_OPEN: Self = Self {
        left: BoundType::Closed,
        right: BoundType::Open,
    };

    pub const fn new(left: BoundType, right: BoundType) -> Self {
        Self { left, right }
    }
}

impl Default for Bounds {
    fn default() -> Self {
        Self::LEFT_CLOSED_RIGHT_OPEN
    }
}
