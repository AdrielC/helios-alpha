use std::collections::HashSet;
use std::hash::Hash;

use helio_scan::{Emit, FlushReason, FlushableScan, Scan, SnapshottingScan, VersionedSnapshot};
use serde::{Deserialize, Serialize};

/// Keyed observation: first time a key appears, emit `(key, value)`; later duplicates dropped.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DedupSample<K, V> {
    pub key: K,
    pub value: V,
}

#[derive(Debug, Clone)]
pub struct DedupScan<K, V> {
    _p: std::marker::PhantomData<(K, V)>,
}

impl<K, V> Default for DedupScan<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K, V> DedupScan<K, V> {
    pub fn new() -> Self {
        Self {
            _p: std::marker::PhantomData,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DedupState<K>
where
    K: Eq + Hash,
{
    #[serde(bound(
        serialize = "K: Serialize",
        deserialize = "K: Deserialize<'de> + Eq + Hash"
    ))]
    pub seen: HashSet<K>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DedupSnapshot<K>
where
    K: Eq + Hash,
{
    #[serde(bound(
        serialize = "K: Serialize",
        deserialize = "K: Deserialize<'de> + Eq + Hash"
    ))]
    pub seen: Vec<K>,
}

impl<K: Clone + Eq + Hash, V: Clone> Scan for DedupScan<K, V> {
    type In = DedupSample<K, V>;
    type Out = DedupSample<K, V>;
    type State = DedupState<K>;

    fn init(&self) -> Self::State {
        DedupState {
            seen: HashSet::new(),
        }
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        if state.seen.insert(input.key.clone()) {
            emit.emit(input);
        }
    }
}

impl<K: Clone + Eq + Hash, V: Clone> FlushableScan for DedupScan<K, V> {
    type Offset = u64;

    fn flush<E>(&self, _state: &mut Self::State, _signal: FlushReason<Self::Offset>, _emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
    }
}

impl<K: Clone + Eq + Hash + Serialize + for<'de> Deserialize<'de>, V: Clone> SnapshottingScan
    for DedupScan<K, V>
{
    type Snapshot = DedupSnapshot<K>;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        DedupSnapshot {
            seen: state.seen.iter().cloned().collect(),
        }
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        DedupState {
            seen: snapshot.seen.into_iter().collect(),
        }
    }
}

impl<K: Eq + Hash + Serialize + for<'de> Deserialize<'de>> VersionedSnapshot for DedupSnapshot<K> {
    const VERSION: u32 = 1;
}
