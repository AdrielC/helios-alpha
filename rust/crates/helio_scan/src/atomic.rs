//! Atomic source progress, checkpoint, and transactional outbox coordination.
//!
//! A database adapter can implement the same contract with one serializable transaction. The
//! in-memory implementation is executable reference semantics and a fault-injection harness.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::Checkpoint;

/// Stable identity understood by both the outbox and the external sink.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct OutputId(String);

impl OutputId {
    pub fn try_new(value: impl Into<String>) -> Result<Self, AtomicCommitError> {
        let value = value.into();
        if value.trim().is_empty() {
            Err(AtomicCommitError::EmptyOutputId)
        } else {
            Ok(Self(value))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One externally visible effect derived from an input inside a source transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransactionalOutput<T> {
    pub id: OutputId,
    pub source_offset: u64,
    pub payload: T,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutboxStatus<A> {
    Pending,
    Delivered { acknowledgement: A },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboxEntry<T, A> {
    pub output: TransactionalOutput<T>,
    pub delivery_attempts: u32,
    pub status: OutboxStatus<A>,
}

/// One transaction covering a contiguous source prefix, its resume checkpoint, and every output.
///
/// `checkpoint.offset` is the next source offset to read, not the last consumed offset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AtomicCommitBundle<S, T> {
    pub transaction_id: String,
    pub expected_next_offset: u64,
    pub next_offset: u64,
    pub checkpoint: Checkpoint<S, u64>,
    pub outputs: Vec<TransactionalOutput<T>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AtomicCommitReceipt {
    pub transaction_id: String,
    pub next_offset: u64,
    pub output_count: usize,
    pub replayed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitFault {
    None,
    BeforeCommit,
    /// The transaction commits, but the caller observes an ambiguous infrastructure failure.
    AfterCommit,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AtomicCommitError {
    #[error("transaction identity must not be empty")]
    EmptyTransactionId,
    #[error("output identity must not be empty")]
    EmptyOutputId,
    #[error("source transaction must advance beyond offset {offset}")]
    EmptySourceRange { offset: u64 },
    #[error("source cursor conflict: committed {committed}, transaction expected {expected}")]
    SourceConflict { committed: u64, expected: u64 },
    #[error("checkpoint offset {checkpoint} does not equal next source offset {next}")]
    CheckpointOffsetMismatch { checkpoint: u64, next: u64 },
    #[error("output {id} cites source offset {offset} outside [{first}, {next})")]
    OutputOutsideTransaction {
        id: String,
        offset: u64,
        first: u64,
        next: u64,
    },
    #[error("transaction contains duplicate output identity {0}")]
    DuplicateOutputId(String),
    #[error("output identity {0} was already committed by another transaction")]
    OutputIdentityConflict(String),
    #[error("transaction identity was replayed with different contents")]
    TransactionIdentityConflict,
    #[error("fault injected before transaction commit")]
    FailedBeforeCommit,
    #[error("commit outcome is unknown; retry the same transaction identity")]
    CommitOutcomeUnknown,
    #[error("outbox acknowledgement references unknown output {0}")]
    UnknownOutput(String),
    #[error("outbox delivery-attempt counter overflowed for {0}")]
    DeliveryAttemptOverflow(String),
    #[error("restored atomic commit state is corrupt: {0}")]
    RestoredStateCorrupt(String),
}

/// Reference state that a production adapter stores in one serializable database transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AtomicCommitState<S, T, A> {
    pub next_offset: u64,
    pub checkpoint: Option<Checkpoint<S, u64>>,
    pub outbox: BTreeMap<OutputId, OutboxEntry<T, A>>,
    /// Durable identity ledger required to resolve a lost commit acknowledgement after restart.
    pub transactions: BTreeMap<String, AtomicCommitBundle<S, T>>,
}

/// In-memory reference store with explicit before-commit and after-commit fault injection.
#[derive(Debug, Clone)]
pub struct InMemoryAtomicCommitStore<S, T, A> {
    state: AtomicCommitState<S, T, A>,
}

impl<S, T, A> InMemoryAtomicCommitStore<S, T, A> {
    pub fn new(initial_next_offset: u64) -> Self {
        Self {
            state: AtomicCommitState {
                next_offset: initial_next_offset,
                checkpoint: None,
                outbox: BTreeMap::new(),
                transactions: BTreeMap::new(),
            },
        }
    }

    pub const fn state(&self) -> &AtomicCommitState<S, T, A> {
        &self.state
    }

    pub fn pending_outputs(&self) -> impl Iterator<Item = &OutboxEntry<T, A>> {
        self.state
            .outbox
            .values()
            .filter(|entry| matches!(entry.status, OutboxStatus::Pending))
    }
}

impl<S, T, A> InMemoryAtomicCommitStore<S, T, A>
where
    S: Clone + PartialEq,
    T: Clone + PartialEq,
    A: Clone,
{
    pub fn restore(state: AtomicCommitState<S, T, A>) -> Result<Self, AtomicCommitError> {
        validate_restored_state(&state)?;
        Ok(Self { state })
    }

    pub fn commit(
        &mut self,
        bundle: AtomicCommitBundle<S, T>,
    ) -> Result<AtomicCommitReceipt, AtomicCommitError> {
        self.commit_with_fault(bundle, CommitFault::None)
    }

    pub fn commit_with_fault(
        &mut self,
        bundle: AtomicCommitBundle<S, T>,
        fault: CommitFault,
    ) -> Result<AtomicCommitReceipt, AtomicCommitError> {
        validate_bundle(&bundle)?;
        if let Some(existing) = self.state.transactions.get(&bundle.transaction_id) {
            if existing == &bundle {
                return Ok(AtomicCommitReceipt {
                    transaction_id: bundle.transaction_id,
                    next_offset: bundle.next_offset,
                    output_count: bundle.outputs.len(),
                    replayed: true,
                });
            }
            return Err(AtomicCommitError::TransactionIdentityConflict);
        }
        if self.state.next_offset != bundle.expected_next_offset {
            return Err(AtomicCommitError::SourceConflict {
                committed: self.state.next_offset,
                expected: bundle.expected_next_offset,
            });
        }
        for output in &bundle.outputs {
            if self.state.outbox.contains_key(&output.id) {
                return Err(AtomicCommitError::OutputIdentityConflict(
                    output.id.as_str().to_owned(),
                ));
            }
        }
        if fault == CommitFault::BeforeCommit {
            return Err(AtomicCommitError::FailedBeforeCommit);
        }

        let mut next = self.state.clone();
        next.next_offset = bundle.next_offset;
        next.checkpoint = Some(bundle.checkpoint.clone());
        for output in &bundle.outputs {
            next.outbox.insert(
                output.id.clone(),
                OutboxEntry {
                    output: output.clone(),
                    delivery_attempts: 0,
                    status: OutboxStatus::Pending,
                },
            );
        }
        next.transactions
            .insert(bundle.transaction_id.clone(), bundle.clone());
        self.state = next;

        if fault == CommitFault::AfterCommit {
            return Err(AtomicCommitError::CommitOutcomeUnknown);
        }
        Ok(AtomicCommitReceipt {
            transaction_id: bundle.transaction_id,
            next_offset: bundle.next_offset,
            output_count: bundle.outputs.len(),
            replayed: false,
        })
    }

    pub fn record_delivery_attempt(&mut self, id: &OutputId) -> Result<(), AtomicCommitError> {
        let entry = self
            .state
            .outbox
            .get_mut(id)
            .ok_or_else(|| AtomicCommitError::UnknownOutput(id.as_str().to_owned()))?;
        entry.delivery_attempts = entry
            .delivery_attempts
            .checked_add(1)
            .ok_or_else(|| AtomicCommitError::DeliveryAttemptOverflow(id.as_str().to_owned()))?;
        Ok(())
    }

    pub fn acknowledge(
        &mut self,
        id: &OutputId,
        acknowledgement: A,
    ) -> Result<(), AtomicCommitError> {
        let entry = self
            .state
            .outbox
            .get_mut(id)
            .ok_or_else(|| AtomicCommitError::UnknownOutput(id.as_str().to_owned()))?;
        entry.status = OutboxStatus::Delivered { acknowledgement };
        Ok(())
    }
}

fn validate_restored_state<S, T, A>(
    state: &AtomicCommitState<S, T, A>,
) -> Result<(), AtomicCommitError>
where
    S: PartialEq,
    T: PartialEq,
{
    if let Some(checkpoint) = &state.checkpoint {
        if checkpoint.offset != state.next_offset {
            return Err(AtomicCommitError::RestoredStateCorrupt(
                "checkpoint does not match the source cursor".into(),
            ));
        }
    }
    for (id, entry) in &state.outbox {
        if id != &entry.output.id {
            return Err(AtomicCommitError::RestoredStateCorrupt(
                "outbox key does not match its output identity".into(),
            ));
        }
    }

    let mut committed_output_ids = BTreeSet::new();
    let mut transactions: Vec<_> = state.transactions.iter().collect();
    transactions.sort_by_key(|(_, bundle)| bundle.expected_next_offset);
    for (transaction_id, bundle) in &transactions {
        validate_bundle(bundle)?;
        if *transaction_id != &bundle.transaction_id || bundle.next_offset > state.next_offset {
            return Err(AtomicCommitError::RestoredStateCorrupt(
                "transaction identity or source range is inconsistent".into(),
            ));
        }
        for output in &bundle.outputs {
            let Some(entry) = state.outbox.get(&output.id) else {
                return Err(AtomicCommitError::RestoredStateCorrupt(
                    "committed transaction output is absent from the outbox".into(),
                ));
            };
            if entry.output != *output || !committed_output_ids.insert(output.id.clone()) {
                return Err(AtomicCommitError::RestoredStateCorrupt(
                    "committed output content or identity is inconsistent".into(),
                ));
            }
        }
    }
    if transactions
        .windows(2)
        .any(|pair| pair[0].1.next_offset != pair[1].1.expected_next_offset)
    {
        return Err(AtomicCommitError::RestoredStateCorrupt(
            "committed source transaction ranges are not contiguous".into(),
        ));
    }
    if committed_output_ids.len() != state.outbox.len() {
        return Err(AtomicCommitError::RestoredStateCorrupt(
            "outbox contains an output with no committed transaction".into(),
        ));
    }
    let latest = transactions.last().map(|(_, bundle)| *bundle);
    match (latest, &state.checkpoint) {
        (Some(bundle), Some(checkpoint))
            if bundle.next_offset == state.next_offset && &bundle.checkpoint == checkpoint =>
        {
            Ok(())
        }
        (None, None) => Ok(()),
        _ => Err(AtomicCommitError::RestoredStateCorrupt(
            "latest transaction, checkpoint, and source cursor disagree".into(),
        )),
    }
}

fn validate_bundle<S, T>(bundle: &AtomicCommitBundle<S, T>) -> Result<(), AtomicCommitError> {
    if bundle.transaction_id.trim().is_empty() {
        return Err(AtomicCommitError::EmptyTransactionId);
    }
    if bundle.next_offset <= bundle.expected_next_offset {
        return Err(AtomicCommitError::EmptySourceRange {
            offset: bundle.expected_next_offset,
        });
    }
    if bundle.checkpoint.offset != bundle.next_offset {
        return Err(AtomicCommitError::CheckpointOffsetMismatch {
            checkpoint: bundle.checkpoint.offset,
            next: bundle.next_offset,
        });
    }
    let mut ids = BTreeSet::new();
    for output in &bundle.outputs {
        if !ids.insert(output.id.as_str()) {
            return Err(AtomicCommitError::DuplicateOutputId(
                output.id.as_str().to_owned(),
            ));
        }
        if output.source_offset < bundle.expected_next_offset
            || output.source_offset >= bundle.next_offset
        {
            return Err(AtomicCommitError::OutputOutsideTransaction {
                id: output.id.as_str().to_owned(),
                offset: output.source_offset,
                first: bundle.expected_next_offset,
                next: bundle.next_offset,
            });
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SinkDelivery<A> {
    Applied(A),
    Duplicate(A),
}

/// External sinks must make `OutputId` their idempotency identity.
pub trait IdempotentSink<T> {
    type Acknowledgement: Clone;
    type Error;

    fn deliver(
        &mut self,
        id: &OutputId,
        payload: &T,
    ) -> Result<SinkDelivery<Self::Acknowledgement>, Self::Error>;
}

#[derive(Debug, PartialEq, Eq)]
pub enum DrainOutboxError<SinkError> {
    Store(AtomicCommitError),
    Sink(SinkError),
}

/// Deliver every pending entry. A crash after sink acceptance but before acknowledgement is safe
/// only when the sink deduplicates the same [`OutputId`].
pub fn drain_outbox<S, T, A, Sink>(
    store: &mut InMemoryAtomicCommitStore<S, T, A>,
    sink: &mut Sink,
) -> Result<usize, DrainOutboxError<Sink::Error>>
where
    S: Clone + PartialEq,
    T: Clone + PartialEq,
    A: Clone,
    Sink: IdempotentSink<T, Acknowledgement = A>,
{
    let pending: Vec<_> = store
        .pending_outputs()
        .map(|entry| entry.output.clone())
        .collect();
    let mut delivered = 0;
    for output in pending {
        store
            .record_delivery_attempt(&output.id)
            .map_err(DrainOutboxError::Store)?;
        let acknowledgement = match sink
            .deliver(&output.id, &output.payload)
            .map_err(DrainOutboxError::Sink)?
        {
            SinkDelivery::Applied(acknowledgement) | SinkDelivery::Duplicate(acknowledgement) => {
                acknowledgement
            }
        };
        store
            .acknowledge(&output.id, acknowledgement)
            .map_err(DrainOutboxError::Store)?;
        delivered += 1;
    }
    Ok(delivered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CheckpointMeta;

    fn bundle() -> AtomicCommitBundle<u64, String> {
        AtomicCommitBundle {
            transaction_id: "feed/0/10-12".into(),
            expected_next_offset: 10,
            next_offset: 13,
            checkpoint: Checkpoint {
                snapshot: 99,
                offset: 13,
                watermark: Some(12),
                metadata: CheckpointMeta::default(),
            },
            outputs: vec![TransactionalOutput {
                id: OutputId::try_new("candidate/12/0").unwrap(),
                source_offset: 12,
                payload: "candidate".into(),
            }],
        }
    }

    #[test]
    fn source_checkpoint_and_outbox_commit_together() {
        let mut store = InMemoryAtomicCommitStore::<u64, String, String>::new(10);
        let receipt = store.commit(bundle()).unwrap();
        assert!(!receipt.replayed);
        assert_eq!(store.state().next_offset, 13);
        assert_eq!(store.state().checkpoint.as_ref().unwrap().snapshot, 99);
        assert_eq!(store.pending_outputs().count(), 1);
    }

    #[test]
    fn before_commit_failure_is_atomic_and_after_commit_failure_is_replayable() {
        let mut store = InMemoryAtomicCommitStore::<u64, String, String>::new(10);
        let before = store.state().clone();
        assert_eq!(
            store.commit_with_fault(bundle(), CommitFault::BeforeCommit),
            Err(AtomicCommitError::FailedBeforeCommit)
        );
        assert_eq!(store.state(), &before);

        assert_eq!(
            store.commit_with_fault(bundle(), CommitFault::AfterCommit),
            Err(AtomicCommitError::CommitOutcomeUnknown)
        );
        assert_eq!(store.state().next_offset, 13);
        let mut restarted = InMemoryAtomicCommitStore::restore(store.state().clone()).unwrap();
        let replay = restarted.commit(bundle()).unwrap();
        assert!(replay.replayed);
        assert_eq!(restarted.state().outbox.len(), 1);
    }

    #[derive(Default)]
    struct TestSink {
        accepted: BTreeMap<OutputId, String>,
        lose_first_ack: bool,
    }

    impl IdempotentSink<String> for TestSink {
        type Acknowledgement = String;
        type Error = &'static str;

        fn deliver(
            &mut self,
            id: &OutputId,
            payload: &String,
        ) -> Result<SinkDelivery<Self::Acknowledgement>, Self::Error> {
            if let Some(existing) = self.accepted.get(id) {
                return Ok(SinkDelivery::Duplicate(existing.clone()));
            }
            let acknowledgement = format!("ack/{}", id.as_str());
            self.accepted.insert(id.clone(), acknowledgement.clone());
            if self.lose_first_ack {
                self.lose_first_ack = false;
                Err("acknowledgement lost")
            } else {
                assert_eq!(payload, "candidate");
                Ok(SinkDelivery::Applied(acknowledgement))
            }
        }
    }

    #[test]
    fn sink_acknowledgement_loss_retries_without_duplicate_effect() {
        let mut store = InMemoryAtomicCommitStore::<u64, String, String>::new(10);
        store.commit(bundle()).unwrap();
        let mut sink = TestSink {
            lose_first_ack: true,
            ..TestSink::default()
        };
        assert_eq!(
            drain_outbox(&mut store, &mut sink),
            Err(DrainOutboxError::Sink("acknowledgement lost"))
        );
        assert_eq!(sink.accepted.len(), 1);
        assert_eq!(store.pending_outputs().count(), 1);
        assert_eq!(drain_outbox(&mut store, &mut sink), Ok(1));
        assert_eq!(sink.accepted.len(), 1);
        assert_eq!(store.pending_outputs().count(), 0);
        assert_eq!(
            store
                .state()
                .outbox
                .values()
                .next()
                .unwrap()
                .delivery_attempts,
            2
        );
    }

    #[test]
    fn invalid_or_conflicting_commits_leave_state_unchanged() {
        let mut store = InMemoryAtomicCommitStore::<u64, String, String>::new(10);
        let mut invalid = bundle();
        invalid.checkpoint.offset = 12;
        let before = store.state().clone();
        assert!(matches!(
            store.commit(invalid),
            Err(AtomicCommitError::CheckpointOffsetMismatch { .. })
        ));
        assert_eq!(store.state(), &before);

        store.commit(bundle()).unwrap();
        let mut changed_replay = bundle();
        changed_replay.checkpoint.snapshot = 100;
        assert_eq!(
            store.commit(changed_replay),
            Err(AtomicCommitError::TransactionIdentityConflict)
        );
    }

    #[test]
    fn restore_rejects_non_contiguous_transaction_history() {
        let mut store = InMemoryAtomicCommitStore::<u64, String, String>::new(10);
        store.commit(bundle()).unwrap();
        store
            .commit(AtomicCommitBundle {
                transaction_id: "feed/0/13".into(),
                expected_next_offset: 13,
                next_offset: 14,
                checkpoint: Checkpoint::new(101, 14),
                outputs: vec![],
            })
            .unwrap();
        let mut corrupt = store.state().clone();
        corrupt
            .transactions
            .get_mut("feed/0/13")
            .unwrap()
            .expected_next_offset = 12;
        assert!(matches!(
            InMemoryAtomicCommitStore::restore(corrupt),
            Err(AtomicCommitError::RestoredStateCorrupt(_))
        ));
    }
}
