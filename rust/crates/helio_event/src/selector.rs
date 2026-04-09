use helio_scan::{Emit, FlushReason, FlushableScan, Scan, SnapshottingScan, VersionedSnapshot};
use serde::{Deserialize, Serialize};

use crate::{AvailabilityTagged, TreatmentEvent};

/// Passes through [`TreatmentEvent`] when `available_at` is at or before the decision cut.
#[derive(Debug, Clone, Copy)]
pub struct TreatmentSelectorScan {
    pub decision_available: helio_time::AvailableAt,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreatmentSelectorState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreatmentSelectorSnapshot;

impl Scan for TreatmentSelectorScan {
    type In = AvailabilityTagged<TreatmentEvent>;
    type Out = TreatmentEvent;
    type State = TreatmentSelectorState;

    fn init(&self) -> Self::State {
        TreatmentSelectorState
    }

    fn step<E>(&self, _state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        if input.available_at <= self.decision_available {
            emit.emit(input.value);
        }
    }
}

impl FlushableScan for TreatmentSelectorScan {
    type Offset = u64;

    fn flush<E>(&self, _state: &mut Self::State, _signal: FlushReason<Self::Offset>, _emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
    }
}

impl SnapshottingScan for TreatmentSelectorScan {
    type Snapshot = TreatmentSelectorSnapshot;

    fn snapshot(&self, _state: &Self::State) -> Self::Snapshot {
        TreatmentSelectorSnapshot
    }

    fn restore(&self, _snapshot: Self::Snapshot) -> Self::State {
        TreatmentSelectorState
    }
}

impl VersionedSnapshot for TreatmentSelectorSnapshot {
    const VERSION: u32 = 1;
}
