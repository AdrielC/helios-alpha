//! **Bucket interval ≠ availability.** A bucket covering \([t, t+\Delta)\) may only enter models at
//! `t + Δ` (bar complete), plus optional latency, next session, etc.
//!
//! Always thread [`crate::AvailableAt`](super::AvailableAt) (or a richer [`crate::Timed`]) alongside
//! bucket membership for causal pipelines.

use crate::bucket::TimeWindow;
use crate::{AvailableAt, Bounds};

/// Wall-clock bucket \([start, start+width)\) in epoch seconds, aligned so `origin` maps to a bucket boundary.
#[inline]
pub fn wall_bucket_interval_wall_secs(ts: i64, width_secs: i64, origin: i64) -> TimeWindow<i64> {
    debug_assert!(width_secs > 0);
    let o = ts.saturating_sub(origin);
    let q = o.div_euclid(width_secs);
    let start = origin.saturating_add(q.saturating_mul(width_secs));
    let end = start.saturating_add(width_secs);
    TimeWindow {
        start,
        end,
        bounds: Bounds::LEFT_CLOSED_RIGHT_OPEN,
    }
}

/// When a left-closed right-open bucket is **complete** at `end_exclusive`, it may enter models at this instant (plus `latency`).
#[inline]
pub fn bucket_close_instant(end_exclusive: i64) -> i64 {
    end_exclusive
}

/// Suggested availability instant when a left-closed right-open bucket \([start, end)\) is **closed**
/// at `end` (event time / bucket end in the same units as `latency`).
#[inline]
pub fn available_at_bucket_close(end_exclusive: i64, latency: i64) -> AvailableAt {
    AvailableAt(bucket_close_instant(end_exclusive).saturating_add(latency))
}

/// Observation tied to a bucket interval **and** when it may be used (causal cut).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BucketTimed<T, I> {
    pub value: T,
    /// Bucket / event interval (meanings depend on upstream; often `[start, end)` in epoch seconds).
    pub interval: crate::bucket::TimeWindow<I>,
    /// When this bucket’s value is knowable for decision-making.
    pub available_at: AvailableAt,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wall_bucket_half_open_alignment() {
        let w = wall_bucket_interval_wall_secs(65, 60, 0);
        assert_eq!(w.start, 60);
        assert_eq!(w.end, 120);
        assert!(w.contains(&65));
        assert!(!w.contains(&120));
    }
}
