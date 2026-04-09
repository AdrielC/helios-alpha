use helio_scan::{Emit, FlushReason, FlushableScan, Scan, SnapshottingScan, VersionedSnapshot};
use serde::{Deserialize, Serialize};

/// Emit the **previous** input on each step (first step emits nothing).
#[derive(Debug, Clone, Copy, Default)]
pub struct LagScan<T> {
    _p: std::marker::PhantomData<T>,
}

impl<T> LagScan<T> {
    pub fn new() -> Self {
        Self {
            _p: std::marker::PhantomData,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LagState<T> {
    pub prev: Option<T>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LagSnapshot<T> {
    pub prev: Option<T>,
}

impl<T: Clone> Scan for LagScan<T> {
    type In = T;
    type Out = T;
    type State = LagState<T>;

    fn init(&self) -> Self::State {
        LagState { prev: None }
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        if let Some(p) = state.prev.clone() {
            emit.emit(p);
        }
        state.prev = Some(input);
    }
}

impl<T: Clone> FlushableScan for LagScan<T> {
    type Offset = u64;

    fn flush<E>(&self, _state: &mut Self::State, _signal: FlushReason<Self::Offset>, _emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
    }
}

impl<T: Clone + Serialize + for<'de> Deserialize<'de>> SnapshottingScan for LagScan<T> {
    type Snapshot = LagSnapshot<T>;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        LagSnapshot {
            prev: state.prev.clone(),
        }
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        LagState {
            prev: snapshot.prev,
        }
    }
}

impl<T: Serialize + for<'de> Deserialize<'de>> VersionedSnapshot for LagSnapshot<T> {
    const VERSION: u32 = 1;
}
