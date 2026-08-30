use std::collections::{BTreeMap, BTreeSet};
use std::marker::PhantomData;

use helio_scan::{
    Emit, FallibleRestoreScan, FlushReason, FlushableScan, Scan, SnapshottingScan,
    VersionedSnapshot,
};
use helio_time::{AvailableAt, EffectiveAt};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use smallvec::SmallVec;
use thiserror::Error;

use crate::{CausalEvidence, HypothesisEffect, HypothesisModel, TimerId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HypothesisConfig {
    pub max_active: usize,
    pub max_terminal: usize,
    pub max_timers_per_hypothesis: usize,
    pub max_effects_per_transition: usize,
    pub max_timer_fires_per_advance: usize,
}

impl HypothesisConfig {
    pub fn try_new(
        max_active: usize,
        max_terminal: usize,
        max_timers_per_hypothesis: usize,
        max_effects_per_transition: usize,
        max_timer_fires_per_advance: usize,
    ) -> Result<Self, HypothesisConfigError> {
        if max_active == 0 {
            return Err(HypothesisConfigError::ZeroActiveCapacity);
        }
        if max_timer_fires_per_advance == 0 {
            return Err(HypothesisConfigError::ZeroAdvanceLimit);
        }
        Ok(Self {
            max_active,
            max_terminal,
            max_timers_per_hypothesis,
            max_effects_per_transition,
            max_timer_fires_per_advance,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum HypothesisConfigError {
    #[error("active hypothesis capacity must be positive")]
    ZeroActiveCapacity,
    #[error("timer fire limit per frontier advance must be positive")]
    ZeroAdvanceLimit,
}

/// Input to the keyed lifecycle runtime.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HypothesisInput<K, Evidence, Reason> {
    Open {
        key: K,
        evidence: CausalEvidence<Evidence>,
    },
    Evidence {
        key: K,
        evidence: CausalEvidence<Evidence>,
    },
    Advance {
        to: AvailableAt,
    },
    Close {
        key: K,
        sequence: u64,
        available_at: AvailableAt,
        reason: Reason,
    },
    Retract {
        key: K,
        sequence: u64,
        available_at: AvailableAt,
        reason: Reason,
    },
    /// Atomically terminate `key` and open `by` from replacement evidence.
    Supersede {
        key: K,
        by: K,
        sequence: u64,
        evidence: CausalEvidence<Evidence>,
        reason: Reason,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HypothesisRejection<ModelError> {
    ActiveCapacityExceeded {
        capacity: usize,
    },
    KeyAlreadyActive,
    KeyAlreadyTerminal,
    UnknownHypothesis,
    InitialSequenceMismatch {
        expected: u64,
        found: u64,
    },
    AvailabilityAtOrBeforeFrontier {
        available_at: AvailableAt,
        frontier: AvailableAt,
    },
    AvailabilityRegression {
        previous: AvailableAt,
        found: AvailableAt,
    },
    OverdueTimer {
        timer_id: TimerId,
        scheduled_at: AvailableAt,
        input_available_at: AvailableAt,
    },
    SequenceMismatch {
        expected: u64,
        found: u64,
    },
    SequenceExhausted,
    RevisionExhausted,
    FrontierRegression {
        previous: AvailableAt,
        found: AvailableAt,
    },
    FrontierBeforeEvidence {
        latest_evidence: AvailableAt,
        found: AvailableAt,
    },
    TimerAtOrBeforeCurrent {
        timer_id: TimerId,
        at: AvailableAt,
        current: AvailableAt,
    },
    UnknownTimer {
        timer_id: TimerId,
    },
    TimerCapacityExceeded {
        capacity: usize,
    },
    EffectCapacityExceeded {
        capacity: usize,
    },
    InvalidCompletionEffects,
    TimerFireLimitExceeded {
        limit: usize,
    },
    InvalidRuntimeState,
    Model(ModelError),
}

/// Observable lifecycle and model output. Every accepted mutation carries a monotonically
/// increasing per-hypothesis revision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HypothesisEvent<K, Output, Reason, ModelError> {
    Opened {
        key: K,
        sequence: u64,
        effective_at: EffectiveAt,
        available_at: AvailableAt,
        revision: u64,
    },
    EvidenceAccepted {
        key: K,
        sequence: u64,
        effective_at: EffectiveAt,
        available_at: AvailableAt,
        revision: u64,
    },
    ModelOutput {
        key: K,
        revision: u64,
        output: Output,
    },
    TimerScheduled {
        key: K,
        revision: u64,
        timer_id: TimerId,
        at: AvailableAt,
        replaced: Option<AvailableAt>,
    },
    TimerCancelled {
        key: K,
        revision: u64,
        timer_id: TimerId,
        scheduled_at: AvailableAt,
    },
    TimerFired {
        key: K,
        revision: u64,
        timer_id: TimerId,
        at: AvailableAt,
    },
    Completed {
        key: K,
        revision: u64,
        available_at: AvailableAt,
    },
    Closed {
        key: K,
        revision: u64,
        available_at: AvailableAt,
        reason: Reason,
    },
    Retracted {
        key: K,
        revision: u64,
        available_at: AvailableAt,
        reason: Reason,
    },
    Superseded {
        key: K,
        by: K,
        revision: u64,
        available_at: AvailableAt,
        reason: Reason,
    },
    TerminalEvicted {
        key: K,
    },
    FrontierAdvanced {
        previous: Option<AvailableAt>,
        current: AvailableAt,
        timers_fired: usize,
    },
    Rejected {
        key: Option<K>,
        error: HypothesisRejection<ModelError>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActiveHypothesis<State> {
    pub opened_effective_at: EffectiveAt,
    pub opened_available_at: AvailableAt,
    pub last_available_at: AvailableAt,
    pub last_sequence: u64,
    pub revision: u64,
    pub model_state: State,
    pub timers: BTreeMap<TimerId, AvailableAt>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TerminalStatus<K, Reason> {
    Completed {
        available_at: AvailableAt,
        revision: u64,
    },
    Closed {
        available_at: AvailableAt,
        revision: u64,
        reason: Reason,
    },
    Retracted {
        available_at: AvailableAt,
        revision: u64,
        reason: Reason,
    },
    Superseded {
        available_at: AvailableAt,
        revision: u64,
        by: K,
        reason: Reason,
    },
}

impl<K, Reason> TerminalStatus<K, Reason> {
    pub const fn available_at(&self) -> AvailableAt {
        match self {
            Self::Completed { available_at, .. }
            | Self::Closed { available_at, .. }
            | Self::Retracted { available_at, .. }
            | Self::Superseded { available_at, .. } => *available_at,
        }
    }

    pub const fn revision(&self) -> u64 {
        match self {
            Self::Completed { revision, .. }
            | Self::Closed { revision, .. }
            | Self::Retracted { revision, .. }
            | Self::Superseded { revision, .. } => *revision,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TerminalHypothesis<K, Reason> {
    pub status: TerminalStatus<K, Reason>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct TimerKey<K> {
    at: AvailableAt,
    key: K,
    timer_id: TimerId,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HypothesisState<K, State, Reason> {
    frontier: Option<AvailableAt>,
    latest_input_available_at: Option<AvailableAt>,
    active: BTreeMap<K, ActiveHypothesis<State>>,
    terminal: BTreeMap<K, TerminalHypothesis<K, Reason>>,
    timer_queue: BTreeSet<TimerKey<K>>,
}

impl<K, State, Reason> HypothesisState<K, State, Reason> {
    pub const fn frontier(&self) -> Option<AvailableAt> {
        self.frontier
    }

    pub const fn latest_input_available_at(&self) -> Option<AvailableAt> {
        self.latest_input_available_at
    }

    pub fn active_count(&self) -> usize {
        self.active.len()
    }

    pub fn terminal_count(&self) -> usize {
        self.terminal.len()
    }

    pub fn get(&self, key: &K) -> Option<&ActiveHypothesis<State>>
    where
        K: Ord,
    {
        self.active.get(key)
    }

    pub fn active(&self) -> impl ExactSizeIterator<Item = (&K, &ActiveHypothesis<State>)> {
        self.active.iter()
    }

    pub fn terminal(&self) -> impl ExactSizeIterator<Item = (&K, &TerminalHypothesis<K, Reason>)> {
        self.terminal.iter()
    }

    /// Earliest availability-time deadline, useful for a durable host scheduler.
    pub fn next_timer_at(&self) -> Option<AvailableAt>
    where
        K: Ord,
    {
        self.timer_queue.first().map(|timer| timer.at)
    }
}

/// Stable snapshot. The timer queue is rebuilt from the per-hypothesis timer maps on restore.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "K: Ord + Serialize, State: Serialize, Reason: Serialize",
    deserialize = "K: Ord + Deserialize<'de>, State: Deserialize<'de>, Reason: Deserialize<'de>"
))]
pub struct HypothesisSnapshot<K, State, Reason> {
    pub frontier: Option<AvailableAt>,
    pub latest_input_available_at: Option<AvailableAt>,
    pub active: BTreeMap<K, ActiveHypothesis<State>>,
    pub terminal: BTreeMap<K, TerminalHypothesis<K, Reason>>,
}

impl<K, State, Reason> VersionedSnapshot for HypothesisSnapshot<K, State, Reason> {
    const VERSION: u32 = 1;
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum HypothesisRestoreError<K, ModelError> {
    #[error("snapshot has {found} active hypotheses; capacity is {capacity}")]
    ActiveCapacityExceeded { found: usize, capacity: usize },
    #[error("snapshot has {found} terminal hypotheses; capacity is {capacity}")]
    TerminalCapacityExceeded { found: usize, capacity: usize },
    #[error("snapshot key is both active and terminal: {key:?}")]
    ActiveTerminalKeyOverlap { key: K },
    #[error("snapshot has an invalid revision for key {key:?}")]
    InvalidRevision { key: K },
    #[error("snapshot has an invalid sequence for key {key:?}")]
    InvalidSequence { key: K },
    #[error("snapshot has invalid availability bounds for key {key:?}")]
    InvalidAvailability { key: K },
    #[error("snapshot key {key:?} has {found} timers; capacity is {capacity}")]
    TimerCapacityExceeded {
        key: K,
        found: usize,
        capacity: usize,
    },
    #[error(
        "snapshot timer {timer_id:?} for key {key:?} is due at {at:?}, not after frontier {frontier:?}"
    )]
    TimerAtOrBeforeFrontier {
        key: K,
        timer_id: TimerId,
        at: AvailableAt,
        frontier: AvailableAt,
    },
    #[error(
        "snapshot timer {timer_id:?} for key {key:?} is due at {at:?}, before latest input {latest_input_available_at:?}"
    )]
    TimerBeforeLatestInput {
        key: K,
        timer_id: TimerId,
        at: AvailableAt,
        latest_input_available_at: AvailableAt,
    },
    #[error("snapshot model state is invalid for key {key:?}: {error:?}")]
    Model { key: K, error: ModelError },
}

#[derive(Debug, Clone)]
pub struct KeyedHypothesisMachine<K, Model, Reason> {
    model: Model,
    config: HypothesisConfig,
    _types: PhantomData<fn() -> (K, Reason)>,
}

impl<K, Model, Reason> KeyedHypothesisMachine<K, Model, Reason> {
    pub fn try_new(model: Model, config: HypothesisConfig) -> Result<Self, HypothesisConfigError> {
        if config.max_active == 0 {
            return Err(HypothesisConfigError::ZeroActiveCapacity);
        }
        if config.max_timer_fires_per_advance == 0 {
            return Err(HypothesisConfigError::ZeroAdvanceLimit);
        }
        Ok(Self {
            model,
            config,
            _types: PhantomData,
        })
    }

    pub const fn config(&self) -> &HypothesisConfig {
        &self.config
    }

    pub const fn model(&self) -> &Model {
        &self.model
    }
}

type EventFor<K, M, R> =
    HypothesisEvent<K, <M as HypothesisModel<K>>::Output, R, <M as HypothesisModel<K>>::Error>;

type StateFor<K, M, R> = HypothesisState<K, <M as HypothesisModel<K>>::State, R>;
type EventBuffer<K, M, R> = SmallVec<[EventFor<K, M, R>; 8]>;
type PrepareResult<K, M, R> = Result<
    PreparedTransition<
        K,
        <M as HypothesisModel<K>>::State,
        <M as HypothesisModel<K>>::Output,
        R,
        <M as HypothesisModel<K>>::Error,
    >,
    HypothesisRejection<<M as HypothesisModel<K>>::Error>,
>;
type EventResult<K, M, R> =
    Result<EventBuffer<K, M, R>, HypothesisRejection<<M as HypothesisModel<K>>::Error>>;
type AdvanceResult<K, M, R> = Result<
    (StateFor<K, M, R>, Vec<EventFor<K, M, R>>),
    (
        Option<K>,
        HypothesisRejection<<M as HypothesisModel<K>>::Error>,
    ),
>;

struct PreparedTransition<K, State, Output, Reason, ModelError> {
    record: ActiveHypothesis<State>,
    effects: SmallVec<[HypothesisEvent<K, Output, Reason, ModelError>; 4]>,
    complete: bool,
}

struct SupersedeCommand<K, Evidence, Reason> {
    key: K,
    by: K,
    sequence: u64,
    evidence: CausalEvidence<Evidence>,
    reason: Reason,
}

impl<K, Model, Reason> KeyedHypothesisMachine<K, Model, Reason>
where
    K: Clone + Ord,
    Model: HypothesisModel<K>,
    Model::State: Clone,
    Reason: Clone,
{
    fn empty_state(&self) -> StateFor<K, Model, Reason> {
        HypothesisState {
            frontier: None,
            latest_input_available_at: None,
            active: BTreeMap::new(),
            terminal: BTreeMap::new(),
            timer_queue: BTreeSet::new(),
        }
    }

    fn reject<E: Emit<EventFor<K, Model, Reason>>>(
        &self,
        key: Option<K>,
        error: HypothesisRejection<Model::Error>,
        emit: &mut E,
    ) {
        emit.emit(HypothesisEvent::Rejected { key, error });
    }

    fn one_event(&self, event: EventFor<K, Model, Reason>) -> EventBuffer<K, Model, Reason> {
        let mut events = SmallVec::new();
        events.push(event);
        events
    }

    fn validate_input_time(
        &self,
        state: &StateFor<K, Model, Reason>,
        available_at: AvailableAt,
    ) -> Result<(), HypothesisRejection<Model::Error>> {
        if let Some(frontier) = state.frontier {
            if available_at <= frontier {
                return Err(HypothesisRejection::AvailabilityAtOrBeforeFrontier {
                    available_at,
                    frontier,
                });
            }
        }
        if let Some(previous) = state.latest_input_available_at {
            if available_at < previous {
                return Err(HypothesisRejection::AvailabilityRegression {
                    previous,
                    found: available_at,
                });
            }
        }
        if let Some(timer) = state.timer_queue.first() {
            if timer.at < available_at {
                return Err(HypothesisRejection::OverdueTimer {
                    timer_id: timer.timer_id,
                    scheduled_at: timer.at,
                    input_available_at: available_at,
                });
            }
        }
        Ok(())
    }

    fn validate_initial_sequence(
        &self,
        sequence: u64,
    ) -> Result<(), HypothesisRejection<Model::Error>> {
        if sequence == 0 {
            Ok(())
        } else {
            Err(HypothesisRejection::InitialSequenceMismatch {
                expected: 0,
                found: sequence,
            })
        }
    }

    fn validate_new_key(
        &self,
        state: &StateFor<K, Model, Reason>,
        key: &K,
    ) -> Result<(), HypothesisRejection<Model::Error>> {
        if state.active.contains_key(key) {
            return Err(HypothesisRejection::KeyAlreadyActive);
        }
        if state.terminal.contains_key(key) {
            return Err(HypothesisRejection::KeyAlreadyTerminal);
        }
        Ok(())
    }

    fn next_sequence(
        &self,
        record: &ActiveHypothesis<Model::State>,
        found: u64,
    ) -> Result<(), HypothesisRejection<Model::Error>> {
        let expected = record
            .last_sequence
            .checked_add(1)
            .ok_or(HypothesisRejection::SequenceExhausted)?;
        if found == expected {
            Ok(())
        } else {
            Err(HypothesisRejection::SequenceMismatch { expected, found })
        }
    }

    fn next_revision(
        &self,
        record: &ActiveHypothesis<Model::State>,
    ) -> Result<u64, HypothesisRejection<Model::Error>> {
        record
            .revision
            .checked_add(1)
            .ok_or(HypothesisRejection::RevisionExhausted)
    }

    fn prepare_transition(
        &self,
        key: &K,
        mut record: ActiveHypothesis<Model::State>,
        revision: u64,
        current: AvailableAt,
        effects: SmallVec<[HypothesisEffect<Model::Output>; 4]>,
    ) -> PrepareResult<K, Model, Reason> {
        if effects.len() > self.config.max_effects_per_transition {
            return Err(HypothesisRejection::EffectCapacityExceeded {
                capacity: self.config.max_effects_per_transition,
            });
        }
        self.model
            .validate(key, &record.model_state)
            .map_err(HypothesisRejection::Model)?;

        let complete_count = effects
            .iter()
            .filter(|effect| matches!(effect, HypothesisEffect::Complete))
            .count();
        let complete = complete_count == 1;
        if complete_count > 1
            || (complete
                && effects.iter().any(|effect| {
                    matches!(
                        effect,
                        HypothesisEffect::Schedule { .. } | HypothesisEffect::Cancel { .. }
                    )
                }))
        {
            return Err(HypothesisRejection::InvalidCompletionEffects);
        }

        let mut prepared = SmallVec::new();
        for effect in effects {
            match effect {
                HypothesisEffect::Emit(output) => {
                    prepared.push(HypothesisEvent::ModelOutput {
                        key: key.clone(),
                        revision,
                        output,
                    });
                }
                HypothesisEffect::Schedule { timer_id, at } => {
                    if at <= current {
                        return Err(HypothesisRejection::TimerAtOrBeforeCurrent {
                            timer_id,
                            at,
                            current,
                        });
                    }
                    let replaced = record.timers.insert(timer_id, at);
                    prepared.push(HypothesisEvent::TimerScheduled {
                        key: key.clone(),
                        revision,
                        timer_id,
                        at,
                        replaced,
                    });
                }
                HypothesisEffect::Cancel { timer_id } => {
                    let Some(scheduled_at) = record.timers.remove(&timer_id) else {
                        return Err(HypothesisRejection::UnknownTimer { timer_id });
                    };
                    prepared.push(HypothesisEvent::TimerCancelled {
                        key: key.clone(),
                        revision,
                        timer_id,
                        scheduled_at,
                    });
                }
                HypothesisEffect::Complete => {}
            }
        }
        if record.timers.len() > self.config.max_timers_per_hypothesis {
            return Err(HypothesisRejection::TimerCapacityExceeded {
                capacity: self.config.max_timers_per_hypothesis,
            });
        }
        record.revision = revision;
        Ok(PreparedTransition {
            record,
            effects: prepared,
            complete,
        })
    }

    fn remove_queued_timers(
        &self,
        state: &mut StateFor<K, Model, Reason>,
        key: &K,
        timers: &BTreeMap<TimerId, AvailableAt>,
    ) {
        for (&timer_id, &at) in timers {
            state.timer_queue.remove(&TimerKey {
                at,
                key: key.clone(),
                timer_id,
            });
        }
    }

    fn insert_queued_timers(
        &self,
        state: &mut StateFor<K, Model, Reason>,
        key: &K,
        timers: &BTreeMap<TimerId, AvailableAt>,
    ) {
        for (&timer_id, &at) in timers {
            state.timer_queue.insert(TimerKey {
                at,
                key: key.clone(),
                timer_id,
            });
        }
    }

    fn retain_terminal(
        &self,
        state: &mut StateFor<K, Model, Reason>,
        key: K,
        status: TerminalStatus<K, Reason>,
        events: &mut EventBuffer<K, Model, Reason>,
    ) {
        state.terminal.insert(key, TerminalHypothesis { status });
        while state.terminal.len() > self.config.max_terminal {
            let evict = state
                .terminal
                .iter()
                .min_by(|(left_key, left), (right_key, right)| {
                    (left.status.available_at(), *left_key)
                        .cmp(&(right.status.available_at(), *right_key))
                })
                .map(|(key, _)| key.clone());
            let Some(evict) = evict else {
                break;
            };
            state.terminal.remove(&evict);
            events.push(HypothesisEvent::TerminalEvicted { key: evict });
        }
    }

    fn commit_transition(
        &self,
        state: &mut StateFor<K, Model, Reason>,
        key: K,
        old_timers: BTreeMap<TimerId, AvailableAt>,
        prepared: PreparedTransition<K, Model::State, Model::Output, Reason, Model::Error>,
        mut prefix: EventBuffer<K, Model, Reason>,
    ) -> EventBuffer<K, Model, Reason> {
        self.remove_queued_timers(state, &key, &old_timers);
        prefix.extend(prepared.effects);
        if prepared.complete {
            state.active.remove(&key);
            let revision = prepared.record.revision;
            let available_at = prepared.record.last_available_at;
            prefix.push(HypothesisEvent::Completed {
                key: key.clone(),
                revision,
                available_at,
            });
            self.retain_terminal(
                state,
                key,
                TerminalStatus::Completed {
                    available_at,
                    revision,
                },
                &mut prefix,
            );
        } else {
            self.insert_queued_timers(state, &key, &prepared.record.timers);
            state.active.insert(key, prepared.record);
        }
        prefix
    }

    fn process_open<E: Emit<EventFor<K, Model, Reason>>>(
        &self,
        state: &mut StateFor<K, Model, Reason>,
        key: K,
        evidence: CausalEvidence<Model::Evidence>,
        emit: &mut E,
    ) {
        if let Err(error) = self.validate_input_time(state, evidence.available_at) {
            self.reject(Some(key), error, emit);
            return;
        }
        if let Err(error) = self.validate_new_key(state, &key) {
            self.reject(Some(key), error, emit);
            return;
        }
        if let Err(error) = self.validate_initial_sequence(evidence.sequence) {
            self.reject(Some(key), error, emit);
            return;
        }
        if state.active.len() >= self.config.max_active {
            self.reject(
                Some(key),
                HypothesisRejection::ActiveCapacityExceeded {
                    capacity: self.config.max_active,
                },
                emit,
            );
            return;
        }

        let sequence = evidence.sequence;
        let effective_at = evidence.effective_at;
        let available_at = evidence.available_at;
        let transition = match self.model.open(&key, evidence) {
            Ok(transition) => transition,
            Err(error) => {
                self.reject(Some(key), HypothesisRejection::Model(error), emit);
                return;
            }
        };
        let (next_state, effects) = transition.into_parts();
        let record = ActiveHypothesis {
            opened_effective_at: effective_at,
            opened_available_at: available_at,
            last_available_at: available_at,
            last_sequence: sequence,
            revision: 1,
            model_state: next_state,
            timers: BTreeMap::new(),
        };
        let prepared = match self.prepare_transition(&key, record, 1, available_at, effects) {
            Ok(prepared) => prepared,
            Err(error) => {
                self.reject(Some(key), error, emit);
                return;
            }
        };
        let events = self.commit_transition(
            state,
            key.clone(),
            BTreeMap::new(),
            prepared,
            self.one_event(HypothesisEvent::Opened {
                key,
                sequence,
                effective_at,
                available_at,
                revision: 1,
            }),
        );
        state.latest_input_available_at = Some(available_at);
        for event in events {
            emit.emit(event);
        }
    }

    fn process_evidence<E: Emit<EventFor<K, Model, Reason>>>(
        &self,
        state: &mut StateFor<K, Model, Reason>,
        key: K,
        evidence: CausalEvidence<Model::Evidence>,
        emit: &mut E,
    ) {
        if let Err(error) = self.validate_input_time(state, evidence.available_at) {
            self.reject(Some(key), error, emit);
            return;
        }
        let Some(existing) = state.active.get(&key) else {
            self.reject(Some(key), HypothesisRejection::UnknownHypothesis, emit);
            return;
        };
        if let Err(error) = self.next_sequence(existing, evidence.sequence) {
            self.reject(Some(key), error, emit);
            return;
        }
        let revision = match self.next_revision(existing) {
            Ok(revision) => revision,
            Err(error) => {
                self.reject(Some(key), error, emit);
                return;
            }
        };
        let sequence = evidence.sequence;
        let effective_at = evidence.effective_at;
        let available_at = evidence.available_at;
        let transition = match self.model.update(&key, &existing.model_state, evidence) {
            Ok(transition) => transition,
            Err(error) => {
                self.reject(Some(key), HypothesisRejection::Model(error), emit);
                return;
            }
        };
        let old_timers = existing.timers.clone();
        let timers = old_timers.clone();
        let opened_effective_at = existing.opened_effective_at;
        let opened_available_at = existing.opened_available_at;
        let (next_state, effects) = transition.into_parts();
        let record = ActiveHypothesis {
            opened_effective_at,
            opened_available_at,
            last_available_at: available_at,
            last_sequence: sequence,
            revision,
            model_state: next_state,
            timers,
        };
        let prepared = match self.prepare_transition(&key, record, revision, available_at, effects)
        {
            Ok(prepared) => prepared,
            Err(error) => {
                self.reject(Some(key), error, emit);
                return;
            }
        };
        let events = self.commit_transition(
            state,
            key.clone(),
            old_timers,
            prepared,
            self.one_event(HypothesisEvent::EvidenceAccepted {
                key,
                sequence,
                effective_at,
                available_at,
                revision,
            }),
        );
        state.latest_input_available_at = Some(available_at);
        for event in events {
            emit.emit(event);
        }
    }

    fn terminalize(
        &self,
        state: &mut StateFor<K, Model, Reason>,
        key: K,
        sequence: u64,
        available_at: AvailableAt,
        reason: Reason,
        retracted: bool,
    ) -> EventResult<K, Model, Reason> {
        self.validate_input_time(state, available_at)?;
        let Some(record) = state.active.get(&key) else {
            return Err(HypothesisRejection::UnknownHypothesis);
        };
        self.next_sequence(record, sequence)?;
        let revision = self.next_revision(record)?;
        let timers = record.timers.clone();
        self.remove_queued_timers(state, &key, &timers);
        state.active.remove(&key);
        let mut events = if retracted {
            self.one_event(HypothesisEvent::Retracted {
                key: key.clone(),
                revision,
                available_at,
                reason: reason.clone(),
            })
        } else {
            self.one_event(HypothesisEvent::Closed {
                key: key.clone(),
                revision,
                available_at,
                reason: reason.clone(),
            })
        };
        let status = if retracted {
            TerminalStatus::Retracted {
                available_at,
                revision,
                reason,
            }
        } else {
            TerminalStatus::Closed {
                available_at,
                revision,
                reason,
            }
        };
        self.retain_terminal(state, key, status, &mut events);
        state.latest_input_available_at = Some(available_at);
        Ok(events)
    }

    fn process_supersede<E: Emit<EventFor<K, Model, Reason>>>(
        &self,
        state: &mut StateFor<K, Model, Reason>,
        command: SupersedeCommand<K, Model::Evidence, Reason>,
        emit: &mut E,
    ) {
        let SupersedeCommand {
            key,
            by,
            sequence,
            evidence,
            reason,
        } = command;
        let available_at = evidence.available_at;
        if let Err(error) = self.validate_input_time(state, available_at) {
            self.reject(Some(key), error, emit);
            return;
        }
        let Some(old) = state.active.get(&key) else {
            self.reject(Some(key), HypothesisRejection::UnknownHypothesis, emit);
            return;
        };
        if let Err(error) = self.next_sequence(old, sequence) {
            self.reject(Some(key), error, emit);
            return;
        }
        if let Err(error) = self.validate_new_key(state, &by) {
            self.reject(Some(by), error, emit);
            return;
        }
        if let Err(error) = self.validate_initial_sequence(evidence.sequence) {
            self.reject(Some(by), error, emit);
            return;
        }
        let old_revision = match self.next_revision(old) {
            Ok(revision) => revision,
            Err(error) => {
                self.reject(Some(key), error, emit);
                return;
            }
        };
        let old_timers = old.timers.clone();

        let initial_sequence = evidence.sequence;
        let effective_at = evidence.effective_at;
        let transition = match self.model.open(&by, evidence) {
            Ok(transition) => transition,
            Err(error) => {
                self.reject(Some(by), HypothesisRejection::Model(error), emit);
                return;
            }
        };
        let (next_state, effects) = transition.into_parts();
        let replacement = ActiveHypothesis {
            opened_effective_at: effective_at,
            opened_available_at: available_at,
            last_available_at: available_at,
            last_sequence: initial_sequence,
            revision: 1,
            model_state: next_state,
            timers: BTreeMap::new(),
        };
        let prepared = match self.prepare_transition(&by, replacement, 1, available_at, effects) {
            Ok(prepared) => prepared,
            Err(error) => {
                self.reject(Some(by), error, emit);
                return;
            }
        };

        self.remove_queued_timers(state, &key, &old_timers);
        state.active.remove(&key);
        let mut events = self.one_event(HypothesisEvent::Superseded {
            key: key.clone(),
            by: by.clone(),
            revision: old_revision,
            available_at,
            reason: reason.clone(),
        });
        self.retain_terminal(
            state,
            key,
            TerminalStatus::Superseded {
                available_at,
                revision: old_revision,
                by: by.clone(),
                reason,
            },
            &mut events,
        );
        events.push(HypothesisEvent::Opened {
            key: by.clone(),
            sequence: initial_sequence,
            effective_at,
            available_at,
            revision: 1,
        });
        events = self.commit_transition(state, by, BTreeMap::new(), prepared, events);
        state.latest_input_available_at = Some(available_at);
        for event in events {
            emit.emit(event);
        }
    }

    fn advance_trial(
        &self,
        state: &StateFor<K, Model, Reason>,
        to: AvailableAt,
    ) -> AdvanceResult<K, Model, Reason> {
        let previous = self
            .validate_advance(state, to)
            .map_err(|error| (None, error))?;

        let mut trial = state.clone();
        let mut events = Vec::new();
        let mut timers_fired = 0_usize;
        loop {
            let Some(timer) = trial.timer_queue.first().cloned() else {
                break;
            };
            if timer.at > to {
                break;
            }
            if timers_fired >= self.config.max_timer_fires_per_advance {
                return Err((
                    Some(timer.key),
                    HypothesisRejection::TimerFireLimitExceeded {
                        limit: self.config.max_timer_fires_per_advance,
                    },
                ));
            }
            let key = timer.key.clone();
            let Some(existing) = trial.active.get(&key) else {
                return Err((Some(key), HypothesisRejection::InvalidRuntimeState));
            };
            if existing.timers.get(&timer.timer_id) != Some(&timer.at) {
                return Err((Some(key), HypothesisRejection::InvalidRuntimeState));
            }
            let revision = self
                .next_revision(existing)
                .map_err(|error| (Some(key.clone()), error))?;
            let transition = self
                .model
                .on_timer(&key, &existing.model_state, timer.timer_id, timer.at)
                .map_err(|error| (Some(key.clone()), HypothesisRejection::Model(error)))?;
            let old_timers = existing.timers.clone();
            let mut timers = old_timers.clone();
            timers.remove(&timer.timer_id);
            let (next_state, effects) = transition.into_parts();
            let record = ActiveHypothesis {
                opened_effective_at: existing.opened_effective_at,
                opened_available_at: existing.opened_available_at,
                last_available_at: timer.at,
                last_sequence: existing.last_sequence,
                revision,
                model_state: next_state,
                timers,
            };
            let prepared = self
                .prepare_transition(&key, record, revision, timer.at, effects)
                .map_err(|error| (Some(key.clone()), error))?;
            let fired = HypothesisEvent::TimerFired {
                key: key.clone(),
                revision,
                timer_id: timer.timer_id,
                at: timer.at,
            };
            let committed = self.commit_transition(
                &mut trial,
                key,
                old_timers,
                prepared,
                self.one_event(fired),
            );
            events.extend(committed);
            timers_fired += 1;
        }
        trial.frontier = Some(to);
        events.push(HypothesisEvent::FrontierAdvanced {
            previous,
            current: to,
            timers_fired,
        });
        Ok((trial, events))
    }

    fn validate_advance(
        &self,
        state: &StateFor<K, Model, Reason>,
        to: AvailableAt,
    ) -> Result<Option<AvailableAt>, HypothesisRejection<Model::Error>> {
        if let Some(previous) = state.frontier {
            if to <= previous {
                return Err(HypothesisRejection::FrontierRegression {
                    previous,
                    found: to,
                });
            }
        }
        if let Some(latest_evidence) = state.latest_input_available_at {
            if to < latest_evidence {
                return Err(HypothesisRejection::FrontierBeforeEvidence {
                    latest_evidence,
                    found: to,
                });
            }
        }
        Ok(state.frontier)
    }

    fn process_advance<E: Emit<EventFor<K, Model, Reason>>>(
        &self,
        state: &mut StateFor<K, Model, Reason>,
        to: AvailableAt,
        emit: &mut E,
    ) {
        let previous = match self.validate_advance(state, to) {
            Ok(previous) => previous,
            Err(error) => {
                self.reject(None, error, emit);
                return;
            }
        };
        if state.timer_queue.first().is_none_or(|timer| timer.at > to) {
            state.frontier = Some(to);
            emit.emit(HypothesisEvent::FrontierAdvanced {
                previous,
                current: to,
                timers_fired: 0,
            });
            return;
        }
        match self.advance_trial(state, to) {
            Ok((next, events)) => {
                *state = next;
                for event in events {
                    emit.emit(event);
                }
            }
            Err((key, error)) => self.reject(key, error, emit),
        }
    }

    fn snapshot_from_state(
        &self,
        state: &StateFor<K, Model, Reason>,
    ) -> HypothesisSnapshot<K, Model::State, Reason> {
        HypothesisSnapshot {
            frontier: state.frontier,
            latest_input_available_at: state.latest_input_available_at,
            active: state.active.clone(),
            terminal: state.terminal.clone(),
        }
    }

    fn state_from_snapshot(
        &self,
        snapshot: HypothesisSnapshot<K, Model::State, Reason>,
    ) -> StateFor<K, Model, Reason> {
        let mut timer_queue = BTreeSet::new();
        for (key, record) in &snapshot.active {
            for (&timer_id, &at) in &record.timers {
                timer_queue.insert(TimerKey {
                    at,
                    key: key.clone(),
                    timer_id,
                });
            }
        }
        HypothesisState {
            frontier: snapshot.frontier,
            latest_input_available_at: snapshot.latest_input_available_at,
            active: snapshot.active,
            terminal: snapshot.terminal,
            timer_queue,
        }
    }

    fn validate_snapshot(
        &self,
        snapshot: &HypothesisSnapshot<K, Model::State, Reason>,
    ) -> Result<(), HypothesisRestoreError<K, Model::Error>> {
        if snapshot.active.len() > self.config.max_active {
            return Err(HypothesisRestoreError::ActiveCapacityExceeded {
                found: snapshot.active.len(),
                capacity: self.config.max_active,
            });
        }
        if snapshot.terminal.len() > self.config.max_terminal {
            return Err(HypothesisRestoreError::TerminalCapacityExceeded {
                found: snapshot.terminal.len(),
                capacity: self.config.max_terminal,
            });
        }
        for key in snapshot.active.keys() {
            if snapshot.terminal.contains_key(key) {
                return Err(HypothesisRestoreError::ActiveTerminalKeyOverlap { key: key.clone() });
            }
        }
        let causal_bound = match (snapshot.frontier, snapshot.latest_input_available_at) {
            (Some(left), Some(right)) => Some(left.max(right)),
            (left, right) => left.or(right),
        };
        for (key, record) in &snapshot.active {
            if record.revision == 0 {
                return Err(HypothesisRestoreError::InvalidRevision { key: key.clone() });
            }
            if record
                .last_sequence
                .checked_add(1)
                .is_none_or(|minimum_revision| record.revision < minimum_revision)
            {
                return Err(HypothesisRestoreError::InvalidSequence { key: key.clone() });
            }
            if record.last_available_at < record.opened_available_at
                || causal_bound.is_none_or(|bound| record.last_available_at > bound)
            {
                return Err(HypothesisRestoreError::InvalidAvailability { key: key.clone() });
            }
            if record.timers.len() > self.config.max_timers_per_hypothesis {
                return Err(HypothesisRestoreError::TimerCapacityExceeded {
                    key: key.clone(),
                    found: record.timers.len(),
                    capacity: self.config.max_timers_per_hypothesis,
                });
            }
            if let Some(frontier) = snapshot.frontier {
                if let Some((&timer_id, &at)) =
                    record.timers.iter().find(|(_, at)| **at <= frontier)
                {
                    return Err(HypothesisRestoreError::TimerAtOrBeforeFrontier {
                        key: key.clone(),
                        timer_id,
                        at,
                        frontier,
                    });
                }
            }
            if let Some(latest_input_available_at) = snapshot.latest_input_available_at {
                if let Some((&timer_id, &at)) = record
                    .timers
                    .iter()
                    .find(|(_, at)| **at < latest_input_available_at)
                {
                    return Err(HypothesisRestoreError::TimerBeforeLatestInput {
                        key: key.clone(),
                        timer_id,
                        at,
                        latest_input_available_at,
                    });
                }
            }
            self.model
                .validate(key, &record.model_state)
                .map_err(|error| HypothesisRestoreError::Model {
                    key: key.clone(),
                    error,
                })?;
        }
        for (key, terminal) in &snapshot.terminal {
            if terminal.status.revision() == 0 {
                return Err(HypothesisRestoreError::InvalidRevision { key: key.clone() });
            }
            if causal_bound.is_none_or(|bound| terminal.status.available_at() > bound) {
                return Err(HypothesisRestoreError::InvalidAvailability { key: key.clone() });
            }
        }
        Ok(())
    }
}

impl<K, Model, Reason> Scan for KeyedHypothesisMachine<K, Model, Reason>
where
    K: Clone + Ord,
    Model: HypothesisModel<K>,
    Model::State: Clone,
    Reason: Clone,
{
    type In = HypothesisInput<K, Model::Evidence, Reason>;
    type Out = EventFor<K, Model, Reason>;
    type State = StateFor<K, Model, Reason>;

    fn init(&self) -> Self::State {
        self.empty_state()
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        match input {
            HypothesisInput::Open { key, evidence } => {
                self.process_open(state, key, evidence, emit)
            }
            HypothesisInput::Evidence { key, evidence } => {
                self.process_evidence(state, key, evidence, emit)
            }
            HypothesisInput::Advance { to } => self.process_advance(state, to, emit),
            HypothesisInput::Close {
                key,
                sequence,
                available_at,
                reason,
            } => {
                match self.terminalize(state, key.clone(), sequence, available_at, reason, false) {
                    Ok(events) => {
                        for event in events {
                            emit.emit(event);
                        }
                    }
                    Err(error) => self.reject(Some(key), error, emit),
                }
            }
            HypothesisInput::Retract {
                key,
                sequence,
                available_at,
                reason,
            } => match self.terminalize(state, key.clone(), sequence, available_at, reason, true) {
                Ok(events) => {
                    for event in events {
                        emit.emit(event);
                    }
                }
                Err(error) => self.reject(Some(key), error, emit),
            },
            HypothesisInput::Supersede {
                key,
                by,
                sequence,
                evidence,
                reason,
            } => self.process_supersede(
                state,
                SupersedeCommand {
                    key,
                    by,
                    sequence,
                    evidence,
                    reason,
                },
                emit,
            ),
        }
    }
}

impl<K, Model, Reason> FlushableScan for KeyedHypothesisMachine<K, Model, Reason>
where
    K: Clone + Ord,
    Model: HypothesisModel<K>,
    Model::State: Clone,
    Reason: Clone,
{
    type Offset = AvailableAt;

    fn flush<E>(&self, state: &mut Self::State, signal: FlushReason<Self::Offset>, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        if let FlushReason::Watermark(to) = signal {
            self.process_advance(state, to, emit);
        }
    }
}

impl<K, Model, Reason> SnapshottingScan for KeyedHypothesisMachine<K, Model, Reason>
where
    K: Clone + Ord + Serialize + DeserializeOwned,
    Model: HypothesisModel<K>,
    Model::State: Clone + Serialize + DeserializeOwned,
    Reason: Clone + Serialize + DeserializeOwned,
{
    type Snapshot = HypothesisSnapshot<K, Model::State, Reason>;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        self.snapshot_from_state(state)
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        self.state_from_snapshot(snapshot)
    }
}

impl<K, Model, Reason> FallibleRestoreScan for KeyedHypothesisMachine<K, Model, Reason>
where
    K: Clone + Ord + Serialize + DeserializeOwned,
    Model: HypothesisModel<K>,
    Model::State: Clone + Serialize + DeserializeOwned,
    Reason: Clone + Serialize + DeserializeOwned,
{
    type RestoreError = HypothesisRestoreError<K, Model::Error>;

    fn try_restore(&self, snapshot: Self::Snapshot) -> Result<Self::State, Self::RestoreError> {
        self.validate_snapshot(&snapshot)?;
        Ok(self.state_from_snapshot(snapshot))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use helio_scan::{FallibleRestoreScan, Scan, SnapshottingScan};
    use serde::{Deserialize, Serialize};

    use super::*;
    use crate::HypothesisTransition;

    const TIMEOUT: TimerId = TimerId(1);
    const FOLLOW_UP: TimerId = TimerId(2);

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    enum Evidence {
        Trigger { probability: f64 },
        Geometry { earthward: f64 },
        Severity { probability: f64 },
        Fail,
        InvalidCompletion,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    enum Stage {
        Triggered,
        Propagating,
        Impact,
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct ShockState {
        stage: Stage,
        probability: f64,
    }

    #[derive(Debug, Clone, PartialEq)]
    enum Output {
        Posterior { stage: Stage, probability: f64 },
        RequestImpactModel,
        TimedOut,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum ModelError {
        UnexpectedEvidence,
        InvalidProbability,
        DeliberateFailure,
    }

    #[derive(Debug, Clone, Copy)]
    struct ShockModel;

    impl ShockModel {
        fn probability(value: f64) -> Result<f64, ModelError> {
            if value.is_finite() && (0.0..=1.0).contains(&value) {
                Ok(value)
            } else {
                Err(ModelError::InvalidProbability)
            }
        }
    }

    impl HypothesisModel<String> for ShockModel {
        type Evidence = Evidence;
        type State = ShockState;
        type Output = Output;
        type Error = ModelError;

        fn open(
            &self,
            _key: &String,
            evidence: CausalEvidence<Self::Evidence>,
        ) -> Result<HypothesisTransition<Self::State, Self::Output>, Self::Error> {
            let Evidence::Trigger { probability } = evidence.payload else {
                return Err(ModelError::UnexpectedEvidence);
            };
            let probability = Self::probability(probability)?;
            Ok(HypothesisTransition::new(ShockState {
                stage: Stage::Triggered,
                probability,
            })
            .emit(Output::Posterior {
                stage: Stage::Triggered,
                probability,
            })
            .schedule(TIMEOUT, AvailableAt(evidence.available_at.0 + 10)))
        }

        fn update(
            &self,
            _key: &String,
            state: &Self::State,
            evidence: CausalEvidence<Self::Evidence>,
        ) -> Result<HypothesisTransition<Self::State, Self::Output>, Self::Error> {
            let mut state = state.clone();
            match (state.stage, evidence.payload) {
                (Stage::Triggered, Evidence::Geometry { earthward }) => {
                    let earthward = Self::probability(earthward)?;
                    state.stage = Stage::Propagating;
                    state.probability *= earthward;
                    let probability = state.probability;
                    Ok(HypothesisTransition::new(state)
                        .cancel(TIMEOUT)
                        .emit(Output::Posterior {
                            stage: Stage::Propagating,
                            probability,
                        })
                        .emit(Output::RequestImpactModel)
                        .schedule(FOLLOW_UP, AvailableAt(evidence.available_at.0 + 10)))
                }
                (Stage::Propagating, Evidence::Severity { probability }) => {
                    let probability = Self::probability(probability)?;
                    state.stage = Stage::Impact;
                    state.probability *= probability;
                    let probability = state.probability;
                    Ok(HypothesisTransition::new(state)
                        .emit(Output::Posterior {
                            stage: Stage::Impact,
                            probability,
                        })
                        .complete())
                }
                (_, Evidence::Fail) => Err(ModelError::DeliberateFailure),
                (_, Evidence::InvalidCompletion) => Ok(HypothesisTransition::new(state)
                    .schedule(TIMEOUT, AvailableAt(evidence.available_at.0 + 1))
                    .complete()),
                _ => Err(ModelError::UnexpectedEvidence),
            }
        }

        fn on_timer(
            &self,
            _key: &String,
            state: &Self::State,
            _timer_id: TimerId,
            _at: AvailableAt,
        ) -> Result<HypothesisTransition<Self::State, Self::Output>, Self::Error> {
            Ok(HypothesisTransition::new(state.clone())
                .emit(Output::TimedOut)
                .complete())
        }

        fn validate(&self, _key: &String, state: &Self::State) -> Result<(), Self::Error> {
            Self::probability(state.probability).map(|_| ())
        }
    }

    type Machine = KeyedHypothesisMachine<String, ShockModel, String>;
    type Event = HypothesisEvent<String, Output, String, ModelError>;

    #[derive(Debug)]
    struct CloneTrackedState {
        value: u64,
        clones: Arc<AtomicUsize>,
    }

    impl Clone for CloneTrackedState {
        fn clone(&self) -> Self {
            self.clones.fetch_add(1, Ordering::Relaxed);
            Self {
                value: self.value,
                clones: Arc::clone(&self.clones),
            }
        }
    }

    struct BorrowingModel {
        clones: Arc<AtomicUsize>,
    }

    impl HypothesisModel<u64> for BorrowingModel {
        type Evidence = u64;
        type State = CloneTrackedState;
        type Output = ();
        type Error = std::convert::Infallible;

        fn open(
            &self,
            _key: &u64,
            evidence: CausalEvidence<Self::Evidence>,
        ) -> Result<HypothesisTransition<Self::State, Self::Output>, Self::Error> {
            Ok(HypothesisTransition::new(CloneTrackedState {
                value: evidence.payload,
                clones: Arc::clone(&self.clones),
            }))
        }

        fn update(
            &self,
            _key: &u64,
            state: &Self::State,
            evidence: CausalEvidence<Self::Evidence>,
        ) -> Result<HypothesisTransition<Self::State, Self::Output>, Self::Error> {
            Ok(HypothesisTransition::new(CloneTrackedState {
                value: state.value + evidence.payload,
                clones: Arc::clone(&state.clones),
            }))
        }

        fn on_timer(
            &self,
            _key: &u64,
            state: &Self::State,
            _timer_id: TimerId,
            _at: AvailableAt,
        ) -> Result<HypothesisTransition<Self::State, Self::Output>, Self::Error> {
            Ok(HypothesisTransition::new(CloneTrackedState {
                value: state.value,
                clones: Arc::clone(&state.clones),
            }))
        }

        fn validate(&self, _key: &u64, _state: &Self::State) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    fn machine_with(max_active: usize, max_terminal: usize, timer_limit: usize) -> Machine {
        KeyedHypothesisMachine::try_new(
            ShockModel,
            HypothesisConfig::try_new(max_active, max_terminal, 4, 8, timer_limit).unwrap(),
        )
        .unwrap()
    }

    fn machine() -> Machine {
        machine_with(8, 8, 32)
    }

    fn evidence(sequence: u64, available_at: i64, payload: Evidence) -> CausalEvidence<Evidence> {
        CausalEvidence::new(
            sequence,
            EffectiveAt(available_at - 5),
            AvailableAt(available_at),
            payload,
        )
    }

    fn step(
        machine: &Machine,
        state: &mut <Machine as Scan>::State,
        input: HypothesisInput<String, Evidence, String>,
    ) -> Vec<Event> {
        machine.step_collect(state, input)
    }

    #[test]
    fn interleaved_hypotheses_keep_independent_conditional_state() {
        let machine = machine();
        let mut state = machine.init();
        step(
            &machine,
            &mut state,
            HypothesisInput::Open {
                key: "shock-a".into(),
                evidence: evidence(0, 10, Evidence::Trigger { probability: 0.8 }),
            },
        );
        step(
            &machine,
            &mut state,
            HypothesisInput::Open {
                key: "shock-b".into(),
                evidence: evidence(0, 11, Evidence::Trigger { probability: 0.4 }),
            },
        );
        let events = step(
            &machine,
            &mut state,
            HypothesisInput::Evidence {
                key: "shock-a".into(),
                evidence: evidence(1, 12, Evidence::Geometry { earthward: 0.5 }),
            },
        );

        let a = state.get(&"shock-a".to_string()).unwrap();
        let b = state.get(&"shock-b".to_string()).unwrap();
        assert_eq!(a.model_state.stage, Stage::Propagating);
        assert_eq!(a.model_state.probability, 0.4);
        assert_eq!(a.revision, 2);
        assert_eq!(b.model_state.stage, Stage::Triggered);
        assert_eq!(b.model_state.probability, 0.4);
        assert!(events.iter().any(|event| matches!(
            event,
            HypothesisEvent::ModelOutput {
                output: Output::RequestImpactModel,
                ..
            }
        )));
    }

    #[test]
    fn evidence_updates_do_not_clone_model_state_inside_the_runtime() {
        let clones = Arc::new(AtomicUsize::new(0));
        let machine = KeyedHypothesisMachine::<u64, _, ()>::try_new(
            BorrowingModel {
                clones: Arc::clone(&clones),
            },
            HypothesisConfig::try_new(1, 0, 0, 0, 1).unwrap(),
        )
        .unwrap();
        let mut state = machine.init();
        machine.step_collect(
            &mut state,
            HypothesisInput::Open {
                key: 7,
                evidence: CausalEvidence::new(0, EffectiveAt(1), AvailableAt(1), 11),
            },
        );
        machine.step_collect(
            &mut state,
            HypothesisInput::Evidence {
                key: 7,
                evidence: CausalEvidence::new(1, EffectiveAt(2), AvailableAt(2), 5),
            },
        );

        assert_eq!(state.get(&7).unwrap().model_state.value, 16);
        assert_eq!(clones.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn sequence_and_causal_frontier_rejections_are_atomic() {
        let machine = machine();
        let mut state = machine.init();
        step(
            &machine,
            &mut state,
            HypothesisInput::Open {
                key: "shock".into(),
                evidence: evidence(0, 10, Evidence::Trigger { probability: 0.8 }),
            },
        );
        let before_gap = state.clone();
        let gap = step(
            &machine,
            &mut state,
            HypothesisInput::Evidence {
                key: "shock".into(),
                evidence: evidence(2, 11, Evidence::Geometry { earthward: 0.5 }),
            },
        );
        assert_eq!(state, before_gap);
        assert!(matches!(
            gap.as_slice(),
            [HypothesisEvent::Rejected {
                error: HypothesisRejection::SequenceMismatch {
                    expected: 1,
                    found: 2
                },
                ..
            }]
        ));

        step(
            &machine,
            &mut state,
            HypothesisInput::Advance {
                to: AvailableAt(15),
            },
        );
        let before_late = state.clone();
        let late = step(
            &machine,
            &mut state,
            HypothesisInput::Evidence {
                key: "shock".into(),
                evidence: evidence(1, 15, Evidence::Geometry { earthward: 0.5 }),
            },
        );
        assert_eq!(state, before_late);
        assert!(matches!(
            late.as_slice(),
            [HypothesisEvent::Rejected {
                error: HypothesisRejection::AvailabilityAtOrBeforeFrontier { .. },
                ..
            }]
        ));
    }

    #[test]
    fn open_requires_zero_sequence_and_overdue_timers_block_later_input() {
        let machine = machine();
        let mut state = machine.init();
        let invalid_open = step(
            &machine,
            &mut state,
            HypothesisInput::Open {
                key: "bad-sequence".into(),
                evidence: evidence(4, 10, Evidence::Trigger { probability: 0.8 }),
            },
        );
        assert!(matches!(
            invalid_open.as_slice(),
            [HypothesisEvent::Rejected {
                error: HypothesisRejection::InitialSequenceMismatch {
                    expected: 0,
                    found: 4
                },
                ..
            }]
        ));

        step(
            &machine,
            &mut state,
            HypothesisInput::Open {
                key: "first".into(),
                evidence: evidence(0, 10, Evidence::Trigger { probability: 0.8 }),
            },
        );
        assert_eq!(state.next_timer_at(), Some(AvailableAt(20)));
        let before = state.clone();
        let blocked = step(
            &machine,
            &mut state,
            HypothesisInput::Open {
                key: "second".into(),
                evidence: evidence(0, 21, Evidence::Trigger { probability: 0.4 }),
            },
        );
        assert_eq!(state, before);
        assert!(matches!(
            blocked.as_slice(),
            [HypothesisEvent::Rejected {
                error: HypothesisRejection::OverdueTimer {
                    scheduled_at: AvailableAt(20),
                    input_available_at: AvailableAt(21),
                    ..
                },
                ..
            }]
        ));

        let fired = step(
            &machine,
            &mut state,
            HypothesisInput::Advance {
                to: AvailableAt(20),
            },
        );
        assert!(fired
            .iter()
            .any(|event| matches!(event, HypothesisEvent::TimerFired { .. })));
    }

    #[test]
    fn due_timers_fire_by_time_then_key_and_complete_hypotheses() {
        let machine = machine();
        let mut state = machine.init();
        for key in ["z-shock", "a-shock"] {
            step(
                &machine,
                &mut state,
                HypothesisInput::Open {
                    key: key.into(),
                    evidence: evidence(0, 10, Evidence::Trigger { probability: 0.5 }),
                },
            );
        }
        let events = step(
            &machine,
            &mut state,
            HypothesisInput::Advance {
                to: AvailableAt(20),
            },
        );
        let fired_keys: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                HypothesisEvent::TimerFired { key, .. } => Some(key.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(fired_keys, ["a-shock", "z-shock"]);
        assert_eq!(state.active_count(), 0);
        assert_eq!(state.terminal_count(), 2);
        assert!(matches!(
            events.last(),
            Some(HypothesisEvent::FrontierAdvanced {
                timers_fired: 2,
                ..
            })
        ));
    }

    #[test]
    fn timer_fire_limit_rolls_back_the_entire_frontier_advance() {
        let machine = machine_with(4, 4, 1);
        let mut state = machine.init();
        for key in ["a", "b"] {
            step(
                &machine,
                &mut state,
                HypothesisInput::Open {
                    key: key.into(),
                    evidence: evidence(0, 10, Evidence::Trigger { probability: 0.5 }),
                },
            );
        }
        let before = state.clone();
        let events = step(
            &machine,
            &mut state,
            HypothesisInput::Advance {
                to: AvailableAt(20),
            },
        );
        assert_eq!(state, before);
        assert!(matches!(
            events.as_slice(),
            [HypothesisEvent::Rejected {
                error: HypothesisRejection::TimerFireLimitExceeded { limit: 1 },
                ..
            }]
        ));
    }

    #[test]
    fn model_and_effect_failures_do_not_mutate_live_state() {
        let machine = machine();
        let mut state = machine.init();
        step(
            &machine,
            &mut state,
            HypothesisInput::Open {
                key: "shock".into(),
                evidence: evidence(0, 10, Evidence::Trigger { probability: 0.8 }),
            },
        );
        for payload in [Evidence::Fail, Evidence::InvalidCompletion] {
            let before = state.clone();
            let events = step(
                &machine,
                &mut state,
                HypothesisInput::Evidence {
                    key: "shock".into(),
                    evidence: evidence(1, 11, payload),
                },
            );
            assert_eq!(state, before);
            assert!(matches!(
                events.as_slice(),
                [HypothesisEvent::Rejected { .. }]
            ));
        }
    }

    #[test]
    fn model_effect_batches_are_bounded_before_open_commits() {
        let machine = KeyedHypothesisMachine::try_new(
            ShockModel,
            HypothesisConfig::try_new(4, 4, 4, 1, 8).unwrap(),
        )
        .unwrap();
        let mut state = machine.init();
        let events = step(
            &machine,
            &mut state,
            HypothesisInput::Open {
                key: "shock".into(),
                evidence: evidence(0, 10, Evidence::Trigger { probability: 0.8 }),
            },
        );
        assert_eq!(state.active_count(), 0);
        assert!(matches!(
            events.as_slice(),
            [HypothesisEvent::Rejected {
                error: HypothesisRejection::EffectCapacityExceeded { capacity: 1 },
                ..
            }]
        ));
    }

    #[test]
    fn supersession_is_atomic_and_preserves_a_terminal_tombstone() {
        let machine = machine();
        let mut state = machine.init();
        step(
            &machine,
            &mut state,
            HypothesisInput::Open {
                key: "draft".into(),
                evidence: evidence(0, 10, Evidence::Trigger { probability: 0.7 }),
            },
        );
        let before = state.clone();
        let rejected = step(
            &machine,
            &mut state,
            HypothesisInput::Supersede {
                key: "draft".into(),
                by: "corrected".into(),
                sequence: 1,
                evidence: evidence(0, 11, Evidence::Fail),
                reason: "bad source geometry".into(),
            },
        );
        assert_eq!(state, before);
        assert!(matches!(
            rejected.as_slice(),
            [HypothesisEvent::Rejected {
                key: Some(key),
                error: HypothesisRejection::Model(ModelError::UnexpectedEvidence)
            }] if key == "corrected"
        ));

        let accepted = step(
            &machine,
            &mut state,
            HypothesisInput::Supersede {
                key: "draft".into(),
                by: "corrected".into(),
                sequence: 1,
                evidence: evidence(0, 11, Evidence::Trigger { probability: 0.9 }),
                reason: "corrected coordinates".into(),
            },
        );
        assert!(state.active.contains_key("corrected"));
        assert!(state.terminal.contains_key("draft"));
        assert!(!state.active.contains_key("draft"));
        assert!(matches!(
            accepted.first(),
            Some(HypothesisEvent::Superseded { .. })
        ));
        assert!(matches!(
            accepted.get(1),
            Some(HypothesisEvent::Opened { key, .. }) if key == "corrected"
        ));
    }

    #[test]
    fn active_and_terminal_memory_are_bounded() {
        let machine = machine_with(1, 1, 8);
        let mut state = machine.init();
        step(
            &machine,
            &mut state,
            HypothesisInput::Open {
                key: "a".into(),
                evidence: evidence(0, 10, Evidence::Trigger { probability: 0.5 }),
            },
        );
        let full = step(
            &machine,
            &mut state,
            HypothesisInput::Open {
                key: "b".into(),
                evidence: evidence(0, 11, Evidence::Trigger { probability: 0.5 }),
            },
        );
        assert!(matches!(
            full.as_slice(),
            [HypothesisEvent::Rejected {
                error: HypothesisRejection::ActiveCapacityExceeded { capacity: 1 },
                ..
            }]
        ));
        step(
            &machine,
            &mut state,
            HypothesisInput::Close {
                key: "a".into(),
                sequence: 1,
                available_at: AvailableAt(11),
                reason: "resolved".into(),
            },
        );
        step(
            &machine,
            &mut state,
            HypothesisInput::Open {
                key: "b".into(),
                evidence: evidence(0, 12, Evidence::Trigger { probability: 0.5 }),
            },
        );
        let closed = step(
            &machine,
            &mut state,
            HypothesisInput::Close {
                key: "b".into(),
                sequence: 1,
                available_at: AvailableAt(13),
                reason: "resolved".into(),
            },
        );
        assert_eq!(state.terminal_count(), 1);
        assert!(state.terminal.contains_key("b"));
        assert!(closed.iter().any(|event| matches!(
            event,
            HypothesisEvent::TerminalEvicted { key } if key == "a"
        )));
    }

    #[test]
    fn checkpoint_restore_preserves_future_outputs_exactly() {
        let machine = machine();
        let mut state = machine.init();
        step(
            &machine,
            &mut state,
            HypothesisInput::Open {
                key: "shock".into(),
                evidence: evidence(0, 10, Evidence::Trigger { probability: 0.8 }),
            },
        );
        step(
            &machine,
            &mut state,
            HypothesisInput::Evidence {
                key: "shock".into(),
                evidence: evidence(1, 11, Evidence::Geometry { earthward: 0.5 }),
            },
        );

        let bytes = serde_json::to_vec(&machine.snapshot(&state)).unwrap();
        let snapshot: HypothesisSnapshot<String, ShockState, String> =
            serde_json::from_slice(&bytes).unwrap();
        let mut restored = machine.try_restore(snapshot).unwrap();
        assert_eq!(state, restored);

        let input = HypothesisInput::Evidence {
            key: "shock".into(),
            evidence: evidence(2, 20, Evidence::Severity { probability: 0.25 }),
        };
        let continuous_events = step(&machine, &mut state, input.clone());
        let restored_events = step(&machine, &mut restored, input);
        assert_eq!(continuous_events, restored_events);
        assert_eq!(state, restored);
    }

    #[test]
    fn corrupt_external_snapshot_is_rejected_before_resume() {
        let machine = machine();
        let mut state = machine.init();
        step(
            &machine,
            &mut state,
            HypothesisInput::Open {
                key: "shock".into(),
                evidence: evidence(0, 10, Evidence::Trigger { probability: 0.8 }),
            },
        );
        let mut snapshot = machine.snapshot(&state);
        snapshot.frontier = Some(AvailableAt(20));
        assert!(matches!(
            machine.try_restore(snapshot),
            Err(HypothesisRestoreError::TimerAtOrBeforeFrontier { .. })
        ));

        let mut snapshot = machine.snapshot(&state);
        snapshot
            .active
            .get_mut("shock")
            .unwrap()
            .model_state
            .probability = f64::NAN;
        assert!(matches!(
            machine.try_restore(snapshot),
            Err(HypothesisRestoreError::Model {
                error: ModelError::InvalidProbability,
                ..
            })
        ));
    }

    #[test]
    fn completed_chain_emits_posterior_then_terminal_event() {
        let machine = machine();
        let mut state = machine.init();
        step(
            &machine,
            &mut state,
            HypothesisInput::Open {
                key: "shock".into(),
                evidence: evidence(0, 10, Evidence::Trigger { probability: 0.8 }),
            },
        );
        step(
            &machine,
            &mut state,
            HypothesisInput::Evidence {
                key: "shock".into(),
                evidence: evidence(1, 11, Evidence::Geometry { earthward: 0.5 }),
            },
        );
        let events = step(
            &machine,
            &mut state,
            HypothesisInput::Evidence {
                key: "shock".into(),
                evidence: evidence(2, 12, Evidence::Severity { probability: 0.25 }),
            },
        );
        assert!(matches!(
            events.get(events.len() - 2),
            Some(HypothesisEvent::ModelOutput {
                output: Output::Posterior {
                    stage: Stage::Impact,
                    probability
                },
                ..
            }) if *probability == 0.1
        ));
        assert!(matches!(
            events.last(),
            Some(HypothesisEvent::Completed { .. })
        ));
        assert!(!state.active.contains_key("shock"));
        assert!(state.terminal.contains_key("shock"));
    }
}
