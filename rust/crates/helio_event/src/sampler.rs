use helio_scan::{Emit, FlushReason, FlushableScan, Scan, SnapshottingScan, VersionedSnapshot};
use helio_window::ForwardHorizonOutcome;
use serde::{Deserialize, Serialize};

use crate::{ControlEvent, MatchingConfig};

/// Deterministic pseudo-controls: one control per completed outcome (id = treatment_id + 10_000).
#[derive(Debug, Clone)]
pub struct MatchedControlSampler {
    pub config: MatchingConfig,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchedControlSamplerState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchedControlSamplerSnapshot;

impl Scan for MatchedControlSampler {
    type In = ForwardHorizonOutcome;
    type Out = ControlEvent;
    type State = MatchedControlSamplerState;

    fn init(&self) -> Self::State {
        MatchedControlSamplerState
    }

    fn step<E>(&self, _state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        for i in 0..self.config.controls_per_treatment {
            emit.emit(ControlEvent {
                id: input.treatment_id.wrapping_add(10_000).wrapping_add(i),
                matched_to_treatment: input.treatment_id,
                session_day: input.exit_session_day,
            });
        }
    }
}

impl FlushableScan for MatchedControlSampler {
    type Offset = u64;

    fn flush<E>(&self, _state: &mut Self::State, _signal: FlushReason<Self::Offset>, _emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
    }
}

impl SnapshottingScan for MatchedControlSampler {
    type Snapshot = MatchedControlSamplerSnapshot;

    fn snapshot(&self, _state: &Self::State) -> Self::Snapshot {
        MatchedControlSamplerSnapshot
    }

    fn restore(&self, _snapshot: Self::Snapshot) -> Self::State {
        MatchedControlSamplerState
    }
}

impl VersionedSnapshot for MatchedControlSamplerSnapshot {
    const VERSION: u32 = 1;
}
