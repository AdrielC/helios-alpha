use std::collections::VecDeque;
use std::marker::PhantomData;

use helio_scan::{Emit, FlushReason, FlushableScan, Scan, SnapshottingScan, VersionedSnapshot};
use serde::{Deserialize, Serialize};

/// Fixed-size FIFO window; emits **full snapshots** (oldest→newest) when length reaches `max_len`.
#[derive(Debug, Clone)]
pub struct RollingWindowScan<T> {
    pub max_len: usize,
    _p: PhantomData<T>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RollingWindowState<T> {
    buf: VecDeque<T>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RollingWindowSnapshot<T> {
    pub buf: Vec<T>,
}

impl<T: Clone> RollingWindowScan<T> {
    pub fn new(max_len: usize) -> Self {
        Self {
            max_len,
            _p: PhantomData,
        }
    }
}

impl<T: Clone> Scan for RollingWindowScan<T> {
    type In = T;
    type Out = Vec<T>;
    type State = RollingWindowState<T>;

    fn init(&self) -> Self::State {
        RollingWindowState {
            buf: VecDeque::with_capacity(self.max_len),
        }
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        if self.max_len == 0 {
            return;
        }
        state.buf.push_back(input);
        while state.buf.len() > self.max_len {
            state.buf.pop_front();
        }
        if state.buf.len() == self.max_len {
            emit.emit(state.buf.iter().cloned().collect());
        }
    }
}

impl<T: Clone> FlushableScan for RollingWindowScan<T> {
    type Offset = u64;

    fn flush<E>(&self, _state: &mut Self::State, _signal: FlushReason<Self::Offset>, _emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
    }
}

impl<T: Clone + Serialize + for<'de> Deserialize<'de>> SnapshottingScan for RollingWindowScan<T> {
    type Snapshot = RollingWindowSnapshot<T>;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        RollingWindowSnapshot {
            buf: state.buf.iter().cloned().collect(),
        }
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        RollingWindowState {
            buf: snapshot.buf.into_iter().collect(),
        }
    }
}

impl<T> VersionedSnapshot for RollingWindowSnapshot<T> {
    const VERSION: u32 = 1;
}
