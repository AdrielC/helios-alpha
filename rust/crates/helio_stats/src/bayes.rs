//! Mergeable Bayesian sufficient statistics and replayable Thompson decisions.
//!
//! These types are domain-free mechanisms. They do not define an objective, a constraint, or an
//! execution policy. Callers inject those choices and persist the sampler version with their
//! pipeline fingerprint.

use helio_scan::Emit;
use rand_chacha::ChaCha8Rng;
use rand_core::SeedableRng;
use rand_distr::{Distribution, Gamma, Normal};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{OnlineMoments, StatsError};

/// Version of the keyed random stream used by [`ScalarPosterior::try_draw`].
///
/// Changing the seed derivation, random generator, or distribution implementation is a replay
/// compatibility break and requires a new version.
pub const THOMPSON_SAMPLER_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum BayesError {
    #[error("Bayesian parameter must be finite")]
    NonFiniteParameter,
    #[error("Bayesian scale, shape, precision, and rate parameters must be positive")]
    InvalidParameterRange,
    #[error("Bayesian observation must be finite")]
    NonFiniteObservation,
    #[error("Poisson exposure must be finite and non-negative")]
    InvalidExposure,
    #[error("positive event count requires positive exposure")]
    EventsWithoutExposure,
    #[error("Bayesian observation count overflowed u64")]
    CountOverflow,
    #[error("Bayesian arithmetic produced a non-finite state")]
    NumericalOverflow,
    #[error("Bayesian snapshot contains invalid state")]
    InvalidSnapshot,
    #[error("sampler version {found} is not supported; expected {expected}")]
    UnsupportedSamplerVersion { found: u32, expected: u32 },
    #[error("posterior distribution could not be constructed")]
    InvalidPosterior,
}

fn checked_positive(value: f64) -> Result<f64, BayesError> {
    if !value.is_finite() {
        return Err(BayesError::NonFiniteParameter);
    }
    if value <= 0.0 {
        return Err(BayesError::InvalidParameterRange);
    }
    Ok(value)
}

/// Conjugate prior and update rule for a Poisson event rate.
///
/// `beta` is a rate parameter, not a scale parameter. After observing `count` events over
/// `exposure`, the posterior is `Gamma(alpha + count, beta + exposure)`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GammaPoisson {
    alpha_prior: f64,
    beta_prior: f64,
}

impl GammaPoisson {
    pub fn try_new(alpha_prior: f64, beta_prior: f64) -> Result<Self, BayesError> {
        checked_positive(alpha_prior)?;
        checked_positive(beta_prior)?;
        Ok(Self {
            alpha_prior,
            beta_prior,
        })
    }

    pub const fn alpha_prior(&self) -> f64 {
        self.alpha_prior
    }

    pub const fn beta_prior(&self) -> f64 {
        self.beta_prior
    }

    pub const fn init(&self) -> GammaPoissonState {
        GammaPoissonState::new()
    }

    pub fn try_posterior(
        &self,
        state: &GammaPoissonState,
    ) -> Result<GammaPoissonPosterior, BayesError> {
        state.validate()?;
        let alpha = self.alpha_prior + state.event_count as f64;
        let beta = self.beta_prior + state.exposure;
        if !alpha.is_finite() || !beta.is_finite() {
            return Err(BayesError::NumericalOverflow);
        }
        Ok(GammaPoissonPosterior { alpha, beta })
    }
}

/// Complete mergeable checkpoint state for a Gamma-Poisson rate model.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct GammaPoissonState {
    event_count: u64,
    exposure: f64,
}

impl GammaPoissonState {
    pub const fn new() -> Self {
        Self {
            event_count: 0,
            exposure: 0.0,
        }
    }

    pub const fn event_count(&self) -> u64 {
        self.event_count
    }

    pub const fn exposure(&self) -> f64 {
        self.exposure
    }

    pub fn validate(&self) -> Result<(), BayesError> {
        if !self.exposure.is_finite() || self.exposure < 0.0 {
            return Err(BayesError::InvalidSnapshot);
        }
        if self.event_count > 0 && self.exposure == 0.0 {
            return Err(BayesError::InvalidSnapshot);
        }
        Ok(())
    }

    /// Add an aggregated count and exposure atomically.
    pub fn try_observe(&mut self, count: u64, exposure: f64) -> Result<(), BayesError> {
        if !exposure.is_finite() || exposure < 0.0 {
            return Err(BayesError::InvalidExposure);
        }
        if count > 0 && exposure == 0.0 {
            return Err(BayesError::EventsWithoutExposure);
        }
        let next_count = self
            .event_count
            .checked_add(count)
            .ok_or(BayesError::CountOverflow)?;
        let next_exposure = self.exposure + exposure;
        if !next_exposure.is_finite() {
            return Err(BayesError::NumericalOverflow);
        }
        self.event_count = next_count;
        self.exposure = next_exposure;
        Ok(())
    }

    pub fn try_merge(&mut self, other: &Self) -> Result<(), BayesError> {
        self.validate()?;
        other.validate()?;
        self.try_observe(other.event_count, other.exposure)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GammaPoissonPosterior {
    alpha: f64,
    beta: f64,
}

impl GammaPoissonPosterior {
    pub const fn alpha(&self) -> f64 {
        self.alpha
    }

    pub const fn beta(&self) -> f64 {
        self.beta
    }

    pub fn mean_rate(&self) -> f64 {
        self.alpha / self.beta
    }

    pub fn variance_rate(&self) -> f64 {
        self.alpha / (self.beta * self.beta)
    }
}

/// Normal-Inverse-Gamma prior and update rule for an unknown mean and variance.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct NormalInverseGamma {
    mu_prior: f64,
    lambda_prior: f64,
    alpha_prior: f64,
    beta_prior: f64,
}

impl NormalInverseGamma {
    pub fn try_new(
        mu_prior: f64,
        lambda_prior: f64,
        alpha_prior: f64,
        beta_prior: f64,
    ) -> Result<Self, BayesError> {
        if !mu_prior.is_finite() {
            return Err(BayesError::NonFiniteParameter);
        }
        checked_positive(lambda_prior)?;
        checked_positive(alpha_prior)?;
        checked_positive(beta_prior)?;
        Ok(Self {
            mu_prior,
            lambda_prior,
            alpha_prior,
            beta_prior,
        })
    }

    pub const fn mu_prior(&self) -> f64 {
        self.mu_prior
    }

    pub const fn lambda_prior(&self) -> f64 {
        self.lambda_prior
    }

    pub const fn alpha_prior(&self) -> f64 {
        self.alpha_prior
    }

    pub const fn beta_prior(&self) -> f64 {
        self.beta_prior
    }

    pub const fn init(&self) -> NormalInverseGammaState {
        NormalInverseGammaState::new()
    }

    pub fn try_posterior(
        &self,
        state: &NormalInverseGammaState,
    ) -> Result<NormalInverseGammaPosterior, BayesError> {
        state.validate()?;
        let n = state.moments.count() as f64;
        let mean = state.moments.mean().unwrap_or(0.0);
        let lambda = self.lambda_prior + n;
        let mu = (self.lambda_prior * self.mu_prior + n * mean) / lambda;
        let alpha = self.alpha_prior + n * 0.5;
        let delta = mean - self.mu_prior;
        let beta = self.beta_prior
            + 0.5 * state.moments.sum_of_squared_deviations()
            + self.lambda_prior * n * delta * delta / (2.0 * lambda);
        if !mu.is_finite()
            || !lambda.is_finite()
            || !alpha.is_finite()
            || !beta.is_finite()
            || lambda <= 0.0
            || alpha <= 0.0
            || beta <= 0.0
        {
            return Err(BayesError::NumericalOverflow);
        }
        Ok(NormalInverseGammaPosterior {
            mu,
            lambda,
            alpha,
            beta,
        })
    }
}

/// Mergeable sufficient statistics for a Normal-Inverse-Gamma model.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct NormalInverseGammaState {
    moments: OnlineMoments,
}

impl NormalInverseGammaState {
    pub const fn new() -> Self {
        Self {
            moments: OnlineMoments::new(),
        }
    }

    pub const fn count(&self) -> u64 {
        self.moments.count()
    }

    pub const fn moments(&self) -> &OnlineMoments {
        &self.moments
    }

    pub fn validate(&self) -> Result<(), BayesError> {
        self.moments
            .validate()
            .map_err(|_| BayesError::InvalidSnapshot)
    }

    pub fn try_observe(&mut self, value: f64) -> Result<(), BayesError> {
        self.moments.try_push(value).map_err(map_stats_error)
    }

    pub fn try_merge(&mut self, other: &Self) -> Result<(), BayesError> {
        self.validate()?;
        other.validate()?;
        self.moments
            .try_merge(&other.moments)
            .map_err(map_stats_error)
    }
}

fn map_stats_error(error: StatsError) -> BayesError {
    match error {
        StatsError::NonFiniteInput => BayesError::NonFiniteObservation,
        StatsError::CountOverflow => BayesError::CountOverflow,
        StatsError::InvalidSnapshot => BayesError::InvalidSnapshot,
        StatsError::NumericalOverflow | StatsError::NumericalInstability => {
            BayesError::NumericalOverflow
        }
        StatsError::EmptyRemoval => BayesError::InvalidSnapshot,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct NormalInverseGammaPosterior {
    mu: f64,
    lambda: f64,
    alpha: f64,
    beta: f64,
}

impl NormalInverseGammaPosterior {
    pub const fn mu(&self) -> f64 {
        self.mu
    }

    pub const fn lambda(&self) -> f64 {
        self.lambda
    }

    pub const fn alpha(&self) -> f64 {
        self.alpha
    }

    pub const fn beta(&self) -> f64 {
        self.beta
    }

    pub const fn expected_mean(&self) -> f64 {
        self.mu
    }

    pub fn expected_variance(&self) -> Option<f64> {
        (self.alpha > 1.0).then(|| self.beta / (self.alpha - 1.0))
    }
}

/// Complete identity of one replayable posterior draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ThompsonKey {
    pub strategy_key: u64,
    pub decision_id: u64,
    pub arm_id: u64,
    pub sampler_version: u32,
}

impl ThompsonKey {
    pub const fn new(strategy_key: u64, decision_id: u64, arm_id: u64) -> Self {
        Self {
            strategy_key,
            decision_id,
            arm_id,
            sampler_version: THOMPSON_SAMPLER_VERSION,
        }
    }

    fn try_rng(self) -> Result<ChaCha8Rng, BayesError> {
        if self.sampler_version != THOMPSON_SAMPLER_VERSION {
            return Err(BayesError::UnsupportedSamplerVersion {
                found: self.sampler_version,
                expected: THOMPSON_SAMPLER_VERSION,
            });
        }
        let mut seed = splitmix64(self.strategy_key);
        seed = splitmix64(seed ^ self.decision_id);
        seed = splitmix64(seed ^ self.arm_id);
        seed = splitmix64(seed ^ u64::from(self.sampler_version));
        Ok(ChaCha8Rng::seed_from_u64(seed))
    }
}

const fn splitmix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

/// A posterior that can generate one deterministic scalar utility draw.
pub trait ScalarPosterior {
    fn try_draw(&self, key: ThompsonKey) -> Result<f64, BayesError>;
}

impl<T> ScalarPosterior for &T
where
    T: ScalarPosterior + ?Sized,
{
    fn try_draw(&self, key: ThompsonKey) -> Result<f64, BayesError> {
        (*self).try_draw(key)
    }
}

impl ScalarPosterior for GammaPoissonPosterior {
    fn try_draw(&self, key: ThompsonKey) -> Result<f64, BayesError> {
        checked_positive(self.alpha).map_err(|_| BayesError::InvalidPosterior)?;
        checked_positive(self.beta).map_err(|_| BayesError::InvalidPosterior)?;
        let distribution =
            Gamma::new(self.alpha, 1.0 / self.beta).map_err(|_| BayesError::InvalidPosterior)?;
        let draw = distribution.sample(&mut key.try_rng()?);
        if draw.is_finite() {
            Ok(draw)
        } else {
            Err(BayesError::NumericalOverflow)
        }
    }
}

impl ScalarPosterior for NormalInverseGammaPosterior {
    fn try_draw(&self, key: ThompsonKey) -> Result<f64, BayesError> {
        checked_positive(self.lambda).map_err(|_| BayesError::InvalidPosterior)?;
        checked_positive(self.alpha).map_err(|_| BayesError::InvalidPosterior)?;
        checked_positive(self.beta).map_err(|_| BayesError::InvalidPosterior)?;
        if !self.mu.is_finite() {
            return Err(BayesError::InvalidPosterior);
        }

        let mut rng = key.try_rng()?;
        let precision_distribution =
            Gamma::new(self.alpha, 1.0 / self.beta).map_err(|_| BayesError::InvalidPosterior)?;
        let precision = precision_distribution.sample(&mut rng);
        let standard_deviation = (1.0 / (self.lambda * precision)).sqrt();
        let mean_distribution =
            Normal::new(self.mu, standard_deviation).map_err(|_| BayesError::InvalidPosterior)?;
        let draw = mean_distribution.sample(&mut rng);
        if draw.is_finite() {
            Ok(draw)
        } else {
            Err(BayesError::NumericalOverflow)
        }
    }
}

/// One arm presented to the constrained Thompson selector.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThompsonCandidate<Id, Posterior> {
    pub id: Id,
    pub arm_id: u64,
    pub posterior: Posterior,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Eligibility {
    Eligible,
    Rejected { constraint: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ThompsonTrace<Id> {
    Rejected { id: Id, constraint: u32 },
    Sampled { id: Id, utility: f64 },
    Selected { id: Id, utility: f64 },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThompsonSelection<Id> {
    pub id: Id,
    pub utility: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThompsonDecision<Id> {
    pub selection: Option<ThompsonSelection<Id>>,
    pub examined: u64,
    pub eligible: u64,
}

/// Select the largest posterior utility draw among eligible candidates.
///
/// Eligibility is evaluated before sampling. The function allocates no collection and keeps the
/// first candidate on exact ties, so candidate order is part of the replay contract.
pub fn try_select_thompson<I, Id, Posterior, Gate, Sink>(
    strategy_key: u64,
    decision_id: u64,
    candidates: I,
    mut gate: Gate,
    emit: &mut Sink,
) -> Result<ThompsonDecision<Id>, BayesError>
where
    I: IntoIterator<Item = ThompsonCandidate<Id, Posterior>>,
    Id: Copy,
    Posterior: ScalarPosterior,
    Gate: FnMut(&ThompsonCandidate<Id, Posterior>) -> Eligibility,
    Sink: Emit<ThompsonTrace<Id>>,
{
    let mut examined = 0_u64;
    let mut eligible = 0_u64;
    let mut selection: Option<ThompsonSelection<Id>> = None;

    for candidate in candidates {
        examined = examined.checked_add(1).ok_or(BayesError::CountOverflow)?;
        match gate(&candidate) {
            Eligibility::Rejected { constraint } => {
                emit.emit(ThompsonTrace::Rejected {
                    id: candidate.id,
                    constraint,
                });
            }
            Eligibility::Eligible => {
                eligible = eligible.checked_add(1).ok_or(BayesError::CountOverflow)?;
                let utility = candidate.posterior.try_draw(ThompsonKey::new(
                    strategy_key,
                    decision_id,
                    candidate.arm_id,
                ))?;
                emit.emit(ThompsonTrace::Sampled {
                    id: candidate.id,
                    utility,
                });
                if selection
                    .as_ref()
                    .is_none_or(|current| utility > current.utility)
                {
                    selection = Some(ThompsonSelection {
                        id: candidate.id,
                        utility,
                    });
                }
            }
        }
    }

    if let Some(selected) = selection {
        emit.emit(ThompsonTrace::Selected {
            id: selected.id,
            utility: selected.utility,
        });
    }

    Ok(ThompsonDecision {
        selection,
        examined,
        eligible,
    })
}

#[cfg(test)]
mod tests {
    use helio_scan::VecEmitter;

    use super::*;

    fn close(left: f64, right: f64) {
        assert!((left - right).abs() <= 1e-12, "{left} != {right}");
    }

    #[derive(Debug, Clone, Copy)]
    struct ConstantPosterior(f64);

    impl ScalarPosterior for ConstantPosterior {
        fn try_draw(&self, _key: ThompsonKey) -> Result<f64, BayesError> {
            Ok(self.0)
        }
    }

    #[test]
    fn gamma_poisson_updates_and_merges_sufficient_statistics() {
        let model = GammaPoisson::try_new(2.0, 4.0).unwrap();
        let mut continuous = model.init();
        continuous.try_observe(3, 5.0).unwrap();
        continuous.try_observe(2, 7.0).unwrap();

        let mut left = model.init();
        left.try_observe(3, 5.0).unwrap();
        let mut right = model.init();
        right.try_observe(2, 7.0).unwrap();
        left.try_merge(&right).unwrap();

        assert_eq!(left, continuous);
        let posterior = model.try_posterior(&left).unwrap();
        close(posterior.alpha(), 7.0);
        close(posterior.beta(), 16.0);
        close(posterior.mean_rate(), 7.0 / 16.0);
    }

    #[test]
    fn gamma_poisson_rejection_is_atomic() {
        let mut state = GammaPoissonState::new();
        state.try_observe(1, 2.0).unwrap();
        let before = state;
        assert_eq!(
            state.try_observe(1, 0.0),
            Err(BayesError::EventsWithoutExposure)
        );
        assert_eq!(state, before);
    }

    #[test]
    fn normal_inverse_gamma_matches_closed_form_posterior() {
        let model = NormalInverseGamma::try_new(0.0, 1.0, 2.0, 3.0).unwrap();
        let mut state = model.init();
        for value in [1.0, 2.0, 3.0] {
            state.try_observe(value).unwrap();
        }
        let posterior = model.try_posterior(&state).unwrap();
        close(posterior.lambda(), 4.0);
        close(posterior.mu(), 1.5);
        close(posterior.alpha(), 3.5);
        close(posterior.beta(), 5.5);
    }

    #[test]
    fn normal_inverse_gamma_merge_matches_continuous_update() {
        let model = NormalInverseGamma::try_new(0.0, 1.0, 2.0, 3.0).unwrap();
        let mut continuous = model.init();
        for value in [1.0, 2.0, 3.0, 7.0] {
            continuous.try_observe(value).unwrap();
        }
        let mut left = model.init();
        let mut right = model.init();
        for value in [1.0, 2.0] {
            left.try_observe(value).unwrap();
        }
        for value in [3.0, 7.0] {
            right.try_observe(value).unwrap();
        }
        left.try_merge(&right).unwrap();
        assert_eq!(left, continuous);
        assert_eq!(
            model.try_posterior(&left).unwrap(),
            model.try_posterior(&continuous).unwrap()
        );
    }

    #[test]
    fn posterior_draw_is_keyed_and_replayable() {
        let model = NormalInverseGamma::try_new(0.0, 1.0, 2.0, 3.0).unwrap();
        let mut state = model.init();
        for value in [0.2, -0.1, 0.4, 0.3] {
            state.try_observe(value).unwrap();
        }
        let posterior = model.try_posterior(&state).unwrap();
        let key = ThompsonKey::new(91, 7, 3);
        assert_eq!(
            posterior.try_draw(key).unwrap(),
            posterior.try_draw(key).unwrap()
        );
        assert_ne!(
            posterior.try_draw(key).unwrap(),
            posterior.try_draw(ThompsonKey::new(91, 8, 3)).unwrap()
        );
    }

    #[test]
    fn unsupported_sampler_version_fails_closed() {
        let posterior = GammaPoissonPosterior {
            alpha: 2.0,
            beta: 3.0,
        };
        let mut key = ThompsonKey::new(1, 2, 3);
        key.sampler_version = THOMPSON_SAMPLER_VERSION + 1;
        assert_eq!(
            posterior.try_draw(key),
            Err(BayesError::UnsupportedSamplerVersion {
                found: 2,
                expected: 1,
            })
        );
    }

    #[test]
    fn constraints_run_before_sampling_and_replay_exactly() {
        let posteriors = [
            GammaPoissonPosterior {
                alpha: 3.0,
                beta: 5.0,
            },
            GammaPoissonPosterior {
                alpha: 4.0,
                beta: 5.0,
            },
            GammaPoissonPosterior {
                alpha: 5.0,
                beta: 5.0,
            },
        ];
        let candidates = || {
            posteriors
                .iter()
                .enumerate()
                .map(|(index, posterior)| ThompsonCandidate {
                    id: index,
                    arm_id: index as u64,
                    posterior,
                })
        };
        let run = || {
            let mut trace = VecEmitter::new();
            let decision = try_select_thompson(
                44,
                19,
                candidates(),
                |candidate| {
                    if candidate.id == 1 {
                        Eligibility::Rejected { constraint: 9 }
                    } else {
                        Eligibility::Eligible
                    }
                },
                &mut trace,
            )
            .unwrap();
            (decision, trace.0)
        };

        let first = run();
        let second = run();
        assert_eq!(first, second);
        assert_eq!(first.0.examined, 3);
        assert_eq!(first.0.eligible, 2);
        assert!(first.1.contains(&ThompsonTrace::Rejected {
            id: 1,
            constraint: 9,
        }));
        assert!(!first
            .1
            .iter()
            .any(|entry| matches!(entry, ThompsonTrace::Sampled { id: 1, .. })));
    }

    #[test]
    fn no_eligible_arm_is_an_explicit_empty_decision() {
        let mut trace = VecEmitter::new();
        let decision = try_select_thompson(
            1,
            2,
            [ThompsonCandidate {
                id: "blocked",
                arm_id: 3,
                posterior: ConstantPosterior(99.0),
            }],
            |_| Eligibility::Rejected { constraint: 7 },
            &mut trace,
        )
        .unwrap();
        assert_eq!(decision.selection, None);
        assert_eq!(decision.examined, 1);
        assert_eq!(decision.eligible, 0);
        assert_eq!(
            trace.0,
            [ThompsonTrace::Rejected {
                id: "blocked",
                constraint: 7,
            }]
        );
    }

    #[test]
    fn exact_draw_ties_keep_candidate_order() {
        let mut trace = VecEmitter::new();
        let decision = try_select_thompson(
            1,
            2,
            [
                ThompsonCandidate {
                    id: "first",
                    arm_id: 10,
                    posterior: ConstantPosterior(0.5),
                },
                ThompsonCandidate {
                    id: "second",
                    arm_id: 11,
                    posterior: ConstantPosterior(0.5),
                },
            ],
            |_| Eligibility::Eligible,
            &mut trace,
        )
        .unwrap();
        assert_eq!(decision.selection.unwrap().id, "first");
    }

    #[test]
    fn checkpoint_round_trip_preserves_bayesian_state() {
        let model = NormalInverseGamma::try_new(0.0, 1.0, 2.0, 3.0).unwrap();
        let mut state = model.init();
        state.try_observe(1.25).unwrap();
        let encoded = serde_json::to_vec(&state).unwrap();
        let restored: NormalInverseGammaState = serde_json::from_slice(&encoded).unwrap();
        restored.validate().unwrap();
        assert_eq!(state, restored);
        assert_eq!(
            model.try_posterior(&state).unwrap(),
            model.try_posterior(&restored).unwrap()
        );
    }
}
