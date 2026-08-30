//! Domain-free online statistics for ordered, partitioned, and restartable streams.
//!
//! The primary states, [`OnlineMoments`] and [`OnlineCovariance`], support stable one-at-a-time
//! updates and Chan-style parallel merges. Their serialized form is the complete runtime state, so
//! callers can checkpoint them directly. A fixed merge tree is still required for bit-reproducible
//! floating-point results because floating-point addition is not associative.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};
use thiserror::Error;

mod bayes;
mod hawkes;
mod scans;

pub use bayes::*;
pub use hawkes::*;
pub use scans::*;

/// Invalid input or an impossible state transition in an online statistic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum StatsError {
    #[error("online statistic input must be finite")]
    NonFiniteInput,
    #[error("online statistic sample count overflowed u64")]
    CountOverflow,
    #[error("cannot remove a sample from an empty online statistic")]
    EmptyRemoval,
    #[error("online statistic arithmetic produced a non-finite state")]
    NumericalOverflow,
    #[error("online statistic arithmetic produced an invalid negative variance state")]
    NumericalInstability,
    #[error("online statistic snapshot contains invalid state")]
    InvalidSnapshot,
}

/// Mergeable state for count, mean, and the sum of squared deviations from the mean.
///
/// `m2` is `Σ(x - mean)²`. Population variance is `m2 / count`; sample variance is
/// `m2 / (count - 1)`. Keeping this state avoids the catastrophic cancellation of
/// `Σx² - (Σx)²/n` when values are large and variance is small.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct OnlineMoments {
    count: u64,
    mean: f64,
    m2: f64,
}

impl Default for OnlineMoments {
    fn default() -> Self {
        Self::new()
    }
}

impl OnlineMoments {
    pub const fn new() -> Self {
        Self {
            count: 0,
            mean: 0.0,
            m2: 0.0,
        }
    }

    pub const fn count(&self) -> u64 {
        self.count
    }

    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn mean(&self) -> Option<f64> {
        (self.count > 0).then_some(self.mean)
    }

    pub fn sum_of_squared_deviations(&self) -> f64 {
        non_negative_roundoff(self.m2)
    }

    pub fn population_variance(&self) -> Option<f64> {
        (self.count > 0).then(|| non_negative_roundoff(self.m2) / self.count as f64)
    }

    pub fn sample_variance(&self) -> Option<f64> {
        (self.count > 1).then(|| non_negative_roundoff(self.m2) / (self.count - 1) as f64)
    }

    pub fn population_stddev(&self) -> Option<f64> {
        self.population_variance().map(f64::sqrt)
    }

    pub fn sample_stddev(&self) -> Option<f64> {
        self.sample_variance().map(f64::sqrt)
    }

    /// Validate invariants after deserializing state from external storage.
    pub fn validate(&self) -> Result<(), StatsError> {
        if !self.mean.is_finite() || !self.m2.is_finite() || self.m2 < 0.0 {
            return Err(StatsError::InvalidSnapshot);
        }
        if self.count == 0 && (self.mean != 0.0 || self.m2 != 0.0) {
            return Err(StatsError::InvalidSnapshot);
        }
        Ok(())
    }

    /// Add one observation using Welford's stable recurrence.
    pub fn try_push(&mut self, value: f64) -> Result<(), StatsError> {
        require_finite(value)?;
        let next_count = self.count.checked_add(1).ok_or(StatsError::CountOverflow)?;
        let delta = value - self.mean;
        let next_mean = self.mean + delta / next_count as f64;
        let delta2 = value - next_mean;
        let next_m2 = normalize_nonnegative(self.m2 + delta * delta2, self.m2)?;
        require_finite_state(&[next_mean, next_m2])?;

        self.count = next_count;
        self.mean = next_mean;
        self.m2 = next_m2;
        Ok(())
    }

    /// Remove one observation from a rolling state.
    ///
    /// Removal is useful for bounded windows but is less numerically robust than immutable block
    /// merges. Long-lived, high-dynamic-range windows should periodically rebuild from their ring
    /// buffer or use a merge tree of immutable blocks.
    pub fn try_remove(&mut self, value: f64) -> Result<(), StatsError> {
        require_finite(value)?;
        match self.count {
            0 => Err(StatsError::EmptyRemoval),
            1 => {
                *self = Self::new();
                Ok(())
            }
            n => {
                let next_count = n - 1;
                let next_mean = self.mean - (value - self.mean) / next_count as f64;
                let next_m2 = normalize_nonnegative(
                    self.m2 - (value - self.mean) * (value - next_mean),
                    self.m2,
                )?;
                require_finite_state(&[next_mean, next_m2])?;
                self.count = next_count;
                self.mean = next_mean;
                self.m2 = next_m2;
                Ok(())
            }
        }
    }

    /// Merge another independent partial state using the Chan-Golub-LeVeque recurrence.
    pub fn try_merge(&mut self, other: &Self) -> Result<(), StatsError> {
        if other.is_empty() {
            return Ok(());
        }
        if self.is_empty() {
            *self = *other;
            return Ok(());
        }

        let count = self
            .count
            .checked_add(other.count)
            .ok_or(StatsError::CountOverflow)?;
        let delta = other.mean - self.mean;
        let left_weight = self.count as f64;
        let right_weight = other.count as f64;
        let total_weight = count as f64;

        let next_mean = self.mean + delta * right_weight / total_weight;
        let next_m2 = normalize_nonnegative(
            self.m2 + other.m2 + delta * delta * left_weight * right_weight / total_weight,
            self.m2.max(other.m2),
        )?;
        require_finite_state(&[next_mean, next_m2])?;

        self.mean = next_mean;
        self.m2 = next_m2;
        self.count = count;
        Ok(())
    }

    pub fn clear(&mut self) {
        *self = Self::new();
    }
}

/// Deterministically merge moment states in a balanced binary tree.
///
/// The input order defines the tree, making replay repeatable for the same partitioning. A balanced
/// tree also reduces rounding-error growth compared with repeatedly merging every partition into a
/// single accumulator from the left.
pub fn merge_moments_balanced<I>(states: I) -> Result<OnlineMoments, StatsError>
where
    I: IntoIterator<Item = OnlineMoments>,
{
    let mut queue: VecDeque<OnlineMoments> = states.into_iter().collect();
    if queue.is_empty() {
        return Ok(OnlineMoments::new());
    }
    while queue.len() > 1 {
        let mut next = VecDeque::with_capacity(queue.len().div_ceil(2));
        while let Some(mut left) = queue.pop_front() {
            if let Some(right) = queue.pop_front() {
                left.try_merge(&right)?;
            }
            next.push_back(left);
        }
        queue = next;
    }
    Ok(queue.pop_front().unwrap_or_default())
}

/// Mergeable bivariate state for covariance, correlation, and marginal variances.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct OnlineCovariance {
    count: u64,
    mean_x: f64,
    mean_y: f64,
    m2_x: f64,
    m2_y: f64,
    co_moment: f64,
}

impl Default for OnlineCovariance {
    fn default() -> Self {
        Self::new()
    }
}

impl OnlineCovariance {
    pub const fn new() -> Self {
        Self {
            count: 0,
            mean_x: 0.0,
            mean_y: 0.0,
            m2_x: 0.0,
            m2_y: 0.0,
            co_moment: 0.0,
        }
    }

    pub const fn count(&self) -> u64 {
        self.count
    }

    pub fn mean_x(&self) -> Option<f64> {
        (self.count > 0).then_some(self.mean_x)
    }

    pub fn mean_y(&self) -> Option<f64> {
        (self.count > 0).then_some(self.mean_y)
    }

    pub fn try_push(&mut self, x: f64, y: f64) -> Result<(), StatsError> {
        require_finite(x)?;
        require_finite(y)?;
        let next_count = self.count.checked_add(1).ok_or(StatsError::CountOverflow)?;
        let dx = x - self.mean_x;
        let dy = y - self.mean_y;
        let next_mean_x = self.mean_x + dx / next_count as f64;
        let next_mean_y = self.mean_y + dy / next_count as f64;
        let next_m2_x = normalize_nonnegative(self.m2_x + dx * (x - next_mean_x), self.m2_x)?;
        let next_m2_y = normalize_nonnegative(self.m2_y + dy * (y - next_mean_y), self.m2_y)?;
        let next_co_moment = self.co_moment + dx * (y - next_mean_y);
        require_finite_state(&[
            next_mean_x,
            next_mean_y,
            next_m2_x,
            next_m2_y,
            next_co_moment,
        ])?;

        self.count = next_count;
        self.mean_x = next_mean_x;
        self.mean_y = next_mean_y;
        self.m2_x = next_m2_x;
        self.m2_y = next_m2_y;
        self.co_moment = next_co_moment;
        Ok(())
    }

    pub fn try_merge(&mut self, other: &Self) -> Result<(), StatsError> {
        if other.count == 0 {
            return Ok(());
        }
        if self.count == 0 {
            *self = *other;
            return Ok(());
        }
        let count = self
            .count
            .checked_add(other.count)
            .ok_or(StatsError::CountOverflow)?;
        let dx = other.mean_x - self.mean_x;
        let dy = other.mean_y - self.mean_y;
        let left_weight = self.count as f64;
        let right_weight = other.count as f64;
        let cross_weight = left_weight * right_weight / count as f64;

        let next_mean_x = self.mean_x + dx * right_weight / count as f64;
        let next_mean_y = self.mean_y + dy * right_weight / count as f64;
        let next_m2_x = normalize_nonnegative(
            self.m2_x + other.m2_x + dx * dx * cross_weight,
            self.m2_x.max(other.m2_x),
        )?;
        let next_m2_y = normalize_nonnegative(
            self.m2_y + other.m2_y + dy * dy * cross_weight,
            self.m2_y.max(other.m2_y),
        )?;
        let next_co_moment = self.co_moment + other.co_moment + dx * dy * cross_weight;
        require_finite_state(&[
            next_mean_x,
            next_mean_y,
            next_m2_x,
            next_m2_y,
            next_co_moment,
        ])?;

        self.mean_x = next_mean_x;
        self.mean_y = next_mean_y;
        self.m2_x = next_m2_x;
        self.m2_y = next_m2_y;
        self.co_moment = next_co_moment;
        self.count = count;
        Ok(())
    }

    pub fn population_covariance(&self) -> Option<f64> {
        (self.count > 0).then(|| self.co_moment / self.count as f64)
    }

    pub fn sample_covariance(&self) -> Option<f64> {
        (self.count > 1).then(|| self.co_moment / (self.count - 1) as f64)
    }

    pub fn sample_variance_x(&self) -> Option<f64> {
        (self.count > 1).then(|| non_negative_roundoff(self.m2_x) / (self.count - 1) as f64)
    }

    pub fn sample_variance_y(&self) -> Option<f64> {
        (self.count > 1).then(|| non_negative_roundoff(self.m2_y) / (self.count - 1) as f64)
    }

    pub fn sample_correlation(&self) -> Option<f64> {
        let denom = (non_negative_roundoff(self.m2_x) * non_negative_roundoff(self.m2_y)).sqrt();
        (self.count > 1 && denom > 0.0).then(|| (self.co_moment / denom).clamp(-1.0, 1.0))
    }

    /// Validate invariants after deserializing state from external storage.
    pub fn validate(&self) -> Result<(), StatsError> {
        let fields = [
            self.mean_x,
            self.mean_y,
            self.m2_x,
            self.m2_y,
            self.co_moment,
        ];
        if !fields.iter().all(|value| value.is_finite()) || self.m2_x < 0.0 || self.m2_y < 0.0 {
            return Err(StatsError::InvalidSnapshot);
        }
        if self.count == 0 && fields.iter().any(|value| *value != 0.0) {
            return Err(StatsError::InvalidSnapshot);
        }
        Ok(())
    }

    pub fn clear(&mut self) {
        *self = Self::new();
    }
}

fn require_finite(value: f64) -> Result<(), StatsError> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(StatsError::NonFiniteInput)
    }
}

fn require_finite_state(values: &[f64]) -> Result<(), StatsError> {
    values
        .iter()
        .all(|value| value.is_finite())
        .then_some(())
        .ok_or(StatsError::NumericalOverflow)
}

fn normalize_nonnegative(value: f64, scale: f64) -> Result<f64, StatsError> {
    if !value.is_finite() {
        return Err(StatsError::NumericalOverflow);
    }
    if value >= 0.0 {
        return Ok(value);
    }
    let tolerance = f64::EPSILON * 64.0 * scale.abs().max(1.0);
    if value >= -tolerance {
        Ok(0.0)
    } else {
        Err(StatsError::NumericalInstability)
    }
}

fn non_negative_roundoff(value: f64) -> f64 {
    if value < 0.0 && value > -f64::EPSILON * 16.0 {
        0.0
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn moments(values: &[f64]) -> OnlineMoments {
        let mut state = OnlineMoments::new();
        for &value in values {
            state.try_push(value).unwrap();
        }
        state
    }

    fn close(a: f64, b: f64, tolerance: f64) {
        assert!((a - b).abs() <= tolerance, "{a} != {b}");
    }

    #[test]
    fn known_sample_variance() {
        let state = moments(&[2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0]);
        assert_eq!(state.count(), 8);
        close(state.mean().unwrap(), 5.0, 1e-15);
        close(state.population_variance().unwrap(), 4.0, 1e-15);
        close(state.sample_variance().unwrap(), 32.0 / 7.0, 1e-15);
    }

    #[test]
    fn stable_when_offset_is_large() {
        let state = moments(&[1e12 + 1.0, 1e12 + 2.0, 1e12 + 3.0, 1e12 + 4.0]);
        close(state.sample_variance().unwrap(), 5.0 / 3.0, 1e-12);
    }

    #[test]
    fn balanced_partition_merge_matches_sequential() {
        let values: Vec<f64> = (0..10_003)
            .map(|i| 1e9 + (i as f64 * 0.017).sin() * 0.25)
            .collect();
        let sequential = moments(&values);
        let partials = values.chunks(113).map(moments);
        let merged = merge_moments_balanced(partials).unwrap();
        assert_eq!(merged.count(), sequential.count());
        close(merged.mean().unwrap(), sequential.mean().unwrap(), 1e-6);
        close(
            merged.sample_variance().unwrap(),
            sequential.sample_variance().unwrap(),
            1e-8,
        );
    }

    #[test]
    fn removal_matches_rebuild() {
        let mut rolling = moments(&[10.0, 20.0, 30.0, 40.0]);
        rolling.try_remove(10.0).unwrap();
        let rebuilt = moments(&[20.0, 30.0, 40.0]);
        close(rolling.mean().unwrap(), rebuilt.mean().unwrap(), 1e-14);
        close(
            rolling.sample_variance().unwrap(),
            rebuilt.sample_variance().unwrap(),
            1e-12,
        );
    }

    #[test]
    fn rejects_non_finite_inputs_without_mutation() {
        let mut state = moments(&[1.0, 2.0]);
        let before = state;
        assert_eq!(state.try_push(f64::NAN), Err(StatsError::NonFiniteInput));
        assert_eq!(state, before);
    }

    #[test]
    fn rejects_arithmetic_overflow_without_mutation() {
        let mut state = moments(&[f64::MAX]);
        let before = state;
        assert_eq!(
            state.try_push(-f64::MAX),
            Err(StatsError::NumericalOverflow)
        );
        assert_eq!(state, before);

        let mut covariance = OnlineCovariance::new();
        covariance.try_push(f64::MAX, f64::MAX).unwrap();
        let before = covariance;
        assert_eq!(
            covariance.try_push(-f64::MAX, -f64::MAX),
            Err(StatsError::NumericalOverflow)
        );
        assert_eq!(covariance, before);
    }

    #[test]
    fn covariance_and_merge_match_perfect_linear_relation() {
        let mut all = OnlineCovariance::new();
        let mut left = OnlineCovariance::new();
        let mut right = OnlineCovariance::new();
        for i in 0..1000 {
            let x = i as f64 * 0.5;
            let y = 3.0 * x - 7.0;
            all.try_push(x, y).unwrap();
            if i < 437 {
                left.try_push(x, y).unwrap();
            } else {
                right.try_push(x, y).unwrap();
            }
        }
        left.try_merge(&right).unwrap();
        close(
            left.sample_covariance().unwrap(),
            all.sample_covariance().unwrap(),
            1e-9,
        );
        close(left.sample_correlation().unwrap(), 1.0, 1e-14);
    }

    #[test]
    fn serde_round_trip_is_exact() {
        let state = moments(&[1.25, 5.5, -9.0, 12.0]);
        let encoded = serde_json::to_vec(&state).unwrap();
        let restored: OnlineMoments = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(restored, state);
    }
}
