use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::Hash;
use std::marker::PhantomData;

use serde::{de::DeserializeOwned, Serialize};

use crate::control::{Checkpoint, CheckpointMeta, FlushReason};
use crate::emit::Emit;
use crate::scan::{FallibleRestoreScan, FlushableScan, Scan, SnapshottingScan, VersionedSnapshot};

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

/// Legacy in-band checkpoint wrapper.
///
/// Storage errors cannot be emitted through [`FlushableScan`], so they are captured and exposed by
/// [`Persisted::take_checkpoint_error`]. Production runners should prefer [`write_checkpoint`] and
/// handle its `Result` before committing their source cursor or output transaction.
pub struct Persisted<S, St, KF, O> {
    pub inner: S,
    pub store: RefCell<St>,
    pub key_fn: KF,
    checkpoint_error: RefCell<Option<String>>,
    _offset: PhantomData<O>,
}

impl<S, St, KF, O> Persisted<S, St, KF, O> {
    pub fn new(inner: S, store: St, key_fn: KF) -> Self {
        Self {
            inner,
            store: RefCell::new(store),
            key_fn,
            checkpoint_error: RefCell::new(None),
            _offset: PhantomData,
        }
    }

    /// Take the most recent in-band checkpoint storage error, if any.
    pub fn take_checkpoint_error(&self) -> Option<String> {
        self.checkpoint_error.borrow_mut().take()
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
            if let Err(error) = st.put(key, cp) {
                *self.checkpoint_error.borrow_mut() = Some(format!("{error:?}"));
            }
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

/// Compatibility requirements checked before a checkpoint is accepted for restore.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckpointRequirements<'a> {
    pub format_version: u32,
    pub snapshot_version: Option<u32>,
    pub pipeline_fingerprint: Option<&'a str>,
}

impl Default for CheckpointRequirements<'_> {
    fn default() -> Self {
        Self {
            format_version: 1,
            snapshot_version: None,
            pipeline_fingerprint: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckpointReadError<E> {
    Store(E),
    FormatVersion {
        expected: u32,
        found: u32,
    },
    SnapshotVersion {
        expected: u32,
        found: Option<u32>,
    },
    PipelineFingerprint {
        expected: String,
        found: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckpointRestoreError<StoreError, RestoreError> {
    Read(CheckpointReadError<StoreError>),
    Snapshot(RestoreError),
}

/// Validated runtime state plus the source position that must resume with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoredCheckpoint<State, O> {
    pub state: State,
    pub offset: O,
    pub watermark: Option<O>,
    pub metadata: CheckpointMeta,
}

pub type CheckpointResumeResult<State, O, StoreError, RestoreError> =
    Result<Option<RestoredCheckpoint<State, O>>, CheckpointRestoreError<StoreError, RestoreError>>;

/// Source position and operator identity attached to a checkpoint write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckpointContext<O> {
    pub offset: O,
    pub watermark: Option<O>,
    pub pipeline_fingerprint: Option<String>,
    pub label: Option<String>,
}

/// Persist one typed checkpoint and return storage errors to the caller.
///
/// A successful call only proves that the checkpoint store accepted one value. Exactly-once
/// processing additionally requires the source cursor and externally visible outputs to share an
/// atomic commit protocol or idempotency key.
pub fn write_checkpoint<S, St, K, O>(
    scan: &S,
    state: &S::State,
    store: &mut St,
    key: K,
    context: CheckpointContext<O>,
) -> Result<(), St::Error>
where
    S: SnapshottingScan,
    S::Snapshot: VersionedSnapshot,
    St: SnapshotStore<K, Checkpoint<S::Snapshot, O>>,
{
    let checkpoint = Checkpoint {
        snapshot: scan.snapshot(state),
        offset: context.offset,
        watermark: context.watermark,
        metadata: CheckpointMeta {
            format_version: 1,
            snapshot_version: Some(S::Snapshot::VERSION),
            pipeline_fingerprint: context.pipeline_fingerprint,
            label: context.label,
        },
    };
    store.put(key, checkpoint)
}

/// Read and validate checkpoint compatibility before returning it to a restore path.
pub fn read_checkpoint<St, K, S, O>(
    store: &mut St,
    key: &K,
    requirements: CheckpointRequirements<'_>,
) -> Result<Option<Checkpoint<S, O>>, CheckpointReadError<St::Error>>
where
    St: SnapshotStore<K, Checkpoint<S, O>>,
{
    let Some(checkpoint) = store.get(key).map_err(CheckpointReadError::Store)? else {
        return Ok(None);
    };
    if checkpoint.metadata.format_version != requirements.format_version {
        return Err(CheckpointReadError::FormatVersion {
            expected: requirements.format_version,
            found: checkpoint.metadata.format_version,
        });
    }
    if let Some(expected) = requirements.snapshot_version {
        if checkpoint.metadata.snapshot_version != Some(expected) {
            return Err(CheckpointReadError::SnapshotVersion {
                expected,
                found: checkpoint.metadata.snapshot_version,
            });
        }
    }
    if let Some(expected) = requirements.pipeline_fingerprint {
        if checkpoint.metadata.pipeline_fingerprint.as_deref() != Some(expected) {
            return Err(CheckpointReadError::PipelineFingerprint {
                expected: expected.to_owned(),
                found: checkpoint.metadata.pipeline_fingerprint.clone(),
            });
        }
    }
    Ok(Some(checkpoint))
}

/// Read metadata, validate snapshot contents, and produce state for a resume path.
pub fn read_and_restore_checkpoint<S, St, K, O>(
    scan: &S,
    store: &mut St,
    key: &K,
    requirements: CheckpointRequirements<'_>,
) -> CheckpointResumeResult<S::State, O, St::Error, S::RestoreError>
where
    S: FallibleRestoreScan,
    St: SnapshotStore<K, Checkpoint<S::Snapshot, O>>,
{
    let Some(checkpoint) =
        read_checkpoint(store, key, requirements).map_err(CheckpointRestoreError::Read)?
    else {
        return Ok(None);
    };
    let state = scan
        .try_restore(checkpoint.snapshot)
        .map_err(CheckpointRestoreError::Snapshot)?;
    Ok(Some(RestoredCheckpoint {
        state,
        offset: checkpoint.offset,
        watermark: checkpoint.watermark,
        metadata: checkpoint.metadata,
    }))
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    use super::*;
    use crate::{FlushReason, FlushableScan, Scan, VecEmitter};

    #[derive(Debug, Clone, Copy)]
    struct Counter;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    struct CounterSnapshot(u64);

    impl VersionedSnapshot for CounterSnapshot {
        const VERSION: u32 = 3;
    }

    impl Scan for Counter {
        type In = u64;
        type Out = u64;
        type State = u64;

        fn init(&self) -> Self::State {
            0
        }

        fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
        where
            E: Emit<Self::Out>,
        {
            *state += input;
            emit.emit(*state);
        }
    }

    impl FlushableScan for Counter {
        type Offset = u64;

        fn flush<E>(
            &self,
            _state: &mut Self::State,
            _signal: FlushReason<Self::Offset>,
            _emit: &mut E,
        ) where
            E: Emit<Self::Out>,
        {
        }
    }

    impl SnapshottingScan for Counter {
        type Snapshot = CounterSnapshot;

        fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
            CounterSnapshot(*state)
        }

        fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
            snapshot.0
        }
    }

    impl FallibleRestoreScan for Counter {
        type RestoreError = &'static str;

        fn try_restore(&self, snapshot: Self::Snapshot) -> Result<Self::State, Self::RestoreError> {
            (snapshot.0 <= 100)
                .then_some(snapshot.0)
                .ok_or("counter snapshot exceeds configured maximum")
        }
    }

    #[test]
    fn fallible_checkpoint_round_trip_validates_compatibility() {
        type Stored = Checkpoint<CounterSnapshot, u64>;
        let mut store = HashMapStore::<&'static str, Stored>::default();
        write_checkpoint(
            &Counter,
            &42,
            &mut store,
            "main",
            CheckpointContext {
                offset: 99,
                watermark: Some(90),
                pipeline_fingerprint: Some("pipeline-abc".into()),
                label: Some("test".into()),
            },
        )
        .unwrap();

        let checkpoint = read_checkpoint(
            &mut store,
            &"main",
            CheckpointRequirements {
                format_version: 1,
                snapshot_version: Some(3),
                pipeline_fingerprint: Some("pipeline-abc"),
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(checkpoint.snapshot, CounterSnapshot(42));
        assert_eq!(checkpoint.offset, 99);
        assert_eq!(checkpoint.watermark, Some(90));

        let restored = read_and_restore_checkpoint(
            &Counter,
            &mut store,
            &"main",
            CheckpointRequirements {
                format_version: 1,
                snapshot_version: Some(3),
                pipeline_fingerprint: Some("pipeline-abc"),
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(restored.state, 42);
        assert_eq!(restored.offset, 99);

        let mismatch = read_checkpoint(
            &mut store,
            &"main",
            CheckpointRequirements {
                pipeline_fingerprint: Some("different"),
                ..CheckpointRequirements::default()
            },
        );
        assert!(matches!(
            mismatch,
            Err(CheckpointReadError::PipelineFingerprint { .. })
        ));

        store.inner.insert(
            "corrupt",
            Checkpoint {
                snapshot: CounterSnapshot(101),
                offset: 100,
                watermark: None,
                metadata: CheckpointMeta {
                    format_version: 1,
                    snapshot_version: Some(3),
                    pipeline_fingerprint: Some("pipeline-abc".into()),
                    label: None,
                },
            },
        );
        let corrupt = read_and_restore_checkpoint(
            &Counter,
            &mut store,
            &"corrupt",
            CheckpointRequirements {
                format_version: 1,
                snapshot_version: Some(3),
                pipeline_fingerprint: Some("pipeline-abc"),
            },
        );
        assert_eq!(
            corrupt,
            Err(CheckpointRestoreError::Snapshot(
                "counter snapshot exceeds configured maximum"
            ))
        );
    }

    #[derive(Debug, Default)]
    struct FailingStore;

    impl SnapshotStore<&'static str, Checkpoint<CounterSnapshot, u64>> for FailingStore {
        type Error = &'static str;

        fn put(
            &mut self,
            _key: &'static str,
            _value: Checkpoint<CounterSnapshot, u64>,
        ) -> Result<(), Self::Error> {
            Err("disk full")
        }

        fn get(
            &mut self,
            _key: &&'static str,
        ) -> Result<Option<Checkpoint<CounterSnapshot, u64>>, Self::Error> {
            Ok(None)
        }
    }

    #[derive(Clone)]
    struct FixedKey;

    impl CheckpointKeyFn<u64> for FixedKey {
        type Key = &'static str;

        fn key_for_offset(&self, _offset: &u64) -> Self::Key {
            "main"
        }
    }

    #[test]
    fn legacy_persisted_captures_storage_error_without_panicking() {
        let persisted = Persisted::new(Counter, FailingStore, FixedKey);
        let mut state = persisted.init();
        let mut emit = VecEmitter::new();
        persisted.step(&mut state, 7, &mut emit);
        persisted.flush(&mut state, FlushReason::Checkpoint(1), &mut emit);
        assert_eq!(
            persisted.take_checkpoint_error().as_deref(),
            Some("\"disk full\"")
        );
        assert_eq!(state, 7);
    }
}
