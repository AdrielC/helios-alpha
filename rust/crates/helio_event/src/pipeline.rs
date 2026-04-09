use helio_scan::{Emit, FlushReason, FlushableScan, Scan, SnapshottingScan, VersionedSnapshot};
use helio_window::{
    EventClusterScan, EventClusterState, ForwardHorizonScan, ForwardHorizonState, HorizonInput,
    RawEvent,
};
use serde::{Deserialize, Serialize};

use crate::{TreatmentEvent, TreatmentSelectorScan, TreatmentSelectorState};

/// After [`TreatmentSelectorScan`], cluster overlapping treatments (same geometry as [`RawEvent`]).
#[derive(Debug, Clone)]
pub struct ClusteredTreatmentScan {
    pub inner: EventClusterScan,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClusteredTreatmentState(pub EventClusterState);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClusteredTreatmentSnapshot(pub helio_window::EventClusterSnapshot);

impl Scan for ClusteredTreatmentScan {
    type In = TreatmentEvent;
    type Out = helio_window::ClusteredEvent;
    type State = ClusteredTreatmentState;

    fn init(&self) -> Self::State {
        ClusteredTreatmentState(self.inner.init())
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        let raw = RawEvent {
            day: input.day,
            strength: input.strength,
        };
        self.inner.step(&mut state.0, raw, emit);
    }
}

impl FlushableScan for ClusteredTreatmentScan {
    type Offset = u64;

    fn flush<E>(&self, state: &mut Self::State, signal: FlushReason<Self::Offset>, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        self.inner.flush(&mut state.0, signal, emit);
    }
}

impl SnapshottingScan for ClusteredTreatmentScan {
    type Snapshot = ClusteredTreatmentSnapshot;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        ClusteredTreatmentSnapshot(self.inner.snapshot(&state.0))
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        ClusteredTreatmentState(self.inner.restore(snapshot.0))
    }
}

impl VersionedSnapshot for ClusteredTreatmentSnapshot {
    const VERSION: u32 = 1;
}

/// Maps a finalized cluster to a synthetic treatment id (start day) for horizon labeling.
#[derive(Debug, Clone, Copy, Default)]
pub struct ClusterToHorizonScan;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClusterToHorizonState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClusterToHorizonSnapshot;

impl Scan for ClusterToHorizonScan {
    type In = helio_window::ClusteredEvent;
    type Out = HorizonInput;
    type State = ClusterToHorizonState;

    fn init(&self) -> Self::State {
        ClusterToHorizonState
    }

    fn step<E>(&self, _state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        let horizon = (input.end_day - input.start_day).max(1) as u32;
        emit.emit(HorizonInput::Treatment {
            id: input.start_day as u32,
            horizon_trading_days: horizon,
        });
    }
}

impl FlushableScan for ClusterToHorizonScan {
    type Offset = u64;

    fn flush<E>(&self, _state: &mut Self::State, _signal: FlushReason<Self::Offset>, _emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
    }
}

impl SnapshottingScan for ClusterToHorizonScan {
    type Snapshot = ClusterToHorizonSnapshot;

    fn snapshot(&self, _state: &Self::State) -> Self::Snapshot {
        ClusterToHorizonSnapshot
    }

    fn restore(&self, _snapshot: Self::Snapshot) -> Self::State {
        ClusterToHorizonState
    }
}

impl VersionedSnapshot for ClusterToHorizonSnapshot {
    const VERSION: u32 = 1;
}

/// Pairs [`TreatmentSelectorScan`] then [`ClusteredTreatmentScan`] with one named state struct.
#[derive(Debug, Clone)]
pub struct AvailabilityClusterPipeline {
    pub select: TreatmentSelectorScan,
    pub cluster: ClusteredTreatmentScan,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AvailabilityClusterState {
    pub select: TreatmentSelectorState,
    pub cluster: ClusteredTreatmentState,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AvailabilityClusterSnapshot {
    pub select: crate::selector::TreatmentSelectorSnapshot,
    pub cluster: ClusteredTreatmentSnapshot,
}

impl Scan for AvailabilityClusterPipeline {
    type In = crate::AvailabilityTagged<TreatmentEvent>;
    type Out = helio_window::ClusteredEvent;
    type State = AvailabilityClusterState;

    fn init(&self) -> Self::State {
        AvailabilityClusterState {
            select: self.select.init(),
            cluster: self.cluster.init(),
        }
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        let mut bridge = helio_scan::VecEmitter::new();
        self.select.step(&mut state.select, input, &mut bridge);
        for ev in bridge.into_inner() {
            self.cluster.step(&mut state.cluster, ev, emit);
        }
    }
}

impl FlushableScan for AvailabilityClusterPipeline {
    type Offset = u64;

    fn flush<E>(&self, state: &mut Self::State, signal: FlushReason<Self::Offset>, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        self.select.flush(
            &mut state.select,
            signal.clone(),
            &mut helio_scan::VecEmitter::new(),
        );
        self.cluster.flush(&mut state.cluster, signal, emit);
    }
}

impl SnapshottingScan for AvailabilityClusterPipeline {
    type Snapshot = AvailabilityClusterSnapshot;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        AvailabilityClusterSnapshot {
            select: self.select.snapshot(&state.select),
            cluster: self.cluster.snapshot(&state.cluster),
        }
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        AvailabilityClusterState {
            select: self.select.restore(snapshot.select),
            cluster: self.cluster.restore(snapshot.cluster),
        }
    }
}

impl VersionedSnapshot for AvailabilityClusterSnapshot {
    const VERSION: u32 = 1;
}

/// Full causal labeling slice: **availability gate** → **immediate** forward horizon spawn on each
/// selected treatment; **overlap clusterer** runs in parallel on the same stream (state kept for
/// snapshot / diagnostics — cluster outputs on `step` are discarded so labeling is not delayed until
/// cluster close).
#[derive(Debug, Clone, Copy)]
pub struct CausalEventStudyConfig {
    pub decision_available: helio_time::AvailableAt,
    pub overlap: crate::OverlapConfig,
}

#[derive(Debug, Clone)]
pub struct CausalEventStudyPipeline {
    pub select: TreatmentSelectorScan,
    pub cluster: ClusteredTreatmentScan,
    pub forward: ForwardHorizonScan,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CausalEventStudyState {
    pub select: TreatmentSelectorState,
    pub cluster: ClusteredTreatmentState,
    pub forward: ForwardHorizonState,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CausalEventStudySnapshot {
    pub select: crate::selector::TreatmentSelectorSnapshot,
    pub cluster: ClusteredTreatmentSnapshot,
    pub forward: helio_window::ForwardHorizonSnapshot,
}

impl CausalEventStudyPipeline {
    pub fn new(cfg: CausalEventStudyConfig) -> Self {
        Self {
            select: TreatmentSelectorScan {
                decision_available: cfg.decision_available,
            },
            cluster: ClusteredTreatmentScan {
                inner: EventClusterScan {
                    max_gap_days: cfg.overlap.max_gap_days,
                },
            },
            forward: ForwardHorizonScan::default(),
        }
    }
}

/// Unified stream for the golden-path replay harness.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ReplayRecord {
    Treatment(crate::AvailabilityTagged<TreatmentEvent>),
    Bar { session_day: i32, close: f64 },
}

impl Scan for CausalEventStudyPipeline {
    type In = ReplayRecord;
    type Out = helio_window::ForwardHorizonOutput;
    type State = CausalEventStudyState;

    fn init(&self) -> Self::State {
        CausalEventStudyState {
            select: self.select.init(),
            cluster: self.cluster.init(),
            forward: self.forward.init(),
        }
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        match input {
            ReplayRecord::Treatment(t) => {
                let mut selected = helio_scan::VecEmitter::new();
                self.select.step(&mut state.select, t, &mut selected);
                for ev in selected.into_inner() {
                    self.forward.step(
                        &mut state.forward,
                        HorizonInput::Treatment {
                            id: ev.id,
                            horizon_trading_days: ev.horizon_trading_days,
                        },
                        emit,
                    );
                    let mut drop = helio_scan::VecEmitter::new();
                    self.cluster.step(&mut state.cluster, ev, &mut drop);
                }
            }
            ReplayRecord::Bar { session_day, close } => {
                self.forward.step(
                    &mut state.forward,
                    HorizonInput::Bar { session_day, close },
                    emit,
                );
            }
        }
    }
}

impl FlushableScan for CausalEventStudyPipeline {
    type Offset = u64;

    fn flush<E>(&self, state: &mut Self::State, signal: FlushReason<Self::Offset>, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        self.select.flush(
            &mut state.select,
            signal.clone(),
            &mut helio_scan::VecEmitter::new(),
        );
        let mut cluster_out = helio_scan::VecEmitter::new();
        self.cluster
            .flush(&mut state.cluster, signal.clone(), &mut cluster_out);
        let _ = cluster_out.into_inner();
        self.forward.flush(&mut state.forward, signal, emit);
    }
}

impl SnapshottingScan for CausalEventStudyPipeline {
    type Snapshot = CausalEventStudySnapshot;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        CausalEventStudySnapshot {
            select: self.select.snapshot(&state.select),
            cluster: self.cluster.snapshot(&state.cluster),
            forward: self.forward.snapshot(&state.forward),
        }
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        CausalEventStudyState {
            select: self.select.restore(snapshot.select),
            cluster: self.cluster.restore(snapshot.cluster),
            forward: self.forward.restore(snapshot.forward),
        }
    }
}

impl VersionedSnapshot for CausalEventStudySnapshot {
    const VERSION: u32 = 1;
}
