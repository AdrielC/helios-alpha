//! Guarded floating-point building blocks for restartable streaming computation.
//!
//! These types make the numerical policy part of pipeline state. They reject non-finite input,
//! keep failed updates atomic, validate restored snapshots, and preserve the extra state required
//! by compensated or scaled algorithms.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Failure from a guarded numerical primitive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum NumericalError {
    #[error("numerical input must be finite")]
    NonFiniteInput,
    #[error("numerical operation exceeded the finite f64 range")]
    Overflow,
    #[error("numerical operation produced zero from a non-zero exact magnitude")]
    Underflow,
    #[error("probability must be finite and between zero and one")]
    InvalidProbability,
    #[error("log probability must be finite and non-positive, or negative infinity for zero")]
    InvalidLogProbability,
    #[error("numerical snapshot contains invalid state")]
    InvalidSnapshot,
}

/// Neumaier-compensated streaming sum.
///
/// Neumaier's variant of Kahan summation also recovers the small term in sequences such as
/// `[1e16, 1, -1e16]`. Both the running sum and correction are serialized because dropping the
/// correction at a checkpoint would change the result after restart.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct CompensatedSum {
    sum: f64,
    correction: f64,
}

impl CompensatedSum {
    pub const fn new() -> Self {
        Self {
            sum: 0.0,
            correction: 0.0,
        }
    }

    pub const fn raw_sum(&self) -> f64 {
        self.sum
    }

    pub const fn correction(&self) -> f64 {
        self.correction
    }

    /// Return the corrected sum, rejecting an unrepresentable result.
    pub fn try_total(&self) -> Result<f64, NumericalError> {
        let total = self.sum + self.correction;
        if total.is_finite() {
            Ok(total)
        } else {
            Err(NumericalError::Overflow)
        }
    }

    /// Validate state restored from an external checkpoint.
    pub fn validate(&self) -> Result<(), NumericalError> {
        if !self.sum.is_finite() || !self.correction.is_finite() {
            return Err(NumericalError::InvalidSnapshot);
        }
        self.try_total()
            .map(|_| ())
            .map_err(|_| NumericalError::InvalidSnapshot)
    }

    /// Add one finite value. A rejected update leaves the accumulator unchanged.
    pub fn try_push(&mut self, value: f64) -> Result<(), NumericalError> {
        let (sum, correction) = checked_neumaier_add(self.sum, self.correction, value)?;
        self.sum = sum;
        self.correction = correction;
        Ok(())
    }

    /// Merge another accumulator without discarding its compensation term.
    pub fn try_merge(&mut self, other: &Self) -> Result<(), NumericalError> {
        self.validate()?;
        other.validate()?;
        let mut next = *self;
        next.try_push(other.sum)?;
        next.try_push(other.correction)?;
        *self = next;
        Ok(())
    }

    pub fn clear(&mut self) {
        *self = Self::new();
    }
}

fn checked_neumaier_add(
    sum: f64,
    correction: f64,
    value: f64,
) -> Result<(f64, f64), NumericalError> {
    if !value.is_finite() {
        return Err(NumericalError::NonFiniteInput);
    }
    let next_sum = sum + value;
    if !next_sum.is_finite() {
        return Err(NumericalError::Overflow);
    }
    let correction_delta = if sum.abs() >= value.abs() {
        (sum - next_sum) + value
    } else {
        (value - next_sum) + sum
    };
    let next_correction = correction + correction_delta;
    if !next_correction.is_finite() || !(next_sum + next_correction).is_finite() {
        return Err(NumericalError::Overflow);
    }
    Ok((next_sum, next_correction))
}

/// Scaled sum of squares using the LAPACK `xLASSQ` recurrence.
///
/// The state represents `scale² * scaled_sum`. Keeping the largest magnitude outside the squared
/// accumulator prevents intermediate overflow for large values and underflow for small values.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct ScaledSumSquares {
    scale: f64,
    scaled_sum: f64,
}

impl ScaledSumSquares {
    pub const fn new() -> Self {
        Self {
            scale: 0.0,
            scaled_sum: 0.0,
        }
    }

    pub const fn scale(&self) -> f64 {
        self.scale
    }

    pub const fn scaled_sum(&self) -> f64 {
        self.scaled_sum
    }

    pub const fn is_empty(&self) -> bool {
        self.scale == 0.0
    }

    pub fn validate(&self) -> Result<(), NumericalError> {
        let empty = self.scale == 0.0 && self.scaled_sum == 0.0;
        let populated = self.scale.is_finite()
            && self.scale > 0.0
            && self.scaled_sum.is_finite()
            && self.scaled_sum >= 1.0;
        if empty || populated {
            Ok(())
        } else {
            Err(NumericalError::InvalidSnapshot)
        }
    }

    /// Add one value. A rejected update leaves the accumulator unchanged.
    pub fn try_push(&mut self, value: f64) -> Result<(), NumericalError> {
        if !value.is_finite() {
            return Err(NumericalError::NonFiniteInput);
        }
        let magnitude = value.abs();
        if magnitude == 0.0 {
            return Ok(());
        }
        let (scale, scaled_sum) = if self.scale == 0.0 {
            (magnitude, 1.0)
        } else if self.scale < magnitude {
            let ratio = self.scale / magnitude;
            (magnitude, 1.0 + self.scaled_sum * ratio * ratio)
        } else {
            let ratio = magnitude / self.scale;
            (self.scale, self.scaled_sum + ratio * ratio)
        };
        if !scale.is_finite() || !scaled_sum.is_finite() {
            return Err(NumericalError::Overflow);
        }
        self.scale = scale;
        self.scaled_sum = scaled_sum;
        Ok(())
    }

    /// Merge an independent partial state with the same scaled recurrence.
    pub fn try_merge(&mut self, other: &Self) -> Result<(), NumericalError> {
        self.validate()?;
        other.validate()?;
        if other.is_empty() {
            return Ok(());
        }
        if self.is_empty() {
            *self = *other;
            return Ok(());
        }
        let (scale, scaled_sum) = if self.scale < other.scale {
            let ratio = self.scale / other.scale;
            (
                other.scale,
                other.scaled_sum + self.scaled_sum * ratio * ratio,
            )
        } else {
            let ratio = other.scale / self.scale;
            (
                self.scale,
                self.scaled_sum + other.scaled_sum * ratio * ratio,
            )
        };
        if !scaled_sum.is_finite() {
            return Err(NumericalError::Overflow);
        }
        self.scale = scale;
        self.scaled_sum = scaled_sum;
        Ok(())
    }

    /// Stable Euclidean norm. This remains representable for many inputs whose naive squares do not.
    pub fn try_norm(&self) -> Result<f64, NumericalError> {
        self.validate()?;
        if self.is_empty() {
            return Ok(0.0);
        }
        let root = self.scaled_sum.sqrt();
        if self.scale > f64::MAX / root {
            return Err(NumericalError::Overflow);
        }
        Ok(self.scale * root)
    }

    /// Sum of squares when that final quantity is representable.
    pub fn try_sum_squares(&self) -> Result<f64, NumericalError> {
        let norm = self.try_norm()?;
        if norm > f64::MAX.sqrt() {
            return Err(NumericalError::Overflow);
        }
        let sum_squares = norm * norm;
        if norm != 0.0 && sum_squares == 0.0 {
            Err(NumericalError::Underflow)
        } else {
            Ok(sum_squares)
        }
    }

    pub fn clear(&mut self) {
        *self = Self::new();
    }
}

/// Probability stored in natural-log space.
///
/// Products become additions, so long conditional chains retain information after their ordinary
/// `f64` probability has underflowed to zero. Negative infinity is the canonical representation of
/// an exact zero probability.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LogProbability {
    ln_probability: f64,
}

impl LogProbability {
    pub const fn one() -> Self {
        Self {
            ln_probability: 0.0,
        }
    }

    pub const fn zero() -> Self {
        Self {
            ln_probability: f64::NEG_INFINITY,
        }
    }

    pub fn try_from_probability(probability: f64) -> Result<Self, NumericalError> {
        if !probability.is_finite() || !(0.0..=1.0).contains(&probability) {
            return Err(NumericalError::InvalidProbability);
        }
        if probability == 0.0 {
            Ok(Self::zero())
        } else {
            Ok(Self {
                ln_probability: probability.ln(),
            })
        }
    }

    pub fn try_from_ln(ln_probability: f64) -> Result<Self, NumericalError> {
        let valid_zero = ln_probability == f64::NEG_INFINITY;
        let valid_nonzero = ln_probability.is_finite() && ln_probability <= 0.0;
        if valid_zero || valid_nonzero {
            Ok(Self { ln_probability })
        } else {
            Err(NumericalError::InvalidLogProbability)
        }
    }

    pub const fn ln_probability(&self) -> f64 {
        self.ln_probability
    }

    /// Ordinary probability for display or compatible APIs. This may round to zero; retain the log
    /// value for comparisons and downstream probability algebra.
    pub fn probability(&self) -> f64 {
        self.ln_probability.exp()
    }

    pub fn validate(&self) -> Result<(), NumericalError> {
        Self::try_from_ln(self.ln_probability)
            .map(|_| ())
            .map_err(|_| NumericalError::InvalidSnapshot)
    }

    pub fn try_product(self, other: Self) -> Result<Self, NumericalError> {
        self.validate()?;
        other.validate()?;
        if self.ln_probability == f64::NEG_INFINITY || other.ln_probability == f64::NEG_INFINITY {
            return Ok(Self::zero());
        }
        let ln_probability = self.ln_probability + other.ln_probability;
        if ln_probability.is_finite() {
            Ok(Self { ln_probability })
        } else {
            Err(NumericalError::Overflow)
        }
    }

    pub fn try_multiply_probability(self, probability: f64) -> Result<Self, NumericalError> {
        self.try_product(Self::try_from_probability(probability)?)
    }
}

impl Default for LogProbability {
    fn default() -> Self {
        Self::one()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn relative_close(left: f64, right: f64, tolerance: f64) {
        let scale = left.abs().max(right.abs()).max(f64::MIN_POSITIVE);
        assert!(
            (left - right).abs() <= tolerance * scale,
            "{left} != {right}"
        );
    }

    #[test]
    fn compensated_sum_recovers_small_term_across_cancellation() {
        let mut sum = CompensatedSum::new();
        for value in [1e16, 1.0, -1e16] {
            sum.try_push(value).unwrap();
        }
        assert_eq!(sum.try_total().unwrap(), 1.0);
    }

    #[test]
    fn compensated_sum_preserves_checkpoint_and_merge_state() {
        let mut left = CompensatedSum::new();
        let mut right = CompensatedSum::new();
        left.try_push(1e16).unwrap();
        left.try_push(1.0).unwrap();
        right.try_push(-1e16).unwrap();
        let restored: CompensatedSum =
            serde_json::from_slice(&serde_json::to_vec(&left).unwrap()).unwrap();
        restored.validate().unwrap();
        let mut merged = restored;
        merged.try_merge(&right).unwrap();
        assert_eq!(merged.try_total().unwrap(), 1.0);
    }

    #[test]
    fn compensated_sum_rejection_is_atomic() {
        let mut sum = CompensatedSum::new();
        sum.try_push(f64::MAX).unwrap();
        let before = sum;
        assert_eq!(sum.try_push(f64::MAX), Err(NumericalError::Overflow));
        assert_eq!(sum, before);
        assert_eq!(sum.try_push(f64::NAN), Err(NumericalError::NonFiniteInput));
        assert_eq!(sum, before);
    }

    #[test]
    fn scaled_norm_avoids_intermediate_overflow_and_underflow() {
        let mut large = ScaledSumSquares::new();
        large.try_push(3e200).unwrap();
        large.try_push(4e200).unwrap();
        relative_close(large.try_norm().unwrap(), 5e200, 2e-15);
        assert_eq!(large.try_sum_squares(), Err(NumericalError::Overflow));

        let mut small = ScaledSumSquares::new();
        small.try_push(3e-200).unwrap();
        small.try_push(4e-200).unwrap();
        relative_close(small.try_norm().unwrap(), 5e-200, 2e-15);
        assert_eq!(small.try_sum_squares(), Err(NumericalError::Underflow));
    }

    #[test]
    fn scaled_sum_squares_merges_partial_states() {
        let mut left = ScaledSumSquares::new();
        let mut right = ScaledSumSquares::new();
        left.try_push(3.0).unwrap();
        right.try_push(4.0).unwrap();
        left.try_merge(&right).unwrap();
        assert_eq!(left.try_norm().unwrap(), 5.0);
        assert_eq!(left.try_sum_squares().unwrap(), 25.0);
    }

    #[test]
    fn log_probability_preserves_an_underflowed_conditional_chain() {
        let first = LogProbability::try_from_probability(1e-200).unwrap();
        let product = first.try_product(first).unwrap();
        assert_eq!(product.probability(), 0.0);
        assert!(product.ln_probability().is_finite());
        relative_close(product.ln_probability(), 2.0 * 1e-200f64.ln(), 2e-15);
    }

    #[test]
    fn log_probability_rejects_invalid_input_and_snapshot() {
        assert_eq!(
            LogProbability::try_from_probability(f64::NAN),
            Err(NumericalError::InvalidProbability)
        );
        assert_eq!(
            LogProbability::try_from_probability(1.1),
            Err(NumericalError::InvalidProbability)
        );
        let invalid = LogProbability {
            ln_probability: f64::INFINITY,
        };
        assert_eq!(invalid.validate(), Err(NumericalError::InvalidSnapshot));
    }
}
