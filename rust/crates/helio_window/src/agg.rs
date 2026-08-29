use std::collections::VecDeque;

use helio_stats::OnlineMoments;
use serde::{Deserialize, Serialize};

/// Incremental summary over a window. **Insert-only** path; see [`EvictingWindowAggregator`] when
/// evictions must update the summary (e.g. rolling sum).
pub trait WindowAggregator<T> {
    type Summary: Clone;

    fn insert(&mut self, value: &T);
    fn snapshot(&self) -> Self::Summary;
    fn clear(&mut self);
}

/// Aggregator that can **subtract** the contribution of an evicted element (rolling windows).
pub trait EvictingWindowAggregator<T>: WindowAggregator<T> {
    fn evict(&mut self, value: &T);
}

/// Running sum, count, and mean.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SumCountMeanSummary {
    pub sum: f64,
    pub count: u64,
}

#[derive(Debug, Clone, Default)]
pub struct SumCountMeanAggregator {
    sum: f64,
    count: u64,
}

impl WindowAggregator<f64> for SumCountMeanAggregator {
    type Summary = SumCountMeanSummary;

    fn insert(&mut self, value: &f64) {
        self.sum += *value;
        self.count += 1;
    }

    fn snapshot(&self) -> Self::Summary {
        SumCountMeanSummary {
            sum: self.sum,
            count: self.count,
        }
    }

    fn clear(&mut self) {
        self.sum = 0.0;
        self.count = 0;
    }
}

impl EvictingWindowAggregator<f64> for SumCountMeanAggregator {
    fn evict(&mut self, value: &f64) {
        self.sum -= *value;
        self.count = self.count.saturating_sub(1);
    }
}

impl SumCountMeanSummary {
    pub fn mean(&self) -> Option<f64> {
        if self.count == 0 {
            None
        } else {
            Some(self.sum / self.count as f64)
        }
    }
}

/// Numerically stable rolling moments plus explicit accounting for rejected values.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RollingMomentsSummary {
    pub moments: OnlineMoments,
    pub rejected_non_finite: u64,
    #[serde(default)]
    pub rejected_numerical: u64,
}

#[derive(Debug, Clone, Copy)]
enum RollingMomentsEntry {
    Included(f64),
    NonFinite,
    Numerical,
}

/// Eviction-aware adapter around [`OnlineMoments`].
///
/// `NaN` and infinite inputs remain represented in the enclosing window buffer but are excluded
/// from moments and counted in [`RollingMomentsSummary::rejected_non_finite`]. Finite inputs whose
/// moments cannot be represented are tracked separately in
/// [`RollingMomentsSummary::rejected_numerical`]. Per-sample admission state ensures eviction only
/// removes values that were actually included. Pipelines that must fail closed should validate
/// before this infallible legacy [`WindowAggregator`] seam or use [`crate::BucketReduceScan`], whose
/// reducer errors are explicit outputs.
#[derive(Debug, Clone, Default)]
pub struct RollingMomentsAggregator {
    moments: OnlineMoments,
    rejected_non_finite: u64,
    rejected_numerical: u64,
    entries: VecDeque<RollingMomentsEntry>,
}

impl RollingMomentsAggregator {
    fn rebuild(&mut self) {
        let mut moments = OnlineMoments::new();
        for entry in &mut self.entries {
            if let RollingMomentsEntry::Included(value) = *entry {
                if moments.try_push(value).is_err() {
                    *entry = RollingMomentsEntry::Numerical;
                    self.rejected_numerical = self.rejected_numerical.saturating_add(1);
                }
            }
        }
        self.moments = moments;
    }
}

impl WindowAggregator<f64> for RollingMomentsAggregator {
    type Summary = RollingMomentsSummary;

    fn insert(&mut self, value: &f64) {
        let entry = if !value.is_finite() {
            self.rejected_non_finite = self.rejected_non_finite.saturating_add(1);
            RollingMomentsEntry::NonFinite
        } else if self.moments.try_push(*value).is_ok() {
            RollingMomentsEntry::Included(*value)
        } else {
            self.rejected_numerical = self.rejected_numerical.saturating_add(1);
            RollingMomentsEntry::Numerical
        };
        self.entries.push_back(entry);
    }

    fn snapshot(&self) -> Self::Summary {
        RollingMomentsSummary {
            moments: self.moments,
            rejected_non_finite: self.rejected_non_finite,
            rejected_numerical: self.rejected_numerical,
        }
    }

    fn clear(&mut self) {
        self.moments.clear();
        self.rejected_non_finite = 0;
        self.rejected_numerical = 0;
        self.entries.clear();
    }
}

impl EvictingWindowAggregator<f64> for RollingMomentsAggregator {
    fn evict(&mut self, _value: &f64) {
        match self.entries.pop_front() {
            Some(RollingMomentsEntry::Included(value)) => {
                if self.moments.try_remove(value).is_err() {
                    self.rebuild();
                }
            }
            Some(RollingMomentsEntry::NonFinite) => {
                self.rejected_non_finite = self.rejected_non_finite.saturating_sub(1);
            }
            Some(RollingMomentsEntry::Numerical) => {
                self.rejected_numerical = self.rejected_numerical.saturating_sub(1);
            }
            None => {
                self.moments.clear();
                self.rejected_non_finite = 0;
                self.rejected_numerical = 0;
            }
        }
    }
}
