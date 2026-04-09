use helio_scan::{Emit, FlushReason, FlushableScan, Scan, SnapshottingScan, VersionedSnapshot};
use serde::{Deserialize, Serialize};

/// Interleaved left/right updates; emits `(L, R)` whenever both sides have a value after an update.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum JoinLatestIn<L, R> {
    Left(L),
    Right(R),
}

#[derive(Debug, Clone)]
pub struct JoinLatestScan<L, R> {
    _p: std::marker::PhantomData<(L, R)>,
}

impl<L, R> Default for JoinLatestScan<L, R> {
    fn default() -> Self {
        Self::new()
    }
}

impl<L, R> JoinLatestScan<L, R> {
    pub fn new() -> Self {
        Self {
            _p: std::marker::PhantomData,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JoinLatestState<L, R> {
    pub left: Option<L>,
    pub right: Option<R>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JoinLatestSnapshot<L, R> {
    pub left: Option<L>,
    pub right: Option<R>,
}

impl<L: Clone, R: Clone> Scan for JoinLatestScan<L, R> {
    type In = JoinLatestIn<L, R>;
    type Out = (L, R);
    type State = JoinLatestState<L, R>;

    fn init(&self) -> Self::State {
        JoinLatestState {
            left: None,
            right: None,
        }
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        match input {
            JoinLatestIn::Left(l) => state.left = Some(l),
            JoinLatestIn::Right(r) => state.right = Some(r),
        }
        if let (Some(l), Some(r)) = (state.left.clone(), state.right.clone()) {
            emit.emit((l, r));
        }
    }
}

impl<L: Clone, R: Clone> FlushableScan for JoinLatestScan<L, R> {
    type Offset = u64;

    fn flush<E>(&self, _state: &mut Self::State, _signal: FlushReason<Self::Offset>, _emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
    }
}

impl<
        L: Clone + Serialize + for<'de> Deserialize<'de>,
        R: Clone + Serialize + for<'de> Deserialize<'de>,
    > SnapshottingScan for JoinLatestScan<L, R>
{
    type Snapshot = JoinLatestSnapshot<L, R>;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        JoinLatestSnapshot {
            left: state.left.clone(),
            right: state.right.clone(),
        }
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        JoinLatestState {
            left: snapshot.left,
            right: snapshot.right,
        }
    }
}

impl<L: Serialize + for<'de> Deserialize<'de>, R: Serialize + for<'de> Deserialize<'de>>
    VersionedSnapshot for JoinLatestSnapshot<L, R>
{
    const VERSION: u32 = 1;
}
