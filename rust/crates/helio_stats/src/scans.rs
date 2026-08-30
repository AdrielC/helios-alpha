//! `helio_scan` adapters for online statistical states.

use helio_scan::{
    Emit, FallibleRestoreScan, FlushReason, FlushableScan, Scan, SnapshottingScan,
    VersionedSnapshot,
};
use serde::{Deserialize, Serialize};

#[cfg(feature = "bayes")]
use crate::{
    BayesError, GammaPoisson, GammaPoissonPosterior, GammaPoissonState, NormalInverseGamma,
    NormalInverseGammaPosterior, NormalInverseGammaState,
};
use crate::{
    CompensatedSum, ExponentialHawkes, HawkesError, HawkesState, HawkesUpdate, LogProbability,
    NumericalError, OnlineCovariance, OnlineMoments, ScaledSumSquares, StatsError,
};

/// Restartable Neumaier-compensated sum.
#[derive(Debug, Clone, Copy, Default)]
pub struct CompensatedSumScan;

impl Scan for CompensatedSumScan {
    type In = f64;
    type Out = Result<f64, NumericalError>;
    type State = CompensatedSum;

    fn init(&self) -> Self::State {
        CompensatedSum::new()
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        let result = state.try_push(input).and_then(|()| state.try_total());
        emit.emit(result);
    }
}

impl FlushableScan for CompensatedSumScan {
    type Offset = u64;

    fn flush<E>(&self, _state: &mut Self::State, _signal: FlushReason<Self::Offset>, _emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
    }
}

impl SnapshottingScan for CompensatedSumScan {
    type Snapshot = CompensatedSum;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        *state
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        snapshot
    }
}

impl FallibleRestoreScan for CompensatedSumScan {
    type RestoreError = NumericalError;

    fn try_restore(&self, snapshot: Self::Snapshot) -> Result<Self::State, Self::RestoreError> {
        snapshot.validate()?;
        Ok(snapshot)
    }
}

impl VersionedSnapshot for CompensatedSum {
    const VERSION: u32 = 1;
}

/// Restartable scaled sum of squares. Emits the stable Euclidean norm.
#[derive(Debug, Clone, Copy, Default)]
pub struct ScaledSumSquaresScan;

impl Scan for ScaledSumSquaresScan {
    type In = f64;
    type Out = Result<f64, NumericalError>;
    type State = ScaledSumSquares;

    fn init(&self) -> Self::State {
        ScaledSumSquares::new()
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        let result = state.try_push(input).and_then(|()| state.try_norm());
        emit.emit(result);
    }
}

impl FlushableScan for ScaledSumSquaresScan {
    type Offset = u64;

    fn flush<E>(&self, _state: &mut Self::State, _signal: FlushReason<Self::Offset>, _emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
    }
}

impl SnapshottingScan for ScaledSumSquaresScan {
    type Snapshot = ScaledSumSquares;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        *state
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        snapshot
    }
}

impl FallibleRestoreScan for ScaledSumSquaresScan {
    type RestoreError = NumericalError;

    fn try_restore(&self, snapshot: Self::Snapshot) -> Result<Self::State, Self::RestoreError> {
        snapshot.validate()?;
        Ok(snapshot)
    }
}

impl VersionedSnapshot for ScaledSumSquares {
    const VERSION: u32 = 1;
}

/// Restartable product of conditional probabilities in log space.
#[derive(Debug, Clone, Copy, Default)]
pub struct LogProbabilityProductScan;

impl Scan for LogProbabilityProductScan {
    type In = f64;
    type Out = Result<LogProbability, NumericalError>;
    type State = LogProbability;

    fn init(&self) -> Self::State {
        LogProbability::one()
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        let result = state.try_multiply_probability(input);
        if let Ok(next) = result {
            *state = next;
        }
        emit.emit(result);
    }
}

impl FlushableScan for LogProbabilityProductScan {
    type Offset = u64;

    fn flush<E>(&self, _state: &mut Self::State, _signal: FlushReason<Self::Offset>, _emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
    }
}

impl SnapshottingScan for LogProbabilityProductScan {
    type Snapshot = LogProbability;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        *state
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        snapshot
    }
}

impl FallibleRestoreScan for LogProbabilityProductScan {
    type RestoreError = NumericalError;

    fn try_restore(&self, snapshot: Self::Snapshot) -> Result<Self::State, Self::RestoreError> {
        snapshot.validate()?;
        Ok(snapshot)
    }
}

impl VersionedSnapshot for LogProbability {
    const VERSION: u32 = 1;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct OnlineMomentsScan;

impl Scan for OnlineMomentsScan {
    type In = f64;
    type Out = Result<OnlineMoments, StatsError>;
    type State = OnlineMoments;

    fn init(&self) -> Self::State {
        OnlineMoments::new()
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        match state.try_push(input) {
            Ok(()) => emit.emit(Ok(*state)),
            Err(error) => emit.emit(Err(error)),
        }
    }
}

impl FlushableScan for OnlineMomentsScan {
    type Offset = u64;

    fn flush<E>(&self, _state: &mut Self::State, _signal: FlushReason<Self::Offset>, _emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
    }
}

impl SnapshottingScan for OnlineMomentsScan {
    type Snapshot = OnlineMoments;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        *state
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        snapshot
    }
}

impl FallibleRestoreScan for OnlineMomentsScan {
    type RestoreError = StatsError;

    fn try_restore(&self, snapshot: Self::Snapshot) -> Result<Self::State, Self::RestoreError> {
        snapshot.validate()?;
        Ok(snapshot)
    }
}

impl VersionedSnapshot for OnlineMoments {
    const VERSION: u32 = 1;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct OnlineCovarianceScan;

impl Scan for OnlineCovarianceScan {
    type In = (f64, f64);
    type Out = Result<OnlineCovariance, StatsError>;
    type State = OnlineCovariance;

    fn init(&self) -> Self::State {
        OnlineCovariance::new()
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        match state.try_push(input.0, input.1) {
            Ok(()) => emit.emit(Ok(*state)),
            Err(error) => emit.emit(Err(error)),
        }
    }
}

impl FlushableScan for OnlineCovarianceScan {
    type Offset = u64;

    fn flush<E>(&self, _state: &mut Self::State, _signal: FlushReason<Self::Offset>, _emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
    }
}

impl SnapshottingScan for OnlineCovarianceScan {
    type Snapshot = OnlineCovariance;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        *state
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        snapshot
    }
}

impl FallibleRestoreScan for OnlineCovarianceScan {
    type RestoreError = StatsError;

    fn try_restore(&self, snapshot: Self::Snapshot) -> Result<Self::State, Self::RestoreError> {
        snapshot.validate()?;
        Ok(snapshot)
    }
}

impl VersionedSnapshot for OnlineCovariance {
    const VERSION: u32 = 1;
}

#[cfg(feature = "bayes")]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CountExposure {
    pub count: u64,
    pub exposure: f64,
}

#[cfg(feature = "bayes")]
#[derive(Debug, Clone, Copy)]
pub struct GammaPoissonScan {
    pub model: GammaPoisson,
}

#[cfg(feature = "bayes")]
impl GammaPoissonScan {
    pub const fn new(model: GammaPoisson) -> Self {
        Self { model }
    }
}

#[cfg(feature = "bayes")]
impl Scan for GammaPoissonScan {
    type In = CountExposure;
    type Out = Result<GammaPoissonPosterior, BayesError>;
    type State = GammaPoissonState;

    fn init(&self) -> Self::State {
        self.model.init()
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        let mut next = *state;
        let result = next
            .try_observe(input.count, input.exposure)
            .and_then(|()| self.model.try_posterior(&next));
        if result.is_ok() {
            *state = next;
        }
        emit.emit(result);
    }
}

#[cfg(feature = "bayes")]
impl FlushableScan for GammaPoissonScan {
    type Offset = u64;

    fn flush<E>(&self, _state: &mut Self::State, _signal: FlushReason<Self::Offset>, _emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
    }
}

#[cfg(feature = "bayes")]
impl SnapshottingScan for GammaPoissonScan {
    type Snapshot = GammaPoissonState;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        *state
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        snapshot
    }
}

#[cfg(feature = "bayes")]
impl FallibleRestoreScan for GammaPoissonScan {
    type RestoreError = BayesError;

    fn try_restore(&self, snapshot: Self::Snapshot) -> Result<Self::State, Self::RestoreError> {
        snapshot.validate()?;
        self.model.try_posterior(&snapshot)?;
        Ok(snapshot)
    }
}

#[cfg(feature = "bayes")]
impl VersionedSnapshot for GammaPoissonState {
    const VERSION: u32 = 1;
}

#[cfg(feature = "bayes")]
#[derive(Debug, Clone, Copy)]
pub struct NormalInverseGammaScan {
    pub model: NormalInverseGamma,
}

#[cfg(feature = "bayes")]
impl NormalInverseGammaScan {
    pub const fn new(model: NormalInverseGamma) -> Self {
        Self { model }
    }
}

#[cfg(feature = "bayes")]
impl Scan for NormalInverseGammaScan {
    type In = f64;
    type Out = Result<NormalInverseGammaPosterior, BayesError>;
    type State = NormalInverseGammaState;

    fn init(&self) -> Self::State {
        self.model.init()
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        let mut next = *state;
        let result = next
            .try_observe(input)
            .and_then(|()| self.model.try_posterior(&next));
        if result.is_ok() {
            *state = next;
        }
        emit.emit(result);
    }
}

#[cfg(feature = "bayes")]
impl FlushableScan for NormalInverseGammaScan {
    type Offset = u64;

    fn flush<E>(&self, _state: &mut Self::State, _signal: FlushReason<Self::Offset>, _emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
    }
}

#[cfg(feature = "bayes")]
impl SnapshottingScan for NormalInverseGammaScan {
    type Snapshot = NormalInverseGammaState;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        *state
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        snapshot
    }
}

#[cfg(feature = "bayes")]
impl FallibleRestoreScan for NormalInverseGammaScan {
    type RestoreError = BayesError;

    fn try_restore(&self, snapshot: Self::Snapshot) -> Result<Self::State, Self::RestoreError> {
        snapshot.validate()?;
        self.model.try_posterior(&snapshot)?;
        Ok(snapshot)
    }
}

#[cfg(feature = "bayes")]
impl VersionedSnapshot for NormalInverseGammaState {
    const VERSION: u32 = 1;
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct HawkesEvent {
    pub timestamp: i64,
    pub mark: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct HawkesScan {
    pub model: ExponentialHawkes,
}

impl HawkesScan {
    pub const fn new(model: ExponentialHawkes) -> Self {
        Self { model }
    }
}

impl Scan for HawkesScan {
    type In = HawkesEvent;
    type Out = Result<HawkesUpdate, HawkesError>;
    type State = HawkesState;

    fn init(&self) -> Self::State {
        self.model.init()
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        emit.emit(self.model.try_observe(state, input.timestamp, input.mark));
    }
}

impl FlushableScan for HawkesScan {
    type Offset = u64;

    fn flush<E>(&self, _state: &mut Self::State, _signal: FlushReason<Self::Offset>, _emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
    }
}

impl SnapshottingScan for HawkesScan {
    type Snapshot = HawkesState;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        *state
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        snapshot
    }
}

impl FallibleRestoreScan for HawkesScan {
    type RestoreError = HawkesError;

    fn try_restore(&self, snapshot: Self::Snapshot) -> Result<Self::State, Self::RestoreError> {
        snapshot.validate()?;
        Ok(snapshot)
    }
}

impl VersionedSnapshot for HawkesState {
    const VERSION: u32 = 1;
}

#[cfg(test)]
mod tests {
    use helio_scan::{Scan, SnapshottingScan, VecEmitter};

    use super::*;

    #[test]
    fn moments_scan_rejects_nan_without_state_mutation() {
        let scan = OnlineMomentsScan;
        let mut state = scan.init();
        let mut emit = VecEmitter::new();
        scan.step(&mut state, 1.0, &mut emit);
        let before = state;
        scan.step(&mut state, f64::NAN, &mut emit);
        assert_eq!(state, before);
        assert_eq!(emit.0[1], Err(StatsError::NonFiniteInput));
    }

    #[test]
    fn guarded_numeric_scans_resume_without_losing_hidden_state() {
        let sum_scan = CompensatedSumScan;
        let mut sum = sum_scan.init();
        let mut emitted = VecEmitter::new();
        sum_scan.step(&mut sum, 1e16, &mut emitted);
        sum_scan.step(&mut sum, 1.0, &mut emitted);
        sum = sum_scan.try_restore(sum_scan.snapshot(&sum)).unwrap();
        sum_scan.step(&mut sum, -1e16, &mut emitted);
        assert_eq!(emitted.0.last(), Some(&Ok(1.0)));

        let probability_scan = LogProbabilityProductScan;
        let mut probability = probability_scan.init();
        let mut products = VecEmitter::new();
        probability_scan.step(&mut probability, 1e-200, &mut products);
        probability = probability_scan
            .try_restore(probability_scan.snapshot(&probability))
            .unwrap();
        probability_scan.step(&mut probability, 1e-200, &mut products);
        let product = products.0.last().copied().unwrap().unwrap();
        assert_eq!(product.probability(), 0.0);
        assert!(product.ln_probability().is_finite());
    }

    #[test]
    fn hawkes_scan_snapshot_resume_matches_continuous() {
        let model = ExponentialHawkes::try_new(0.1, 0.2, 1.0).unwrap();
        let scan = HawkesScan::new(model);
        let mut continuous = scan.init();
        let mut resumed = scan.init();
        let mut left = VecEmitter::new();
        let mut right = VecEmitter::new();
        let first = HawkesEvent {
            timestamp: 10,
            mark: 1.0,
        };
        scan.step(&mut continuous, first, &mut left);
        scan.step(&mut resumed, first, &mut right);
        resumed = scan.restore(scan.snapshot(&resumed));
        let second = HawkesEvent {
            timestamp: 20,
            mark: 2.0,
        };
        scan.step(&mut continuous, second, &mut left);
        scan.step(&mut resumed, second, &mut right);
        assert_eq!(continuous, resumed);
        assert_eq!(left.0, right.0);
    }

    #[test]
    #[cfg(feature = "bayes")]
    fn bayesian_scans_resume_with_identical_posteriors() {
        let model = NormalInverseGamma::try_new(0.0, 1.0, 2.0, 3.0).unwrap();
        let scan = NormalInverseGammaScan::new(model);
        let mut continuous = scan.init();
        let mut resumed = scan.init();
        let mut left = VecEmitter::new();
        let mut right = VecEmitter::new();
        scan.step(&mut continuous, 0.5, &mut left);
        scan.step(&mut resumed, 0.5, &mut right);
        resumed = scan.try_restore(scan.snapshot(&resumed)).unwrap();
        scan.step(&mut continuous, 1.25, &mut left);
        scan.step(&mut resumed, 1.25, &mut right);
        assert_eq!(continuous, resumed);
        assert_eq!(left.0, right.0);

        let rate = GammaPoisson::try_new(1.0, 1.0).unwrap();
        let scan = GammaPoissonScan::new(rate);
        let mut state = scan.init();
        let mut emit = VecEmitter::new();
        scan.step(
            &mut state,
            CountExposure {
                count: 2,
                exposure: 4.0,
            },
            &mut emit,
        );
        assert_eq!(emit.0[0].unwrap().mean_rate(), 3.0 / 5.0);
    }

    #[test]
    fn fallible_restore_rejects_corrupt_statistical_state() {
        let invalid: OnlineMoments =
            serde_json::from_str(r#"{"count":1,"mean":0.0,"m2":-1.0}"#).unwrap();
        assert_eq!(
            OnlineMomentsScan.try_restore(invalid),
            Err(StatsError::InvalidSnapshot)
        );

        let model = ExponentialHawkes::try_new(0.1, 0.2, 1.0).unwrap();
        let invalid = HawkesState {
            last_time: None,
            excitation: 1.0,
            event_count: 0,
        };
        assert_eq!(
            HawkesScan::new(model).try_restore(invalid),
            Err(HawkesError::InvalidSnapshot)
        );

        #[cfg(feature = "bayes")]
        {
            let invalid: GammaPoissonState =
                serde_json::from_str(r#"{"event_count":1,"exposure":0.0}"#).unwrap();
            let model = GammaPoisson::try_new(1.0, 1.0).unwrap();
            assert_eq!(
                GammaPoissonScan::new(model).try_restore(invalid),
                Err(BayesError::InvalidSnapshot)
            );
        }
    }
}
