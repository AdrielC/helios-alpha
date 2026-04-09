use serde::{Deserialize, Serialize};

use crate::{Anchor, Bounds, Frequency};

/// How pre-binned or to-be-binned data is labeled: frequency + interval shape + alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BucketSpec {
    pub freq: Frequency,
    pub bounds: Bounds,
    pub anchor: Anchor,
}

impl Default for BucketSpec {
    fn default() -> Self {
        Self {
            freq: Frequency::Samples(1),
            bounds: Bounds::LEFT_CLOSED_RIGHT_OPEN,
            anchor: Anchor::Epoch,
        }
    }
}

/// A time interval with explicit bounds semantics (default `[start, end)` via [`Bounds::default`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TimeWindow<T> {
    pub start: T,
    pub end: T,
    pub bounds: Bounds,
}

impl<T> TimeWindow<T> {
    pub fn new(start: T, end: T) -> Self {
        Self {
            start,
            end,
            bounds: Bounds::LEFT_CLOSED_RIGHT_OPEN,
        }
    }
}
