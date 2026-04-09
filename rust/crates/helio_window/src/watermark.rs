use helio_scan::{Emit, FlushReason, FlushableScan, Scan, SnapshottingScan, VersionedSnapshot};
use serde::{Deserialize, Serialize};

/// Event-time accessor for watermark finalization.
pub trait WatermarkTime {
    fn event_time(&self) -> i64;
}

/// Buffers inputs; on [`FlushReason::Watermark`], emits and removes all items with
/// `event_time <= watermark` (stable order: FIFO).
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
            for x in std::mem::take(&mut state.pending) {
                if x.event_time() <= w {
                    emit.emit(x);
                } else {
                    kept.push(x);
                }
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
