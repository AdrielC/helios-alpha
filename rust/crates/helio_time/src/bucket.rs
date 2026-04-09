use serde::{Deserialize, Serialize};

use crate::{Anchor, BoundType, Bounds, Frequency};

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

    /// Membership for this window’s [`Bounds`] (same semantics as [`crate::half_open::HalfOpenRange`]
    /// when `bounds` is [`Bounds::LEFT_CLOSED_RIGHT_OPEN`]).
    pub fn contains(&self, x: &T) -> bool
    where
        T: PartialOrd,
    {
        let ge_start = match self.bounds.left {
            BoundType::Closed => *x >= self.start,
            BoundType::Open => *x > self.start,
        };
        let lt_end = match self.bounds.right {
            BoundType::Closed => *x <= self.end,
            BoundType::Open => *x < self.end,
        };
        ge_start && lt_end
    }

    /// Half-open overlap: `[a,b) ∩ [c,d) ≠ ∅` iff `a < d && c < b`. Intended when both windows use
    /// [`Bounds::LEFT_CLOSED_RIGHT_OPEN`] (the default from [`Self::new`]).
    pub fn overlaps(&self, other: &Self) -> bool
    where
        T: PartialOrd,
    {
        self.start < other.end && other.start < self.end
    }
}

impl<T> From<crate::half_open::HalfOpenRange<T>> for TimeWindow<T> {
    fn from(r: crate::half_open::HalfOpenRange<T>) -> Self {
        Self {
            start: r.start,
            end: r.end,
            bounds: Bounds::LEFT_CLOSED_RIGHT_OPEN,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_window_contains_half_open() {
        let w = TimeWindow::new(0i32, 10);
        assert!(w.contains(&0));
        assert!(w.contains(&5));
        assert!(!w.contains(&10));
    }

    #[test]
    fn half_open_range_converts_to_time_window() {
        let r = crate::half_open::HalfOpenRange::try_new(1u8, 3).unwrap();
        let w: TimeWindow<_> = r.into();
        assert!(w.contains(&1));
        assert!(!w.contains(&3));
    }
}
