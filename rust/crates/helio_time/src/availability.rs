//! **Bucket interval ≠ availability.** A bucket covering \([t, t+\Delta)\) may only enter models at
//! `t + Δ` (bar complete), plus optional latency, next session, etc.
//!
//! Always thread [`crate::AvailableAt`](super::AvailableAt) (or a richer [`crate::Timed`]) alongside
//! bucket membership for causal pipelines.

use crate::AvailableAt;

/// Suggested availability instant when a left-closed right-open bucket \([start, end)\) is **closed**
/// at `end` (event time / bucket end in the same units as `latency`).
#[inline]
pub fn available_at_bucket_close(end_exclusive: i64, latency: i64) -> AvailableAt {
    AvailableAt(end_exclusive.saturating_add(latency))
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
