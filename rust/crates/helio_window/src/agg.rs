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
