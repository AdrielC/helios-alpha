use std::marker::PhantomData;

use helio_scan::{
    Emit, FlushReason, FlushableScan, Scan, SessionDate, SnapshottingScan, VersionedSnapshot,
};
use serde::{Deserialize, Serialize};

use crate::{AvailableAt, Timed};

/// Emits `Timed<T>` only when `available_at <= decision_available` (inclusive).
#[derive(Debug, Clone)]
pub struct AvailabilityGateScan {
    /// Treat `None` as no gate (pass-through).
    pub decision_available: Option<AvailableAt>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AvailabilityGateState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AvailabilityGateSnapshot;

impl Scan for AvailabilityGateScan {
    type In = Timed<()>;
    type Out = Timed<()>;
    type State = AvailabilityGateState;

    fn init(&self) -> Self::State {
        AvailabilityGateState
    }

    fn step<E>(&self, _state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        if let Some(cut) = self.decision_available {
            if input.available_at <= cut {
                emit.emit(input);
            }
        } else {
            emit.emit(input);
        }
    }
}

impl FlushableScan for AvailabilityGateScan {
    type Offset = u64;

    fn flush<E>(&self, _state: &mut Self::State, _signal: FlushReason<Self::Offset>, _emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
    }
}

impl SnapshottingScan for AvailabilityGateScan {
    type Snapshot = AvailabilityGateSnapshot;

    fn snapshot(&self, _state: &Self::State) -> Self::Snapshot {
        AvailabilityGateSnapshot
    }

    fn restore(&self, _snapshot: Self::Snapshot) -> Self::State {
        AvailabilityGateState
    }
}

impl VersionedSnapshot for AvailabilityGateSnapshot {
    const VERSION: u32 = 1;
}

/// Attach or replace [`SessionDate`] on the stream (alignment helper).
#[derive(Debug, Clone, Copy)]
pub struct SessionAlignScan<T> {
    pub session: SessionDate,
    _p: PhantomData<T>,
}

impl<T> SessionAlignScan<T> {
    pub fn new(session: SessionDate) -> Self {
        Self {
            session,
            _p: PhantomData,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionAlignState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionAlignSnapshot;

impl<T: Clone> Scan for SessionAlignScan<T> {
    type In = Timed<T>;
    type Out = Timed<T>;
    type State = SessionAlignState;

    fn init(&self) -> Self::State {
        SessionAlignState
    }

    fn step<E>(&self, _state: &mut Self::State, mut input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        input.session_date = Some(self.session);
        emit.emit(input);
    }
}

impl<T: Clone> FlushableScan for SessionAlignScan<T> {
    type Offset = u64;

    fn flush<E>(&self, _state: &mut Self::State, _signal: FlushReason<Self::Offset>, _emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
    }
}

impl<T: Clone> SnapshottingScan for SessionAlignScan<T> {
    type Snapshot = SessionAlignSnapshot;

    fn snapshot(&self, _state: &Self::State) -> Self::Snapshot {
        SessionAlignSnapshot
    }

    fn restore(&self, _snapshot: Self::Snapshot) -> Self::State {
        SessionAlignState
    }
}

impl VersionedSnapshot for SessionAlignSnapshot {
    const VERSION: u32 = 1;
}

#[cfg(test)]
mod tests {
    use super::*;
    use helio_scan::VecEmitter;

    #[test]
    fn gate_blocks_future_available() {
        let s = AvailabilityGateScan {
            decision_available: Some(AvailableAt(5)),
        };
        let mut st = s.init();
        let mut e = VecEmitter::new();
        s.step(&mut st, Timed::new((), AvailableAt(10)), &mut e);
        assert!(e.0.is_empty());
        s.step(&mut st, Timed::new((), AvailableAt(3)), &mut e);
        assert_eq!(e.0.len(), 1);
        assert_eq!(e.0[0].available_at.0, 3);
    }
}
