use helio_scan::{Emit, FlushReason, FlushableScan, Scan, SnapshottingScan, VersionedSnapshot};
use helio_window::ForwardHorizonOutput;
use serde::{Deserialize, Serialize};

/// Running summary over **complete** forward outcomes only.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EventStudySummary {
    pub count: u64,
    pub mean_simple_return: f64,
}

/// Incremental fold: updates summary on each [`ForwardHorizonOutput::Complete`].
#[derive(Debug, Clone, Copy, Default)]
pub struct EventStudyFoldScan;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventStudyFoldState {
    pub summary: EventStudySummary,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventStudyFoldSnapshot {
    pub summary: EventStudySummary,
}

impl Scan for EventStudyFoldScan {
    type In = ForwardHorizonOutput;
    type Out = EventStudySummary;
    type State = EventStudyFoldState;

    fn init(&self) -> Self::State {
        EventStudyFoldState {
            summary: EventStudySummary {
                count: 0,
                mean_simple_return: 0.0,
            },
        }
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        if let ForwardHorizonOutput::Complete(c) = input {
            let n = state.summary.count + 1;
            let prev_mean = state.summary.mean_simple_return;
            let x = c.simple_return;
            let new_mean = prev_mean + (x - prev_mean) / n as f64;
            state.summary.count = n;
            state.summary.mean_simple_return = new_mean;
            emit.emit(state.summary.clone());
        }
    }
}

impl FlushableScan for EventStudyFoldScan {
    type Offset = u64;

    fn flush<E>(&self, _state: &mut Self::State, _signal: FlushReason<Self::Offset>, _emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
    }
}

impl SnapshottingScan for EventStudyFoldScan {
    type Snapshot = EventStudyFoldSnapshot;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        EventStudyFoldSnapshot {
            summary: state.summary.clone(),
        }
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        EventStudyFoldState {
            summary: snapshot.summary,
        }
    }
}

impl VersionedSnapshot for EventStudyFoldSnapshot {
    const VERSION: u32 = 1;
}
