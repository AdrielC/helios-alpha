use std::collections::BTreeMap;

use helio_scan::{
    Emit, FallibleRestoreScan, FlushReason, FlushableScan, Scan, SnapshottingScan,
    VersionedSnapshot,
};
use serde::{Deserialize, Serialize};

/// Event-time accessor for watermark finalization.
pub trait WatermarkTime {
    fn event_time(&self) -> i64;
}

/// Legacy unbounded watermark buffer. Prefer [`EventTimeReorderScan`] for bounded state, ordered
/// event-time output, and explicit late/overflow errors.
#[derive(Debug, Clone, Copy, Default)]
pub struct WatermarkFinalizeScan<T> {
    _p: std::marker::PhantomData<T>,
}

impl<T> WatermarkFinalizeScan<T> {
    pub fn new() -> Self {
        Self {
            _p: std::marker::PhantomData,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatermarkFinalizeState<T> {
    pub pending: Vec<T>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatermarkFinalizeSnapshot<T> {
    pub pending: Vec<T>,
}

impl<T: Clone + WatermarkTime> Scan for WatermarkFinalizeScan<T> {
    type In = T;
    type Out = T;
    type State = WatermarkFinalizeState<T>;

    fn init(&self) -> Self::State {
        WatermarkFinalizeState {
            pending: Vec::new(),
        }
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        state.pending.push(input);
        let _ = emit;
    }
}

impl<T: Clone + WatermarkTime> FlushableScan for WatermarkFinalizeScan<T> {
    type Offset = i64;

    fn flush<E>(&self, state: &mut Self::State, signal: FlushReason<Self::Offset>, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        if let FlushReason::Watermark(w) = signal {
            let mut kept = Vec::new();
            let mut ready = Vec::new();
            for x in std::mem::take(&mut state.pending) {
                if x.event_time() <= w {
                    ready.push(x);
                } else {
                    kept.push(x);
                }
            }
            ready.sort_by_key(WatermarkTime::event_time);
            for x in ready {
                emit.emit(x);
            }
            state.pending = kept;
        }
    }
}

impl<T: Clone + WatermarkTime + Serialize + for<'de> Deserialize<'de>> SnapshottingScan
    for WatermarkFinalizeScan<T>
{
    type Snapshot = WatermarkFinalizeSnapshot<T>;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        WatermarkFinalizeSnapshot {
            pending: state.pending.clone(),
        }
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        WatermarkFinalizeState {
            pending: snapshot.pending,
        }
    }
}

impl<T: Serialize + for<'de> Deserialize<'de>> VersionedSnapshot for WatermarkFinalizeSnapshot<T> {
    const VERSION: u32 = 1;
}

/// Construction error for a bounded reorder buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReorderBuildError {
    ZeroCapacity,
}

/// Structural error in a reorder snapshot loaded from external storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReorderRestoreError {
    CapacityExceeded { pending: usize, capacity: usize },
    EventTimeMismatch { stored: i64, actual: i64 },
    AtOrBeforeWatermark { event_time: i64, watermark: i64 },
    SequenceNotBeforeNext { sequence: u64, next_sequence: u64 },
    DuplicateKey { event_time: i64, sequence: u64 },
}

/// Data and control outcomes from [`EventTimeReorderScan`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReorderOutput<T> {
    Ready(T),
    Late { item: T, watermark: i64 },
    Overflow { item: T, capacity: usize },
    SequenceExhausted { item: T },
    RegressedWatermark { attempted: i64, current: i64 },
}

/// Bounded event-time reorder buffer.
///
/// Inputs are retained in `(event_time, arrival_sequence)` order. A watermark emits all ready
/// inputs in event-time order with stable arrival order for ties. Inputs at or before the accepted
/// watermark and inputs exceeding capacity are returned explicitly instead of being silently
/// dropped. Checkpoints do not drain the buffer.
#[derive(Debug, Clone, Copy)]
pub struct EventTimeReorderScan<T> {
    max_pending: usize,
    _input: std::marker::PhantomData<T>,
}

impl<T> EventTimeReorderScan<T> {
    pub fn try_new(max_pending: usize) -> Result<Self, ReorderBuildError> {
        if max_pending == 0 {
            return Err(ReorderBuildError::ZeroCapacity);
        }
        Ok(Self {
            max_pending,
            _input: std::marker::PhantomData,
        })
    }

    pub const fn max_pending(&self) -> usize {
        self.max_pending
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventTimeReorderState<T> {
    pending: BTreeMap<(i64, u64), T>,
    next_sequence: u64,
    watermark: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventTimeReorderSnapshot<T> {
    pub pending: Vec<(i64, u64, T)>,
    pub next_sequence: u64,
    pub watermark: Option<i64>,
}

impl<T: WatermarkTime> Scan for EventTimeReorderScan<T> {
    type In = T;
    type Out = ReorderOutput<T>;
    type State = EventTimeReorderState<T>;

    fn init(&self) -> Self::State {
        EventTimeReorderState {
            pending: BTreeMap::new(),
            next_sequence: 0,
            watermark: None,
        }
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        if let Some(watermark) = state.watermark {
            if input.event_time() <= watermark {
                emit.emit(ReorderOutput::Late {
                    item: input,
                    watermark,
                });
                return;
            }
        }
        if state.pending.len() >= self.max_pending {
            emit.emit(ReorderOutput::Overflow {
                item: input,
                capacity: self.max_pending,
            });
            return;
        }
        let Some(next_sequence) = state.next_sequence.checked_add(1) else {
            emit.emit(ReorderOutput::SequenceExhausted { item: input });
            return;
        };
        let key = (input.event_time(), state.next_sequence);
        state.pending.insert(key, input);
        state.next_sequence = next_sequence;
    }
}

impl<T: WatermarkTime> EventTimeReorderScan<T> {
    fn drain_through<E>(state: &mut EventTimeReorderState<T>, watermark: i64, emit: &mut E)
    where
        E: Emit<ReorderOutput<T>>,
    {
        while state
            .pending
            .first_key_value()
            .is_some_and(|((event_time, _), _)| *event_time <= watermark)
        {
            if let Some((_, item)) = state.pending.pop_first() {
                emit.emit(ReorderOutput::Ready(item));
            }
        }
    }

    fn drain_all<E>(state: &mut EventTimeReorderState<T>, emit: &mut E)
    where
        E: Emit<ReorderOutput<T>>,
    {
        while let Some((_, item)) = state.pending.pop_first() {
            emit.emit(ReorderOutput::Ready(item));
        }
    }
}

impl<T: WatermarkTime> FlushableScan for EventTimeReorderScan<T> {
    type Offset = i64;

    fn flush<E>(&self, state: &mut Self::State, signal: FlushReason<Self::Offset>, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        match signal {
            FlushReason::Watermark(next) => {
                if let Some(current) = state.watermark {
                    if next < current {
                        emit.emit(ReorderOutput::RegressedWatermark {
                            attempted: next,
                            current,
                        });
                        return;
                    }
                }
                state.watermark = Some(next);
                Self::drain_through(state, next, emit);
            }
            FlushReason::Shutdown | FlushReason::EndOfInput | FlushReason::Manual => {
                Self::drain_all(state, emit);
            }
            FlushReason::SessionClose(_) | FlushReason::Checkpoint(_) | FlushReason::Rebalance => {}
        }
    }
}

impl<T> SnapshottingScan for EventTimeReorderScan<T>
where
    T: Clone + WatermarkTime + Serialize + for<'de> Deserialize<'de>,
{
    type Snapshot = EventTimeReorderSnapshot<T>;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        EventTimeReorderSnapshot {
            pending: state
                .pending
                .iter()
                .map(|(&(event_time, sequence), item)| (event_time, sequence, item.clone()))
                .collect(),
            next_sequence: state.next_sequence,
            watermark: state.watermark,
        }
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        EventTimeReorderState {
            pending: snapshot
                .pending
                .into_iter()
                .map(|(event_time, sequence, item)| ((event_time, sequence), item))
                .collect(),
            next_sequence: snapshot.next_sequence,
            watermark: snapshot.watermark,
        }
    }
}

impl<T> FallibleRestoreScan for EventTimeReorderScan<T>
where
    T: Clone + WatermarkTime + Serialize + for<'de> Deserialize<'de>,
{
    type RestoreError = ReorderRestoreError;

    fn try_restore(&self, snapshot: Self::Snapshot) -> Result<Self::State, Self::RestoreError> {
        if snapshot.pending.len() > self.max_pending {
            return Err(ReorderRestoreError::CapacityExceeded {
                pending: snapshot.pending.len(),
                capacity: self.max_pending,
            });
        }
        let mut pending = BTreeMap::new();
        for (event_time, sequence, item) in snapshot.pending {
            let actual = item.event_time();
            if actual != event_time {
                return Err(ReorderRestoreError::EventTimeMismatch {
                    stored: event_time,
                    actual,
                });
            }
            if let Some(watermark) = snapshot.watermark {
                if event_time <= watermark {
                    return Err(ReorderRestoreError::AtOrBeforeWatermark {
                        event_time,
                        watermark,
                    });
                }
            }
            if sequence >= snapshot.next_sequence {
                return Err(ReorderRestoreError::SequenceNotBeforeNext {
                    sequence,
                    next_sequence: snapshot.next_sequence,
                });
            }
            if pending.insert((event_time, sequence), item).is_some() {
                return Err(ReorderRestoreError::DuplicateKey {
                    event_time,
                    sequence,
                });
            }
        }
        Ok(EventTimeReorderState {
            pending,
            next_sequence: snapshot.next_sequence,
            watermark: snapshot.watermark,
        })
    }
}

impl<T> VersionedSnapshot for EventTimeReorderSnapshot<T> {
    const VERSION: u32 = 1;
}

#[cfg(test)]
mod tests {
    use super::*;
    use helio_scan::{FlushReason, FlushableScan, Scan, SnapshottingScan, VecEmitter};

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct Item {
        time: i64,
        id: u8,
    }

    impl WatermarkTime for Item {
        fn event_time(&self) -> i64 {
            self.time
        }
    }

    #[test]
    fn bounded_reorder_emits_event_time_order_and_stable_ties() {
        let scan = EventTimeReorderScan::try_new(4).unwrap();
        let mut state = scan.init();
        let mut emit = VecEmitter::new();
        for item in [
            Item { time: 20, id: 2 },
            Item { time: 10, id: 1 },
            Item { time: 20, id: 3 },
        ] {
            scan.step(&mut state, item, &mut emit);
        }
        scan.flush(&mut state, FlushReason::Watermark(20), &mut emit);
        let ids: Vec<u8> = emit
            .0
            .into_iter()
            .filter_map(|output| match output {
                ReorderOutput::Ready(item) => Some(item.id),
                _ => None,
            })
            .collect();
        assert_eq!(ids, vec![1, 2, 3]);
    }

    #[test]
    fn late_and_overflow_inputs_are_explicit() {
        let scan = EventTimeReorderScan::try_new(1).unwrap();
        let mut state = scan.init();
        let mut emit = VecEmitter::new();
        scan.step(&mut state, Item { time: 20, id: 1 }, &mut emit);
        scan.step(&mut state, Item { time: 30, id: 2 }, &mut emit);
        assert!(matches!(emit.0[0], ReorderOutput::Overflow { .. }));
        scan.flush(&mut state, FlushReason::Watermark(20), &mut emit);
        scan.step(&mut state, Item { time: 19, id: 3 }, &mut emit);
        assert!(matches!(emit.0.last(), Some(ReorderOutput::Late { .. })));
    }

    #[test]
    fn checkpoint_restore_preserves_pending_order() {
        let scan = EventTimeReorderScan::try_new(4).unwrap();
        let mut state = scan.init();
        let mut emit = VecEmitter::new();
        scan.step(&mut state, Item { time: 20, id: 2 }, &mut emit);
        scan.step(&mut state, Item { time: 10, id: 1 }, &mut emit);
        scan.flush(&mut state, FlushReason::Checkpoint(2), &mut emit);
        assert!(emit.0.is_empty());

        let mut restored = scan.restore(scan.snapshot(&state));
        scan.flush(&mut restored, FlushReason::EndOfInput, &mut emit);
        let ids: Vec<u8> = emit
            .0
            .into_iter()
            .filter_map(|output| match output {
                ReorderOutput::Ready(item) => Some(item.id),
                _ => None,
            })
            .collect();
        assert_eq!(ids, vec![1, 2]);
    }

    #[test]
    fn fallible_restore_rejects_corrupt_snapshot() {
        let scan = EventTimeReorderScan::try_new(2).unwrap();
        let snapshot = EventTimeReorderSnapshot {
            pending: vec![(10, 0, Item { time: 11, id: 1 })],
            next_sequence: 1,
            watermark: None,
        };
        assert_eq!(
            scan.try_restore(snapshot),
            Err(ReorderRestoreError::EventTimeMismatch {
                stored: 10,
                actual: 11,
            })
        );
    }
}
