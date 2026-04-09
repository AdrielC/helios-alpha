use helio_scan::{Emit, FlushReason, FlushableScan, Scan, SnapshottingScan, VersionedSnapshot};
use serde::{Deserialize, Serialize};

/// Raw point event with a coarse time key (e.g. session day index).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawEvent {
    pub day: i64,
    pub strength: f64,
}

/// Finalized cluster after a gap larger than `max_gap_days`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClusteredEvent {
    pub start_day: i64,
    pub end_day: i64,
    pub peak_strength: f64,
    pub member_days: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClusterScratch {
    pub start_day: i64,
    pub end_day: i64,
    pub peak_strength: f64,
    pub member_days: Vec<i64>,
}

/// Groups consecutive events when gaps between days are at most `max_gap_days`.
#[derive(Debug, Clone)]
pub struct EventClusterScan {
    pub max_gap_days: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventClusterState {
    pub open: Option<ClusterScratch>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventClusterSnapshot {
    pub open: Option<ClusterScratch>,
}

impl Scan for EventClusterScan {
    type In = RawEvent;
    type Out = ClusteredEvent;
    type State = EventClusterState;

    fn init(&self) -> Self::State {
        EventClusterState { open: None }
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        match &mut state.open {
            None => {
                state.open = Some(ClusterScratch {
                    start_day: input.day,
                    end_day: input.day,
                    peak_strength: input.strength,
                    member_days: vec![input.day],
                });
            }
            Some(c) => {
                if input.day - c.end_day <= self.max_gap_days {
                    c.end_day = input.day;
                    c.member_days.push(input.day);
                    if input.strength > c.peak_strength {
                        c.peak_strength = input.strength;
                    }
                } else {
                    let Some(done) = state.open.take() else {
                        return;
                    };
                    emit.emit(ClusteredEvent {
                        start_day: done.start_day,
                        end_day: done.end_day,
                        peak_strength: done.peak_strength,
                        member_days: done.member_days,
                    });
                    state.open = Some(ClusterScratch {
                        start_day: input.day,
                        end_day: input.day,
                        peak_strength: input.strength,
                        member_days: vec![input.day],
                    });
                }
            }
        }
    }
}

impl FlushableScan for EventClusterScan {
    type Offset = u64;

    fn flush<E>(&self, state: &mut Self::State, signal: FlushReason<Self::Offset>, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        let finalize = matches!(
            signal,
            FlushReason::EndOfInput
                | FlushReason::Shutdown
                | FlushReason::SessionClose(_)
                | FlushReason::Manual
        );
        if finalize {
            if let Some(done) = state.open.take() {
                emit.emit(ClusteredEvent {
                    start_day: done.start_day,
                    end_day: done.end_day,
                    peak_strength: done.peak_strength,
                    member_days: done.member_days,
                });
            }
        }
    }
}

impl SnapshottingScan for EventClusterScan {
    type Snapshot = EventClusterSnapshot;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        EventClusterSnapshot {
            open: state.open.clone(),
        }
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        EventClusterState {
            open: snapshot.open,
        }
    }
}

impl VersionedSnapshot for EventClusterSnapshot {
    const VERSION: u32 = 1;
}
