//! **Time-keyed** trailing windows: keep samples whose sort key lies in `[now - Δ, now)` (half-open),
//! for `WindowSpec::Trailing` with `Frequency::Fixed` only.
//!
//! Sample-count and calendar/session `Frequency` variants are **not** handled here — use
//! [`crate::WindowState`] / [`crate::RollingAggregatorScan`] for samples, or
//! [`crate::session_keyed`] for session-index eviction.

use helio_time::{FixedStep, FixedUnit, Frequency, WindowSpec};
use std::collections::VecDeque;

use crate::agg::EvictingWindowAggregator;

/// Wall-time or event-time key in the **same unit** as `WindowSpec` fixed steps (typically UTC epoch seconds).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TimeKey(pub i64);

#[derive(Debug, Clone)]
pub struct TimeKeyedSample<T> {
    pub key: TimeKey,
    pub value: T,
}

fn fixed_step_duration_secs(step: FixedStep) -> Option<i64> {
    let n = i64::from(step.n);
    let d = match step.unit {
        FixedUnit::Second => n,
        FixedUnit::Minute => n.checked_mul(60)?,
        FixedUnit::Hour => n.checked_mul(3600)?,
        FixedUnit::Day => n.checked_mul(86_400)?,
        FixedUnit::Week => n.checked_mul(7 * 86_400)?,
    };
    Some(d)
}

/// Returns trailing span in seconds for `spec` when it is `Trailing { size: Fixed(..), .. }`.
pub fn trailing_fixed_window_span_secs(spec: WindowSpec) -> Option<i64> {
    match spec {
        WindowSpec::Trailing { size, .. } => match size {
            Frequency::Fixed(fs) => fixed_step_duration_secs(fs),
            _ => None,
        },
        _ => None,
    }
}

/// Ring buffer with **time-keyed** eviction: after each push at `key`, drops front while `front.key < key.0 - span_secs`.
#[derive(Debug, Clone)]
pub struct TimeKeyedWindowState<T, A> {
    span_secs: i64,
    deque: VecDeque<TimeKeyedSample<T>>,
    agg: A,
}

impl<T: Clone, A: EvictingWindowAggregator<T>> TimeKeyedWindowState<T, A> {
    pub fn new(spec: WindowSpec, agg: A) -> Option<Self> {
        let span = trailing_fixed_window_span_secs(spec)?;
        if span <= 0 {
            return None;
        }
        Some(Self {
            span_secs: span,
            deque: VecDeque::new(),
            agg,
        })
    }

    pub fn span_secs(&self) -> i64 {
        self.span_secs
    }

    pub fn push(&mut self, key: TimeKey, value: T) {
        let cutoff = key.0.saturating_sub(self.span_secs);
        while let Some(front) = self.deque.front() {
            if front.key.0 >= cutoff {
                break;
            }
            let old = self.deque.pop_front().expect("front exists");
            self.agg.evict(&old.value);
        }
        self.deque.push_back(TimeKeyedSample { key, value: value.clone() });
        self.agg.insert(&value);
    }

    pub fn summary(&self) -> A::Summary {
        self.agg.snapshot()
    }

    pub fn len(&self) -> usize {
        self.deque.len()
    }

    pub fn clear(&mut self) {
        self.deque.clear();
        self.agg.clear();
    }

    /// Ordered oldest→newest for checkpoint restore.
    pub fn entries(&self) -> Vec<(i64, T)>
    where
        T: Clone,
    {
        self.deque
            .iter()
            .map(|s| (s.key.0, s.value.clone()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agg::SumCountMeanAggregator;
    use helio_time::Bounds;

    #[test]
    fn evicts_by_time_not_count() {
        let spec = WindowSpec::Trailing {
            size: Frequency::Fixed(FixedStep {
                n: 10,
                unit: FixedUnit::Second,
            }),
            bounds: Bounds::LEFT_CLOSED_RIGHT_OPEN,
        };
        let mut w = TimeKeyedWindowState::new(spec, SumCountMeanAggregator::default()).unwrap();
        w.push(TimeKey(100), 1.0);
        w.push(TimeKey(105), 2.0);
        assert_eq!(w.len(), 2);
        w.push(TimeKey(112), 3.0);
        // 100 < 112-10 => evicted
        assert_eq!(w.len(), 2);
        let s = w.summary();
        assert_eq!(s.count, 2);
        assert!((s.sum - 5.0).abs() < 1e-9);
    }
}
