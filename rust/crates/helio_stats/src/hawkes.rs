//! O(1) exponential-kernel Hawkes intensity for clustered point events.
//!
//! This is an online state primitive, not a fitting procedure and not evidence of predictability.
//! Fit parameters out of sample, account for marks and regime changes, and validate residuals
//! before interpreting the resulting conditional intensity.

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Error)]
pub enum HawkesError {
    #[error("Hawkes parameters must be finite")]
    NonFiniteParameter,
    #[error("Hawkes baseline and jump must be non-negative and decay must be positive")]
    InvalidParameterRange,
    #[error("unmarked Hawkes branching ratio jump/decay must be below one")]
    NonStationaryUnmarkedKernel,
    #[error("event timestamp regressed from {previous} to {attempted}")]
    RegressedTime { attempted: i64, previous: i64 },
    #[error("event mark must be finite and non-negative")]
    InvalidMark,
    #[error("Hawkes event count overflowed u64")]
    CountOverflow,
    #[error("Hawkes arithmetic produced a non-finite intensity")]
    NumericalOverflow,
    #[error("Hawkes snapshot contains invalid state")]
    InvalidSnapshot,
}

/// `λ(t) = baseline + Σ jump * mark_i * exp(-decay * (t - t_i))`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ExponentialHawkes {
    baseline: f64,
    jump: f64,
    decay: f64,
}

impl ExponentialHawkes {
    /// Construct an unmarked-stationary exponential kernel.
    ///
    /// The check `jump / decay < 1` is sufficient for unit marks. For marked processes the actual
    /// stability condition also depends on the mark distribution.
    pub fn try_new(baseline: f64, jump: f64, decay: f64) -> Result<Self, HawkesError> {
        if !baseline.is_finite() || !jump.is_finite() || !decay.is_finite() {
            return Err(HawkesError::NonFiniteParameter);
        }
        if baseline < 0.0 || jump < 0.0 || decay <= 0.0 {
            return Err(HawkesError::InvalidParameterRange);
        }
        if jump / decay >= 1.0 {
            return Err(HawkesError::NonStationaryUnmarkedKernel);
        }
        Ok(Self {
            baseline,
            jump,
            decay,
        })
    }

    pub const fn baseline(&self) -> f64 {
        self.baseline
    }

    pub const fn jump(&self) -> f64 {
        self.jump
    }

    pub const fn decay(&self) -> f64 {
        self.decay
    }

    pub fn branching_ratio_unmarked(&self) -> f64 {
        self.jump / self.decay
    }

    pub const fn init(&self) -> HawkesState {
        HawkesState::new()
    }

    /// Conditional intensity immediately before `timestamp`, without changing state.
    pub fn try_intensity_at(
        &self,
        state: &HawkesState,
        timestamp: i64,
    ) -> Result<f64, HawkesError> {
        let excitation = self.decayed_excitation(state, timestamp)?;
        Ok(self.baseline + excitation)
    }

    /// Advance to one marked event and return pre-event and post-event intensities.
    pub fn try_observe(
        &self,
        state: &mut HawkesState,
        timestamp: i64,
        mark: f64,
    ) -> Result<HawkesUpdate, HawkesError> {
        if !mark.is_finite() || mark < 0.0 {
            return Err(HawkesError::InvalidMark);
        }
        let excitation = self.decayed_excitation(state, timestamp)?;
        let next_count = state
            .event_count
            .checked_add(1)
            .ok_or(HawkesError::CountOverflow)?;
        let pre_event_intensity = self.baseline + excitation;
        let post_event_excitation = excitation + self.jump * mark;
        let post_event_intensity = self.baseline + post_event_excitation;
        if !pre_event_intensity.is_finite()
            || !post_event_excitation.is_finite()
            || !post_event_intensity.is_finite()
        {
            return Err(HawkesError::NumericalOverflow);
        }

        state.last_time = Some(timestamp);
        state.excitation = post_event_excitation;
        state.event_count = next_count;
        Ok(HawkesUpdate {
            timestamp,
            mark,
            pre_event_intensity,
            post_event_intensity,
            event_count: next_count,
        })
    }

    fn decayed_excitation(&self, state: &HawkesState, timestamp: i64) -> Result<f64, HawkesError> {
        let Some(previous) = state.last_time else {
            return Ok(0.0);
        };
        if timestamp < previous {
            return Err(HawkesError::RegressedTime {
                attempted: timestamp,
                previous,
            });
        }
        let elapsed = timestamp.saturating_sub(previous) as f64;
        let excitation = state.excitation * (-self.decay * elapsed).exp();
        if excitation.is_finite() {
            Ok(excitation)
        } else {
            Err(HawkesError::NumericalOverflow)
        }
    }
}

/// Complete checkpoint state for an exponential Hawkes filter.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct HawkesState {
    pub last_time: Option<i64>,
    pub excitation: f64,
    pub event_count: u64,
}

impl Default for HawkesState {
    fn default() -> Self {
        Self::new()
    }
}

impl HawkesState {
    pub const fn new() -> Self {
        Self {
            last_time: None,
            excitation: 0.0,
            event_count: 0,
        }
    }

    /// Validate invariants after deserializing state from external storage.
    pub fn validate(&self) -> Result<(), HawkesError> {
        if !self.excitation.is_finite() || self.excitation < 0.0 {
            return Err(HawkesError::InvalidSnapshot);
        }
        match (self.last_time, self.event_count, self.excitation) {
            (None, 0, 0.0) | (Some(_), 1.., _) => Ok(()),
            _ => Err(HawkesError::InvalidSnapshot),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct HawkesUpdate {
    pub timestamp: i64,
    pub mark: f64,
    pub pre_event_intensity: f64,
    pub post_event_intensity: f64,
    pub event_count: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(left: f64, right: f64) {
        assert!((left - right).abs() <= 1e-12, "{left} != {right}");
    }

    #[test]
    fn clustered_events_raise_conditional_intensity() {
        let model = ExponentialHawkes::try_new(0.1, 0.4, 1.0).unwrap();
        let mut state = model.init();
        let first = model.try_observe(&mut state, 0, 1.0).unwrap();
        close(first.pre_event_intensity, 0.1);
        close(first.post_event_intensity, 0.5);

        let second = model.try_observe(&mut state, 0, 1.0).unwrap();
        close(second.pre_event_intensity, 0.5);
        close(second.post_event_intensity, 0.9);
    }

    #[test]
    fn excitation_decays_toward_baseline() {
        let model = ExponentialHawkes::try_new(0.2, 0.4, 0.5).unwrap();
        let mut state = model.init();
        model.try_observe(&mut state, 100, 1.0).unwrap();
        let later = model.try_intensity_at(&state, 120).unwrap();
        close(later, 0.2 + 0.4 * (-10.0f64).exp());
    }

    #[test]
    fn rejected_time_regression_does_not_mutate_state() {
        let model = ExponentialHawkes::try_new(0.2, 0.1, 1.0).unwrap();
        let mut state = model.init();
        model.try_observe(&mut state, 100, 1.0).unwrap();
        let before = state;
        assert!(matches!(
            model.try_observe(&mut state, 99, 1.0),
            Err(HawkesError::RegressedTime { .. })
        ));
        assert_eq!(state, before);
    }

    #[test]
    fn rejects_non_stationary_unmarked_kernel() {
        assert_eq!(
            ExponentialHawkes::try_new(0.1, 1.0, 1.0),
            Err(HawkesError::NonStationaryUnmarkedKernel)
        );
    }

    #[test]
    fn rejects_intensity_overflow_without_mutation() {
        let model = ExponentialHawkes::try_new(0.1, 0.9, 1.0).unwrap();
        let mut state = model.init();
        model.try_observe(&mut state, 0, f64::MAX).unwrap();
        let before = state;
        assert_eq!(
            model.try_observe(&mut state, 0, f64::MAX),
            Err(HawkesError::NumericalOverflow)
        );
        assert_eq!(state, before);
    }

    #[test]
    fn serde_checkpoint_resume_is_exact() {
        let model = ExponentialHawkes::try_new(0.2, 0.1, 1.0).unwrap();
        let mut state = model.init();
        model.try_observe(&mut state, 100, 1.0).unwrap();
        let encoded = serde_json::to_vec(&state).unwrap();
        let mut restored: HawkesState = serde_json::from_slice(&encoded).unwrap();

        let continuous = model.try_observe(&mut state, 110, 2.0).unwrap();
        let resumed = model.try_observe(&mut restored, 110, 2.0).unwrap();
        assert_eq!(continuous, resumed);
        assert_eq!(state, restored);
    }
}
