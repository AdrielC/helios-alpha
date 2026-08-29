//! `helio_scan` adapters for online statistical states.

use helio_scan::{
    Emit, FallibleRestoreScan, FlushReason, FlushableScan, Scan, SnapshottingScan,
    VersionedSnapshot,
};
use serde::{Deserialize, Serialize};

use crate::{
    ExponentialHawkes, HawkesError, HawkesState, HawkesUpdate, OnlineCovariance, OnlineMoments,
    StatsError,
};

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
    }
}
