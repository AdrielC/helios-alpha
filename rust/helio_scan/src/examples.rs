use serde::{Deserialize, Serialize};

use crate::control::FlushReason;
use crate::emit::Emit;
use crate::scan::{FlushableScan, Scan, SnapshottingScan, VersionedSnapshot};

/// Raw point event with a coarse time key (e.g. session day index).
#[derive(Debug, Clone, PartialEq)]
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
    start_day: i64,
    end_day: i64,
    peak_strength: f64,
    member_days: Vec<i64>,
}

/// Groups consecutive events when gaps between days are at most `max_gap_days`.
#[derive(Debug, Clone)]
pub struct EventClusterScan {
    pub max_gap_days: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventClusterState {
    open: Option<ClusterScratch>,
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

// --- Forward outcome ---

#[derive(Debug, Clone, PartialEq)]
pub enum MarketOrTreatment {
    Bar { day: i64, close: f64 },
    Treatment { id: u32, horizon_bars: u32 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForwardOutcome {
    pub treatment_id: u32,
    pub entry_day: i64,
    pub exit_day: i64,
    pub simple_return: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingTreatment {
    id: u32,
    horizon_bars: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActiveOutcome {
    id: u32,
    entry_day: i64,
    entry_close: f64,
    bars_remaining: u32,
}

#[derive(Debug, Clone)]
pub struct ForwardOutcomeScan;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForwardOutcomeState {
    pending: Vec<PendingTreatment>,
    active: Vec<ActiveOutcome>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForwardOutcomeSnapshot {
    pub pending: Vec<PendingTreatment>,
    pub active: Vec<ActiveOutcome>,
}

impl Scan for ForwardOutcomeScan {
    type In = MarketOrTreatment;
    type Out = ForwardOutcome;
    type State = ForwardOutcomeState;

    fn init(&self) -> Self::State {
        ForwardOutcomeState {
            pending: Vec::new(),
            active: Vec::new(),
        }
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        match input {
            MarketOrTreatment::Treatment { id, horizon_bars } => {
                state.pending.push(PendingTreatment { id, horizon_bars });
            }
            MarketOrTreatment::Bar { day, close } => {
                for p in state.pending.drain(..) {
                    state.active.push(ActiveOutcome {
                        id: p.id,
                        entry_day: day,
                        entry_close: close,
                        bars_remaining: p.horizon_bars,
                    });
                }
                let mut i = 0;
                while i < state.active.len() {
                    state.active[i].bars_remaining =
                        state.active[i].bars_remaining.saturating_sub(1);
                    if state.active[i].bars_remaining == 0 {
                        let a = state.active.swap_remove(i);
                        let simple_return = close / a.entry_close - 1.0;
                        emit.emit(ForwardOutcome {
                            treatment_id: a.id,
                            entry_day: a.entry_day,
                            exit_day: day,
                            simple_return,
                        });
                    } else {
                        i += 1;
                    }
                }
            }
        }
    }
}

impl FlushableScan for ForwardOutcomeScan {
    type Offset = u64;

    fn flush<E>(&self, state: &mut Self::State, _signal: FlushReason<Self::Offset>, _emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        // Pending / incomplete horizons are dropped; extend with explicit policy later.
        state.pending.clear();
    }
}

impl SnapshottingScan for ForwardOutcomeScan {
    type Snapshot = ForwardOutcomeSnapshot;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        ForwardOutcomeSnapshot {
            pending: state.pending.clone(),
            active: state.active.clone(),
        }
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        ForwardOutcomeState {
            pending: snapshot.pending,
            active: snapshot.active,
        }
    }
}

impl VersionedSnapshot for ForwardOutcomeSnapshot {
    const VERSION: u32 = 1;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combinator::{Then, ZipInput};
    use crate::emit::VecEmitter;
    use crate::focus::{Focus, ThenLeft, ThenRight, ZipInputA, ZipInputB};
    use crate::persist::{CheckpointKeyFn, HashMapStore, Persisted, SnapshotStore};
    use crate::runner::Runner;
    use crate::scan::Scan;
    use crate::ScanExt;

    /// Trivial scan for composition / focus tests.
    #[derive(Debug, Clone, Copy)]
    struct DoubleI32;

    impl Scan for DoubleI32 {
        type In = i32;
        type Out = i32;
        type State = ();

        fn init(&self) -> Self::State {}

        fn step<E>(&self, _state: &mut Self::State, input: Self::In, emit: &mut E)
        where
            E: Emit<Self::Out>,
        {
            emit.emit(input * 2);
        }
    }

    #[test]
    fn cluster_flushes_open_on_end_of_input() {
        let s = EventClusterScan { max_gap_days: 2 };
        let mut st = s.init();
        let mut e = VecEmitter::new();
        s.step(
            &mut st,
            RawEvent {
                day: 1,
                strength: 1.0,
            },
            &mut e,
        );
        s.flush(&mut st, FlushReason::EndOfInput, &mut e);
        assert_eq!(e.0.len(), 1);
        assert_eq!(e.0[0].start_day, 1);
    }

    #[test]
    fn forward_outcome_deterministic_after_restore() {
        let s = ForwardOutcomeScan;
        let mut st = s.init();
        let mut e = VecEmitter::new();
        let inputs = [
            MarketOrTreatment::Bar {
                day: 0,
                close: 100.0,
            },
            MarketOrTreatment::Treatment {
                id: 7,
                horizon_bars: 2,
            },
            MarketOrTreatment::Bar {
                day: 1,
                close: 101.0,
            },
            MarketOrTreatment::Bar {
                day: 2,
                close: 104.0,
            },
        ];
        for x in &inputs[..2] {
            s.step(&mut st, x.clone(), &mut e);
        }
        let snap = s.snapshot(&st);
        st = s.restore(snap.clone());
        for x in &inputs[2..] {
            s.step(&mut st, x.clone(), &mut e);
        }
        assert_eq!(e.0.len(), 1);
        let expected = 104.0_f64 / 101.0_f64 - 1.0;
        assert!((e.0[0].simple_return - expected).abs() < 1e-9);
        let mut st2 = s.restore(snap);
        let mut e2 = VecEmitter::new();
        for x in &inputs[2..] {
            s.step(&mut st2, x.clone(), &mut e2);
        }
        assert_eq!(e.0, e2.0);
    }

    #[test]
    fn map_preserves_state_shape() {
        let pipe = EventClusterScan { max_gap_days: 1 }.map(|c| c.start_day);
        let mut st = pipe.init();
        let mut e = VecEmitter::new();
        pipe.step(
            &mut st,
            RawEvent {
                day: 1,
                strength: 2.0,
            },
            &mut e,
        );
        pipe.step(
            &mut st,
            RawEvent {
                day: 5,
                strength: 1.0,
            },
            &mut e,
        );
        assert_eq!(e.0.len(), 1);
        assert_eq!(e.0[0], 1);
        assert!(pipe.snapshot(&st).open.is_some());
    }

    #[test]
    fn zip_input_emits_both_branches() {
        let z = ZipInput {
            a: EventClusterScan { max_gap_days: 10 },
            b: EventClusterScan { max_gap_days: 2 },
        };
        let mut st = z.init();
        let mut e = VecEmitter::new();
        let bar = RawEvent {
            day: 1,
            strength: 1.0,
        };
        z.step(&mut st, bar.clone(), &mut e);
        assert!(e.0.is_empty());
        z.flush(&mut st, FlushReason::EndOfInput, &mut e);
        assert_eq!(e.0.len(), 2);
        let a = ZipInputA;
        let _as: &EventClusterState = a.get(&st);
        let b = ZipInputB;
        let _bs: &EventClusterState = b.get(&st);
    }

    #[test]
    fn persisted_checkpoint_roundtrip() {
        #[derive(Clone)]
        struct KeyU64;
        impl CheckpointKeyFn<u64> for KeyU64 {
            type Key = &'static str;
            fn key_for_offset(&self, _offset: &u64) -> Self::Key {
                "main"
            }
        }

        let inner = EventClusterScan { max_gap_days: 1 };
        let persisted = Persisted::new(inner, HashMapStore::default(), KeyU64);
        let mut r = Runner::new(persisted);
        let mut e = VecEmitter::new();
        r.step(
            RawEvent {
                day: 1,
                strength: 1.0,
            },
            &mut e,
        );
        r.flush(FlushReason::Checkpoint(42u64), &mut e);
        let cp = r.machine.store.borrow_mut().get(&"main").unwrap().unwrap();
        assert_eq!(cp.offset, 42);
        r.state = r.machine.restore(cp.snapshot.clone());
        r.step(
            RawEvent {
                day: 3,
                strength: 1.0,
            },
            &mut e,
        );
        r.flush(FlushReason::EndOfInput, &mut e);
        assert_eq!(e.0.len(), 2);
    }

    #[test]
    fn then_compose_focus_left_right() {
        let pipe = Then {
            left: DoubleI32,
            right: DoubleI32,
        };
        let mut st = pipe.init();
        let mut e = VecEmitter::new();
        pipe.step(&mut st, 3, &mut e);
        assert_eq!(e.0, vec![12]);
        let tr = ThenRight;
        let _: &() = tr.get(&st);
        let tl = ThenLeft;
        let _: &() = tl.get(&st);
    }

    #[test]
    fn nested_zip_then_state_accessible_by_field() {
        let pipe = ZipInput {
            a: Then {
                left: DoubleI32,
                right: DoubleI32,
            },
            b: DoubleI32,
        };
        let st = pipe.init();
        let z = ZipInputA;
        let t = ThenLeft;
        let _: &() = t.get(z.get(&st));
    }
}
