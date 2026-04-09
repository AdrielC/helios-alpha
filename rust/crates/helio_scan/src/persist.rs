use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::Hash;
use std::marker::PhantomData;

use serde::{de::DeserializeOwned, Serialize};

use crate::control::{Checkpoint, CheckpointMeta, FlushReason};
use crate::emit::Emit;
use crate::scan::{FlushableScan, Scan, SnapshottingScan};

pub trait SnapshotStore<K, V> {
    type Error: std::fmt::Debug;

    fn put(&mut self, key: K, value: V) -> Result<(), Self::Error>;
    fn get(&mut self, key: &K) -> Result<Option<V>, Self::Error>;
}

/// In-memory store for tests and prototyping.
#[derive(Debug)]
pub struct HashMapStore<K, V> {
    pub inner: HashMap<K, V>,
}

impl<K, V> Default for HashMapStore<K, V> {
    fn default() -> Self {
        Self {
            inner: HashMap::new(),
        }
    }
}

impl<K: Eq + Hash + Clone, V: Clone> SnapshotStore<K, V> for HashMapStore<K, V> {
    type Error = std::convert::Infallible;

    fn put(&mut self, key: K, value: V) -> Result<(), Self::Error> {
        self.inner.insert(key, value);
        Ok(())
    }

    fn get(&mut self, key: &K) -> Result<Option<V>, Self::Error> {
        Ok(self.inner.get(key).cloned())
    }
}

/// Wraps a scan: after handling flush, persists on [`FlushReason::Checkpoint`].
pub struct Persisted<S, St, KF, O> {
    pub inner: S,
    pub store: RefCell<St>,
    pub key_fn: KF,
    _offset: PhantomData<O>,
}

impl<S, St, KF, O> Persisted<S, St, KF, O> {
    pub fn new(inner: S, store: St, key_fn: KF) -> Self {
        Self {
            inner,
            store: RefCell::new(store),
            key_fn,
            _offset: PhantomData,
        }
    }
}

impl<S, St, KF, O> Scan for Persisted<S, St, KF, O>
where
    S: Scan,
    O: Clone,
{
    type In = S::In;
    type Out = S::Out;
    type State = S::State;

    fn init(&self) -> Self::State {
        self.inner.init()
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        self.inner.step(state, input, emit);
    }
}

impl<S, St, KF, O> FlushableScan for Persisted<S, St, KF, O>
where
    S: FlushableScan<Offset = O> + SnapshottingScan,
    St: SnapshotStore<KF::Key, Checkpoint<S::Snapshot, O>>,
    KF: CheckpointKeyFn<O>,
    O: Clone + Serialize,
    S::Snapshot: Serialize,
    Checkpoint<S::Snapshot, O>: Clone,
{
    type Offset = O;

    fn flush<E>(&self, state: &mut Self::State, signal: FlushReason<O>, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        self.inner.flush(state, signal.clone(), emit);

        if let FlushReason::Checkpoint(ref offset) = signal {
            let snap = self.inner.snapshot(state);
            let key = self.key_fn.key_for_offset(offset);
            let cp = Checkpoint {
                snapshot: snap,
                offset: offset.clone(),
                watermark: None,
                metadata: CheckpointMeta::default(),
            };
            let mut st = self.store.borrow_mut();
            st.put(key, cp).expect("snapshot store");
        }
    }
}

impl<S, St, KF, O> SnapshottingScan for Persisted<S, St, KF, O>
where
    S: SnapshottingScan,
    S::Snapshot: Serialize + DeserializeOwned,
    O: Clone,
{
    type Snapshot = S::Snapshot;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        self.inner.snapshot(state)
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        self.inner.restore(snapshot)
    }
}

/// Keys for persisted checkpoints (avoid storing `O` as key when it is not `Hash`).
pub trait CheckpointKeyFn<O> {
    type Key: Clone;

    fn key_for_offset(&self, offset: &O) -> Self::Key;
}
