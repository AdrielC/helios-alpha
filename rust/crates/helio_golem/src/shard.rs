use helio_hypothesis::{
    HypothesisEngine, HypothesisEvent, HypothesisInput, HypothesisModel, HypothesisRejection,
    HypothesisRestoreError, HypothesisSnapshot, HypothesisState, KeyedHypothesisMachine,
};
use helio_scan::VersionedSnapshot;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use thiserror::Error;

/// Stable identity of one durable source-partition owner.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ShardIdentity {
    pub strategy_fingerprint: String,
    pub source_id: String,
    pub source_partition: u64,
    pub logical_shard: u64,
}

impl ShardIdentity {
    pub fn try_new(
        strategy_fingerprint: String,
        source_id: String,
        source_partition: u64,
        logical_shard: u64,
    ) -> Result<Self, ShardConfigError> {
        if strategy_fingerprint.trim().is_empty() {
            return Err(ShardConfigError::EmptyStrategyFingerprint);
        }
        if source_id.trim().is_empty() {
            return Err(ShardConfigError::EmptySourceId);
        }
        Ok(Self {
            strategy_fingerprint,
            source_id,
            source_partition,
            logical_shard,
        })
    }

    /// Deterministic Golem invocation key for a source batch.
    pub fn invocation_key(&self, first_offset: u64, last_offset: u64) -> String {
        format!(
            "v1/{}:{}/{}:{}/{}/{}/{}-{}",
            self.strategy_fingerprint.len(),
            self.strategy_fingerprint,
            self.source_id.len(),
            self.source_id,
            self.source_partition,
            self.logical_shard,
            first_offset,
            last_offset
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ShardConfigError {
    #[error("strategy fingerprint must not be empty")]
    EmptyStrategyFingerprint,
    #[error("source ID must not be empty")]
    EmptySourceId,
    #[error("durable shard batch capacity must be positive")]
    ZeroBatchCapacity,
}

/// One source position and the exact hypothesis input stored there.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OffsetInput<I> {
    pub offset: u64,
    pub input: I,
}

impl<I> OffsetInput<I> {
    pub const fn new(offset: u64, input: I) -> Self {
        Self { offset, input }
    }
}

/// Accepted batch result. External effects derive their identities from this source interval and
/// the revisions in `events`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShardReceipt<Event> {
    pub first_offset: u64,
    pub last_offset: u64,
    pub next_offset: u64,
    pub events: Vec<Event>,
}

type ShardInput<K, Model, Reason> =
    HypothesisInput<K, <Model as HypothesisModel<K>>::Evidence, Reason>;

type ShardEvent<K, Model, Reason> = HypothesisEvent<
    K,
    <Model as HypothesisModel<K>>::Output,
    Reason,
    <Model as HypothesisModel<K>>::Error,
>;

pub type DurableShardReceipt<K, Model, Reason> = ShardReceipt<ShardEvent<K, Model, Reason>>;

#[derive(Debug, PartialEq, Error)]
pub enum ShardProcessError<ModelError> {
    #[error("source batch must contain at least one input")]
    EmptyBatch,
    #[error("source batch has {found} inputs; capacity is {capacity}")]
    BatchCapacityExceeded { found: usize, capacity: u32 },
    #[error("source offset overlaps committed state: expected {expected}, found {found}")]
    OffsetOverlap { expected: u64, found: u64 },
    #[error("source offset has a gap: expected {expected}, found {found}")]
    OffsetGap { expected: u64, found: u64 },
    #[error("source offset space is exhausted at {offset}")]
    OffsetExhausted { offset: u64 },
    #[error("hypothesis transition at source offset {offset} was rejected: {rejection:?}")]
    TransitionRejected {
        offset: u64,
        rejection: HypothesisRejection<ModelError>,
    },
}

/// Versioned durable state used by Golem custom snapshot hooks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "K: Ord + Serialize, State: Serialize, Reason: Serialize",
    deserialize = "K: Ord + Deserialize<'de>, State: Deserialize<'de>, Reason: Deserialize<'de>"
))]
pub struct DurableShardSnapshot<K, State, Reason> {
    pub identity: ShardIdentity,
    pub max_batch_size: u32,
    pub next_offset: u64,
    pub engine: HypothesisSnapshot<K, State, Reason>,
}

impl<K, State, Reason> VersionedSnapshot for DurableShardSnapshot<K, State, Reason> {
    const VERSION: u32 = 1;
}

#[derive(Debug, PartialEq, Error)]
pub enum ShardRestoreError<K, ModelError> {
    #[error("snapshot identity does not match the requested durable shard")]
    IdentityMismatch,
    #[error("snapshot batch capacity {found} does not match configured capacity {expected}")]
    BatchCapacityMismatch { expected: u32, found: u32 },
    #[error("hypothesis snapshot is invalid: {0}")]
    Hypothesis(#[from] HypothesisRestoreError<K, ModelError>),
}

/// Single-owner durable shard core. One Golem agent should own exactly one value of this type.
#[derive(Clone)]
pub struct DurableHypothesisShard<K, Model, Reason>
where
    Model: HypothesisModel<K>,
{
    identity: ShardIdentity,
    max_batch_size: u32,
    next_offset: u64,
    engine: HypothesisEngine<K, Model, Reason>,
}

impl<K, Model, Reason> DurableHypothesisShard<K, Model, Reason>
where
    K: Clone + Ord,
    Model: Clone + HypothesisModel<K>,
    Model::State: Clone,
    Reason: Clone,
{
    pub fn try_new(
        machine: KeyedHypothesisMachine<K, Model, Reason>,
        identity: ShardIdentity,
        max_batch_size: u32,
        next_offset: u64,
    ) -> Result<Self, ShardConfigError> {
        if max_batch_size == 0 {
            return Err(ShardConfigError::ZeroBatchCapacity);
        }
        Ok(Self {
            identity,
            max_batch_size,
            next_offset,
            engine: HypothesisEngine::new(machine),
        })
    }

    pub const fn identity(&self) -> &ShardIdentity {
        &self.identity
    }

    pub const fn max_batch_size(&self) -> u32 {
        self.max_batch_size
    }

    pub const fn next_offset(&self) -> u64 {
        self.next_offset
    }

    pub const fn state(&self) -> &HypothesisState<K, Model::State, Reason> {
        self.engine.state()
    }

    /// Apply one contiguous source batch atomically.
    ///
    /// The model must remain pure. The method clones the bounded engine once, executes every
    /// transition on the trial value, and swaps it into live state only after the full batch is
    /// accepted. A gap, overlap, or model rejection leaves both state and source progress intact.
    pub fn process_batch(
        &mut self,
        inputs: Vec<OffsetInput<ShardInput<K, Model, Reason>>>,
    ) -> Result<DurableShardReceipt<K, Model, Reason>, ShardProcessError<Model::Error>> {
        let Some(first) = inputs.first() else {
            return Err(ShardProcessError::EmptyBatch);
        };
        if inputs.len() > self.max_batch_size as usize {
            return Err(ShardProcessError::BatchCapacityExceeded {
                found: inputs.len(),
                capacity: self.max_batch_size,
            });
        }

        let first_offset = first.offset;
        let mut expected = self.next_offset;
        for item in &inputs {
            if item.offset < expected {
                return Err(ShardProcessError::OffsetOverlap {
                    expected,
                    found: item.offset,
                });
            }
            if item.offset > expected {
                return Err(ShardProcessError::OffsetGap {
                    expected,
                    found: item.offset,
                });
            }
            expected = expected
                .checked_add(1)
                .ok_or(ShardProcessError::OffsetExhausted {
                    offset: item.offset,
                })?;
        }

        let last_offset = inputs
            .last()
            .map(|input| input.offset)
            .ok_or(ShardProcessError::EmptyBatch)?;
        let mut trial = self.engine.clone();
        let mut accepted_events = Vec::new();
        for item in inputs {
            let events = trial.process(item.input);
            for event in events {
                match event {
                    HypothesisEvent::Rejected { error, .. } => {
                        return Err(ShardProcessError::TransitionRejected {
                            offset: item.offset,
                            rejection: error,
                        });
                    }
                    accepted => accepted_events.push(accepted),
                }
            }
        }

        self.engine = trial;
        self.next_offset = expected;
        Ok(ShardReceipt {
            first_offset,
            last_offset,
            next_offset: expected,
            events: accepted_events,
        })
    }
}

impl<K, Model, Reason> DurableHypothesisShard<K, Model, Reason>
where
    K: Clone + Ord + Serialize + DeserializeOwned,
    Model: Clone + HypothesisModel<K>,
    Model::State: Clone + Serialize + DeserializeOwned,
    Reason: Clone + Serialize + DeserializeOwned,
{
    pub fn snapshot(&self) -> DurableShardSnapshot<K, Model::State, Reason> {
        DurableShardSnapshot {
            identity: self.identity.clone(),
            max_batch_size: self.max_batch_size,
            next_offset: self.next_offset,
            engine: self.engine.snapshot(),
        }
    }

    pub fn try_from_snapshot(
        machine: KeyedHypothesisMachine<K, Model, Reason>,
        expected_identity: ShardIdentity,
        expected_max_batch_size: u32,
        snapshot: DurableShardSnapshot<K, Model::State, Reason>,
    ) -> Result<Self, ShardRestoreError<K, Model::Error>> {
        if snapshot.identity != expected_identity {
            return Err(ShardRestoreError::IdentityMismatch);
        }
        if snapshot.max_batch_size != expected_max_batch_size {
            return Err(ShardRestoreError::BatchCapacityMismatch {
                expected: expected_max_batch_size,
                found: snapshot.max_batch_size,
            });
        }
        let engine = HypothesisEngine::try_from_snapshot(machine, snapshot.engine)?;
        Ok(Self {
            identity: expected_identity,
            max_batch_size: expected_max_batch_size,
            next_offset: snapshot.next_offset,
            engine,
        })
    }
}

#[cfg(test)]
mod tests {
    use helio_hypothesis::{CausalEvidence, HypothesisConfig, HypothesisTransition, TimerId};
    use helio_time::{AvailableAt, EffectiveAt};

    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    struct State(u64);

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum ModelError {
        Rejected,
    }

    #[derive(Debug, Clone, Copy)]
    struct Model;

    impl HypothesisModel<String> for Model {
        type Evidence = Result<u64, ()>;
        type State = State;
        type Output = u64;
        type Error = ModelError;

        fn open(
            &self,
            _key: &String,
            evidence: CausalEvidence<Self::Evidence>,
        ) -> Result<HypothesisTransition<Self::State, Self::Output>, Self::Error> {
            let value = evidence.payload.map_err(|()| ModelError::Rejected)?;
            Ok(HypothesisTransition::new(State(value)).emit(value))
        }

        fn update(
            &self,
            _key: &String,
            state: &Self::State,
            evidence: CausalEvidence<Self::Evidence>,
        ) -> Result<HypothesisTransition<Self::State, Self::Output>, Self::Error> {
            let value = evidence.payload.map_err(|()| ModelError::Rejected)?;
            let next = State(state.0 + value);
            Ok(HypothesisTransition::new(next).emit(next.0))
        }

        fn on_timer(
            &self,
            _key: &String,
            state: &Self::State,
            _timer_id: TimerId,
            _at: AvailableAt,
        ) -> Result<HypothesisTransition<Self::State, Self::Output>, Self::Error> {
            Ok(HypothesisTransition::new(*state))
        }

        fn validate(&self, _key: &String, _state: &Self::State) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    type Shard = DurableHypothesisShard<String, Model, String>;

    fn identity() -> ShardIdentity {
        ShardIdentity::try_new("strategy-v1".into(), "events".into(), 2, 7).unwrap()
    }

    fn shard(max_batch_size: u32) -> Shard {
        let machine = KeyedHypothesisMachine::try_new(
            Model,
            HypothesisConfig::try_new(8, 8, 2, 4, 8).unwrap(),
        )
        .unwrap();
        DurableHypothesisShard::try_new(machine, identity(), max_batch_size, 10).unwrap()
    }

    fn evidence(sequence: u64, at: i64, value: Result<u64, ()>) -> CausalEvidence<Result<u64, ()>> {
        CausalEvidence::new(sequence, EffectiveAt(at), AvailableAt(at), value)
    }

    fn open(offset: u64) -> OffsetInput<HypothesisInput<String, Result<u64, ()>, String>> {
        OffsetInput::new(
            offset,
            HypothesisInput::Open {
                key: "incident".into(),
                evidence: evidence(0, 10, Ok(3)),
            },
        )
    }

    fn update(
        offset: u64,
        sequence: u64,
        value: Result<u64, ()>,
    ) -> OffsetInput<HypothesisInput<String, Result<u64, ()>, String>> {
        OffsetInput::new(
            offset,
            HypothesisInput::Evidence {
                key: "incident".into(),
                evidence: evidence(sequence, offset as i64 + 1, value),
            },
        )
    }

    #[test]
    fn contiguous_batch_commits_state_and_source_position() {
        let mut shard = shard(8);
        let receipt = shard
            .process_batch(vec![open(10), update(11, 1, Ok(4))])
            .unwrap();

        assert_eq!(receipt.first_offset, 10);
        assert_eq!(receipt.last_offset, 11);
        assert_eq!(receipt.next_offset, 12);
        assert_eq!(shard.next_offset(), 12);
        assert_eq!(
            shard
                .state()
                .get(&"incident".to_string())
                .unwrap()
                .model_state,
            State(7)
        );
    }

    #[test]
    fn gap_overlap_and_capacity_fail_without_mutation() {
        let mut shard = shard(1);
        let before = shard.snapshot();
        assert!(matches!(
            shard.process_batch(vec![open(11)]),
            Err(ShardProcessError::OffsetGap {
                expected: 10,
                found: 11
            })
        ));
        assert!(matches!(
            shard.process_batch(vec![open(10), update(11, 1, Ok(1))]),
            Err(ShardProcessError::BatchCapacityExceeded { .. })
        ));
        assert_eq!(shard.snapshot(), before);

        shard.process_batch(vec![open(10)]).unwrap();
        assert!(matches!(
            shard.process_batch(vec![update(10, 1, Ok(1))]),
            Err(ShardProcessError::OffsetOverlap {
                expected: 11,
                found: 10
            })
        ));
    }

    #[test]
    fn rejected_late_item_rolls_back_the_entire_batch() {
        let mut shard = shard(8);
        shard.process_batch(vec![open(10)]).unwrap();
        let before = shard.snapshot();

        assert!(matches!(
            shard.process_batch(vec![update(11, 1, Ok(4)), update(12, 2, Err(()))]),
            Err(ShardProcessError::TransitionRejected {
                offset: 12,
                rejection: HypothesisRejection::Model(ModelError::Rejected)
            })
        ));
        assert_eq!(shard.snapshot(), before);
    }

    #[test]
    fn snapshot_restore_preserves_future_receipts_and_duplicate_guard() {
        let mut uninterrupted = shard(8);
        uninterrupted.process_batch(vec![open(10)]).unwrap();
        let json = serde_json::to_vec(&uninterrupted.snapshot()).unwrap();
        let snapshot = serde_json::from_slice(&json).unwrap();
        let machine = KeyedHypothesisMachine::try_new(
            Model,
            HypothesisConfig::try_new(8, 8, 2, 4, 8).unwrap(),
        )
        .unwrap();
        let mut restored =
            DurableHypothesisShard::try_from_snapshot(machine, identity(), 8, snapshot).unwrap();

        let expected = uninterrupted
            .process_batch(vec![update(11, 1, Ok(5))])
            .unwrap();
        let actual = restored.process_batch(vec![update(11, 1, Ok(5))]).unwrap();
        assert_eq!(actual, expected);
        assert!(matches!(
            restored.process_batch(vec![update(11, 1, Ok(5))]),
            Err(ShardProcessError::OffsetOverlap {
                expected: 12,
                found: 11
            })
        ));
    }

    #[test]
    fn invocation_identity_is_stable_and_auditable() {
        assert_eq!(
            identity().invocation_key(10, 12),
            "v1/11:strategy-v1/6:events/2/7/10-12"
        );

        let ambiguous_without_lengths =
            ShardIdentity::try_new("a:b".into(), "c".into(), 2, 7).unwrap();
        let other = ShardIdentity::try_new("a".into(), "b:c".into(), 2, 7).unwrap();
        assert_ne!(
            ambiguous_without_lengths.invocation_key(10, 12),
            other.invocation_key(10, 12)
        );
    }
}
