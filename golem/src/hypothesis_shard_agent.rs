use golem_rust::{Schema, agent_definition, agent_implementation};
use helio_golem::{
    DurableHypothesisShard, DurableShardSnapshot, OffsetInput, ShardConfigError, ShardIdentity,
    ShardProcessError,
};
use helio_hypothesis::{
    CausalEvidence, HypothesisConfig, HypothesisEvent, HypothesisInput, HypothesisModel,
    HypothesisTransition, KeyedHypothesisMachine, TimerId,
};
use helio_time::{AvailableAt, EffectiveAt};
use serde::{Deserialize, Serialize};

const MAX_BATCH_SIZE: u32 = 256;
const SNAPSHOT_FORMAT_VERSION: u32 = 1;
const PROBABILITY_SCALE: u32 = 1_000_000;
const DEADLINE_TIMER: TimerId = TimerId(1);

type ShockShard = DurableHypothesisShard<String, EventShockModel, String>;
type ShockSnapshot = DurableShardSnapshot<String, EventShockState, String>;

/// Wire-level evidence for a general event-shock hypothesis.
///
/// Probabilities use integer parts per million. That makes replay bit-exact across native and WASM
/// targets and avoids making floating-point edge behavior part of the durable state contract.
#[derive(Debug, Clone, PartialEq, Eq, Schema, Serialize, Deserialize)]
pub enum EventShockEvidence {
    Trigger {
        prior_ppm: u32,
        deadline_available_at: i64,
    },
    LikelihoodAssessment {
        observation_positive: bool,
        sensitivity_ppm: u32,
        false_positive_ppm: u32,
        deadline_available_at: i64,
    },
    MarketAssessment {
        expected_edge_bps: i32,
        confidence_ppm: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Schema, Serialize, Deserialize)]
pub struct EvidenceEnvelope {
    pub sequence: u64,
    pub effective_at: i64,
    pub available_at: i64,
    pub payload: EventShockEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Schema, Serialize, Deserialize)]
pub enum SourceMutation {
    Open {
        key: String,
        evidence: EvidenceEnvelope,
    },
    Evidence {
        key: String,
        evidence: EvidenceEnvelope,
    },
    Advance {
        to_available_at: i64,
    },
    Close {
        key: String,
        sequence: u64,
        available_at: i64,
        reason: String,
    },
    Retract {
        key: String,
        sequence: u64,
        available_at: i64,
        reason: String,
    },
    Supersede {
        key: String,
        by: String,
        sequence: u64,
        evidence: EvidenceEnvelope,
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Schema, Serialize, Deserialize)]
pub struct SourceRecord {
    pub offset: u64,
    pub mutation: SourceMutation,
}

#[derive(Debug, Clone, PartialEq, Eq, Schema, Serialize, Deserialize)]
pub struct SourceBatch {
    pub records: Vec<SourceRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Schema, Serialize, Deserialize)]
pub enum EventShockOutput {
    RequestLikelihoodAssessment {
        request_id: String,
    },
    RequestMarketAssessment {
        request_id: String,
        posterior_ppm: u32,
    },
    Candidate {
        signal_id: String,
        posterior_ppm: u32,
        expected_edge_bps: i32,
        confidence_ppm: u32,
        conviction_ppm: u32,
    },
    Expired {
        posterior_ppm: u32,
    },
}

/// Complete typed audit stream emitted by the generic hypothesis runtime.
#[derive(Debug, Clone, PartialEq, Eq, Schema, Serialize, Deserialize)]
pub enum AuditEvent {
    Opened {
        key: String,
        sequence: u64,
        effective_at: i64,
        available_at: i64,
        revision: u64,
    },
    EvidenceAccepted {
        key: String,
        sequence: u64,
        effective_at: i64,
        available_at: i64,
        revision: u64,
    },
    ModelOutput {
        key: String,
        revision: u64,
        output: EventShockOutput,
    },
    TimerScheduled {
        key: String,
        revision: u64,
        timer_id: u64,
        at: i64,
        replaced_at: Option<i64>,
    },
    TimerCancelled {
        key: String,
        revision: u64,
        timer_id: u64,
        scheduled_at: i64,
    },
    TimerFired {
        key: String,
        revision: u64,
        timer_id: u64,
        at: i64,
    },
    Completed {
        key: String,
        revision: u64,
        available_at: i64,
    },
    Closed {
        key: String,
        revision: u64,
        available_at: i64,
        reason: String,
    },
    Retracted {
        key: String,
        revision: u64,
        available_at: i64,
        reason: String,
    },
    Superseded {
        key: String,
        by: String,
        revision: u64,
        available_at: i64,
        reason: String,
    },
    TerminalEvicted {
        key: String,
    },
    FrontierAdvanced {
        previous: Option<i64>,
        current: i64,
        timers_fired: u64,
    },
    /// Defensive wire representation. The durable driver normally converts a rejection into an
    /// `AgentError` before committing its trial state, so a receipt should never contain this.
    RuntimeRejected {
        key: Option<String>,
        detail: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Schema, Serialize, Deserialize)]
pub struct ProcessReceipt {
    pub invocation_key: String,
    pub first_offset: u64,
    pub last_offset: u64,
    pub next_offset: u64,
    pub events: Vec<AuditEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Schema, Serialize, Deserialize)]
pub struct ShardStatus {
    pub strategy_fingerprint: String,
    pub source_id: String,
    pub source_partition: u64,
    pub logical_shard: u64,
    pub next_offset: u64,
    pub frontier_available_at: Option<i64>,
    pub next_deadline_available_at: Option<i64>,
    pub active_hypotheses: u64,
    pub terminal_hypotheses: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Schema, Serialize, Deserialize)]
pub enum AgentError {
    NotInitialized { detail: String },
    EmptyBatch,
    BatchCapacityExceeded { found: u64, capacity: u32 },
    OffsetOverlap { expected: u64, found: u64 },
    OffsetGap { expected: u64, found: u64 },
    OffsetExhausted { offset: u64 },
    TransitionRejected { offset: u64, detail: String },
}

#[agent_definition(snapshotting = "periodic(30s)")]
pub trait HypothesisShardAgent {
    /// Constructor parameters are the Golem agent identity and stable shard placement key.
    fn new(
        strategy_fingerprint: String,
        source_id: String,
        source_partition: u64,
        logical_shard: u64,
        initial_offset: u64,
    ) -> Self;

    fn process_batch(&mut self, batch: SourceBatch) -> Result<ProcessReceipt, AgentError>;

    fn status(&self) -> Result<ShardStatus, AgentError>;

    /// The caller should pass this exact value as `golem agent invoke --idempotency-key`.
    fn invocation_key(&self, first_offset: u64, last_offset: u64) -> Result<String, AgentError>;
}

struct HypothesisShardAgentImpl {
    strategy_fingerprint: String,
    source_id: String,
    source_partition: u64,
    logical_shard: u64,
    initial_offset: u64,
    shard: Option<ShockShard>,
    initialization_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct AgentSnapshot {
    format_version: u32,
    initial_offset: u64,
    shard: ShockSnapshot,
}

#[agent_implementation]
impl HypothesisShardAgent for HypothesisShardAgentImpl {
    fn new(
        strategy_fingerprint: String,
        source_id: String,
        source_partition: u64,
        logical_shard: u64,
        initial_offset: u64,
    ) -> Self {
        let identity = ShardIdentity::try_new(
            strategy_fingerprint.clone(),
            source_id.clone(),
            source_partition,
            logical_shard,
        )
        .map_err(config_error);
        let shard = identity.and_then(|identity| {
            DurableHypothesisShard::try_new(
                hypothesis_machine()?,
                identity,
                MAX_BATCH_SIZE,
                initial_offset,
            )
            .map_err(config_error)
        });
        let (shard, initialization_error) = match shard {
            Ok(shard) => (Some(shard), None),
            Err(error) => (None, Some(error)),
        };

        Self {
            strategy_fingerprint,
            source_id,
            source_partition,
            logical_shard,
            initial_offset,
            shard,
            initialization_error,
        }
    }

    fn process_batch(&mut self, batch: SourceBatch) -> Result<ProcessReceipt, AgentError> {
        let shard = self.shard_mut()?;
        let inputs = batch
            .records
            .into_iter()
            .map(|record| OffsetInput::new(record.offset, record.mutation.into()))
            .collect();
        let receipt = shard.process_batch(inputs).map_err(process_error)?;
        let invocation_key = shard
            .identity()
            .invocation_key(receipt.first_offset, receipt.last_offset);

        log::info!(
            "committed hypothesis source interval {}..={} next={}",
            receipt.first_offset,
            receipt.last_offset,
            receipt.next_offset
        );
        Ok(ProcessReceipt {
            invocation_key,
            first_offset: receipt.first_offset,
            last_offset: receipt.last_offset,
            next_offset: receipt.next_offset,
            events: receipt.events.into_iter().map(AuditEvent::from).collect(),
        })
    }

    fn status(&self) -> Result<ShardStatus, AgentError> {
        let shard = self.shard_ref()?;
        let state = shard.state();
        Ok(ShardStatus {
            strategy_fingerprint: shard.identity().strategy_fingerprint.clone(),
            source_id: shard.identity().source_id.clone(),
            source_partition: shard.identity().source_partition,
            logical_shard: shard.identity().logical_shard,
            next_offset: shard.next_offset(),
            frontier_available_at: state.frontier().map(|at| at.0),
            next_deadline_available_at: state.next_timer_at().map(|at| at.0),
            active_hypotheses: state.active_count() as u64,
            terminal_hypotheses: state.terminal_count() as u64,
        })
    }

    fn invocation_key(&self, first_offset: u64, last_offset: u64) -> Result<String, AgentError> {
        Ok(self
            .shard_ref()?
            .identity()
            .invocation_key(first_offset, last_offset))
    }

    async fn save_snapshot(&self) -> Result<Vec<u8>, String> {
        self.snapshot_bytes()
    }

    async fn load_snapshot(&mut self, bytes: Vec<u8>) -> Result<(), String> {
        self.restore_snapshot_bytes(bytes)
    }
}

impl HypothesisShardAgentImpl {
    fn shard_ref(&self) -> Result<&ShockShard, AgentError> {
        self.shard
            .as_ref()
            .ok_or_else(|| AgentError::NotInitialized {
                detail: self
                    .initialization_error
                    .clone()
                    .unwrap_or_else(|| "durable shard state is unavailable".to_string()),
            })
    }

    fn shard_mut(&mut self) -> Result<&mut ShockShard, AgentError> {
        let detail = self
            .initialization_error
            .clone()
            .unwrap_or_else(|| "durable shard state is unavailable".to_string());
        self.shard
            .as_mut()
            .ok_or(AgentError::NotInitialized { detail })
    }

    fn snapshot_bytes(&self) -> Result<Vec<u8>, String> {
        let shard = self.shard_ref().map_err(|error| format!("{error:?}"))?;
        serde_json::to_vec(&AgentSnapshot {
            format_version: SNAPSHOT_FORMAT_VERSION,
            initial_offset: self.initial_offset,
            shard: shard.snapshot(),
        })
        .map_err(|error| format!("failed to encode hypothesis shard snapshot: {error}"))
    }

    fn restore_snapshot_bytes(&mut self, bytes: Vec<u8>) -> Result<(), String> {
        let snapshot: AgentSnapshot = serde_json::from_slice(&bytes)
            .map_err(|error| format!("failed to decode hypothesis shard snapshot: {error}"))?;
        if snapshot.format_version != SNAPSHOT_FORMAT_VERSION {
            return Err(format!(
                "unsupported hypothesis shard snapshot version {}; expected {}",
                snapshot.format_version, SNAPSHOT_FORMAT_VERSION
            ));
        }
        if snapshot.initial_offset != self.initial_offset {
            return Err(format!(
                "snapshot initial offset {} does not match agent constructor {}",
                snapshot.initial_offset, self.initial_offset
            ));
        }

        let identity = ShardIdentity::try_new(
            self.strategy_fingerprint.clone(),
            self.source_id.clone(),
            self.source_partition,
            self.logical_shard,
        )
        .map_err(config_error)?;
        let restored = DurableHypothesisShard::try_from_snapshot(
            hypothesis_machine()?,
            identity,
            MAX_BATCH_SIZE,
            snapshot.shard,
        )
        .map_err(|error| format!("invalid hypothesis shard snapshot: {error}"))?;

        self.shard = Some(restored);
        self.initialization_error = None;
        Ok(())
    }
}

fn hypothesis_machine() -> Result<KeyedHypothesisMachine<String, EventShockModel, String>, String> {
    let config =
        HypothesisConfig::try_new(4_096, 4_096, 1, 3, 1_024).map_err(|error| error.to_string())?;
    KeyedHypothesisMachine::try_new(EventShockModel, config).map_err(|error| error.to_string())
}

fn config_error(error: ShardConfigError) -> String {
    error.to_string()
}

fn process_error(error: ShardProcessError<ModelError>) -> AgentError {
    match error {
        ShardProcessError::EmptyBatch => AgentError::EmptyBatch,
        ShardProcessError::BatchCapacityExceeded { found, capacity } => {
            AgentError::BatchCapacityExceeded {
                found: found as u64,
                capacity,
            }
        }
        ShardProcessError::OffsetOverlap { expected, found } => {
            AgentError::OffsetOverlap { expected, found }
        }
        ShardProcessError::OffsetGap { expected, found } => {
            AgentError::OffsetGap { expected, found }
        }
        ShardProcessError::OffsetExhausted { offset } => AgentError::OffsetExhausted { offset },
        ShardProcessError::TransitionRejected { offset, rejection } => {
            AgentError::TransitionRejected {
                offset,
                detail: format!("{rejection:?}"),
            }
        }
    }
}

impl From<EvidenceEnvelope> for CausalEvidence<EventShockEvidence> {
    fn from(value: EvidenceEnvelope) -> Self {
        Self::new(
            value.sequence,
            EffectiveAt(value.effective_at),
            AvailableAt(value.available_at),
            value.payload,
        )
    }
}

impl From<SourceMutation> for HypothesisInput<String, EventShockEvidence, String> {
    fn from(value: SourceMutation) -> Self {
        match value {
            SourceMutation::Open { key, evidence } => Self::Open {
                key,
                evidence: evidence.into(),
            },
            SourceMutation::Evidence { key, evidence } => Self::Evidence {
                key,
                evidence: evidence.into(),
            },
            SourceMutation::Advance { to_available_at } => Self::Advance {
                to: AvailableAt(to_available_at),
            },
            SourceMutation::Close {
                key,
                sequence,
                available_at,
                reason,
            } => Self::Close {
                key,
                sequence,
                available_at: AvailableAt(available_at),
                reason,
            },
            SourceMutation::Retract {
                key,
                sequence,
                available_at,
                reason,
            } => Self::Retract {
                key,
                sequence,
                available_at: AvailableAt(available_at),
                reason,
            },
            SourceMutation::Supersede {
                key,
                by,
                sequence,
                evidence,
                reason,
            } => Self::Supersede {
                key,
                by,
                sequence,
                evidence: evidence.into(),
                reason,
            },
        }
    }
}

impl From<HypothesisEvent<String, EventShockOutput, String, ModelError>> for AuditEvent {
    fn from(value: HypothesisEvent<String, EventShockOutput, String, ModelError>) -> Self {
        match value {
            HypothesisEvent::Opened {
                key,
                sequence,
                effective_at,
                available_at,
                revision,
            } => Self::Opened {
                key,
                sequence,
                effective_at: effective_at.0,
                available_at: available_at.0,
                revision,
            },
            HypothesisEvent::EvidenceAccepted {
                key,
                sequence,
                effective_at,
                available_at,
                revision,
            } => Self::EvidenceAccepted {
                key,
                sequence,
                effective_at: effective_at.0,
                available_at: available_at.0,
                revision,
            },
            HypothesisEvent::ModelOutput {
                key,
                revision,
                output,
            } => Self::ModelOutput {
                key,
                revision,
                output,
            },
            HypothesisEvent::TimerScheduled {
                key,
                revision,
                timer_id,
                at,
                replaced,
            } => Self::TimerScheduled {
                key,
                revision,
                timer_id: timer_id.0,
                at: at.0,
                replaced_at: replaced.map(|time| time.0),
            },
            HypothesisEvent::TimerCancelled {
                key,
                revision,
                timer_id,
                scheduled_at,
            } => Self::TimerCancelled {
                key,
                revision,
                timer_id: timer_id.0,
                scheduled_at: scheduled_at.0,
            },
            HypothesisEvent::TimerFired {
                key,
                revision,
                timer_id,
                at,
            } => Self::TimerFired {
                key,
                revision,
                timer_id: timer_id.0,
                at: at.0,
            },
            HypothesisEvent::Completed {
                key,
                revision,
                available_at,
            } => Self::Completed {
                key,
                revision,
                available_at: available_at.0,
            },
            HypothesisEvent::Closed {
                key,
                revision,
                available_at,
                reason,
            } => Self::Closed {
                key,
                revision,
                available_at: available_at.0,
                reason,
            },
            HypothesisEvent::Retracted {
                key,
                revision,
                available_at,
                reason,
            } => Self::Retracted {
                key,
                revision,
                available_at: available_at.0,
                reason,
            },
            HypothesisEvent::Superseded {
                key,
                by,
                revision,
                available_at,
                reason,
            } => Self::Superseded {
                key,
                by,
                revision,
                available_at: available_at.0,
                reason,
            },
            HypothesisEvent::TerminalEvicted { key } => Self::TerminalEvicted { key },
            HypothesisEvent::FrontierAdvanced {
                previous,
                current,
                timers_fired,
            } => Self::FrontierAdvanced {
                previous: previous.map(|time| time.0),
                current: current.0,
                timers_fired: timers_fired as u64,
            },
            HypothesisEvent::Rejected { key, error } => Self::RuntimeRejected {
                key,
                detail: format!("{error:?}"),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum EventShockStage {
    Triggered,
    LikelihoodAssessed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct EventShockState {
    stage: EventShockStage,
    posterior_ppm: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum ModelError {
    UnexpectedEvidence,
    ProbabilityOutOfRange,
    DegenerateLikelihood,
    DeadlineNotAfterEvidence,
}

#[derive(Debug, Clone, Copy)]
struct EventShockModel;

impl HypothesisModel<String> for EventShockModel {
    type Evidence = EventShockEvidence;
    type State = EventShockState;
    type Output = EventShockOutput;
    type Error = ModelError;

    fn open(
        &self,
        key: &String,
        evidence: CausalEvidence<Self::Evidence>,
    ) -> Result<HypothesisTransition<Self::State, Self::Output>, Self::Error> {
        let EventShockEvidence::Trigger {
            prior_ppm,
            deadline_available_at,
        } = evidence.payload
        else {
            return Err(ModelError::UnexpectedEvidence);
        };
        validate_probability(prior_ppm)?;
        validate_deadline(evidence.available_at, deadline_available_at)?;

        Ok(HypothesisTransition::new(EventShockState {
            stage: EventShockStage::Triggered,
            posterior_ppm: prior_ppm,
        })
        .emit(EventShockOutput::RequestLikelihoodAssessment {
            request_id: request_id(key, "likelihood", evidence.sequence),
        })
        .schedule(DEADLINE_TIMER, AvailableAt(deadline_available_at)))
    }

    fn update(
        &self,
        key: &String,
        state: &Self::State,
        evidence: CausalEvidence<Self::Evidence>,
    ) -> Result<HypothesisTransition<Self::State, Self::Output>, Self::Error> {
        match (state.stage, evidence.payload) {
            (
                EventShockStage::Triggered,
                EventShockEvidence::LikelihoodAssessment {
                    observation_positive,
                    sensitivity_ppm,
                    false_positive_ppm,
                    deadline_available_at,
                },
            ) => {
                validate_probability(sensitivity_ppm)?;
                validate_probability(false_positive_ppm)?;
                validate_deadline(evidence.available_at, deadline_available_at)?;
                let posterior_ppm = bayesian_update(
                    state.posterior_ppm,
                    observation_positive,
                    sensitivity_ppm,
                    false_positive_ppm,
                )?;
                Ok(HypothesisTransition::new(EventShockState {
                    stage: EventShockStage::LikelihoodAssessed,
                    posterior_ppm,
                })
                .emit(EventShockOutput::RequestMarketAssessment {
                    request_id: request_id(key, "market", evidence.sequence),
                    posterior_ppm,
                })
                .schedule(DEADLINE_TIMER, AvailableAt(deadline_available_at)))
            }
            (
                EventShockStage::LikelihoodAssessed,
                EventShockEvidence::MarketAssessment {
                    expected_edge_bps,
                    confidence_ppm,
                },
            ) => {
                validate_probability(confidence_ppm)?;
                let conviction_ppm = multiply_ppm(state.posterior_ppm, confidence_ppm);
                Ok(HypothesisTransition::new(state.clone())
                    .emit(EventShockOutput::Candidate {
                        signal_id: request_id(key, "candidate", evidence.sequence),
                        posterior_ppm: state.posterior_ppm,
                        expected_edge_bps,
                        confidence_ppm,
                        conviction_ppm,
                    })
                    .complete())
            }
            _ => Err(ModelError::UnexpectedEvidence),
        }
    }

    fn on_timer(
        &self,
        _key: &String,
        state: &Self::State,
        timer_id: TimerId,
        _at: AvailableAt,
    ) -> Result<HypothesisTransition<Self::State, Self::Output>, Self::Error> {
        if timer_id != DEADLINE_TIMER {
            return Err(ModelError::UnexpectedEvidence);
        }
        Ok(HypothesisTransition::new(state.clone())
            .emit(EventShockOutput::Expired {
                posterior_ppm: state.posterior_ppm,
            })
            .complete())
    }

    fn validate(&self, _key: &String, state: &Self::State) -> Result<(), Self::Error> {
        validate_probability(state.posterior_ppm)
    }
}

fn validate_probability(value: u32) -> Result<(), ModelError> {
    if value <= PROBABILITY_SCALE {
        Ok(())
    } else {
        Err(ModelError::ProbabilityOutOfRange)
    }
}

fn validate_deadline(available_at: AvailableAt, deadline: i64) -> Result<(), ModelError> {
    if deadline > available_at.0 {
        Ok(())
    } else {
        Err(ModelError::DeadlineNotAfterEvidence)
    }
}

fn bayesian_update(
    prior_ppm: u32,
    observation_positive: bool,
    sensitivity_ppm: u32,
    false_positive_ppm: u32,
) -> Result<u32, ModelError> {
    let scale = u128::from(PROBABILITY_SCALE);
    let prior = u128::from(prior_ppm);
    let (hit, false_alarm) = if observation_positive {
        (u128::from(sensitivity_ppm), u128::from(false_positive_ppm))
    } else {
        (
            scale - u128::from(sensitivity_ppm),
            scale - u128::from(false_positive_ppm),
        )
    };
    let supported = prior * hit;
    let alternative = (scale - prior) * false_alarm;
    let denominator = supported + alternative;
    if denominator == 0 {
        return Err(ModelError::DegenerateLikelihood);
    }
    let rounded = (supported * scale + denominator / 2) / denominator;
    u32::try_from(rounded).map_err(|_| ModelError::ProbabilityOutOfRange)
}

fn multiply_ppm(left: u32, right: u32) -> u32 {
    let scale = u64::from(PROBABILITY_SCALE);
    let product = u64::from(left) * u64::from(right);
    ((product + scale / 2) / scale) as u32
}

fn request_id(key: &str, kind: &str, sequence: u64) -> String {
    format!("v1/{}:{key}/{kind}/{sequence}", key.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(initial_offset: u64) -> HypothesisShardAgentImpl {
        HypothesisShardAgentImpl::new(
            "strategy-v1".to_string(),
            "normalized-events".to_string(),
            0,
            17,
            initial_offset,
        )
    }

    fn envelope(sequence: u64, available_at: i64, payload: EventShockEvidence) -> EvidenceEnvelope {
        EvidenceEnvelope {
            sequence,
            effective_at: available_at - 1,
            available_at,
            payload,
        }
    }

    fn open(offset: u64) -> SourceRecord {
        SourceRecord {
            offset,
            mutation: SourceMutation::Open {
                key: "solar-flare".to_string(),
                evidence: envelope(
                    0,
                    10,
                    EventShockEvidence::Trigger {
                        prior_ppm: 1_000,
                        deadline_available_at: 100,
                    },
                ),
            },
        }
    }

    fn likelihood(offset: u64) -> SourceRecord {
        SourceRecord {
            offset,
            mutation: SourceMutation::Evidence {
                key: "solar-flare".to_string(),
                evidence: envelope(
                    1,
                    20,
                    EventShockEvidence::LikelihoodAssessment {
                        observation_positive: true,
                        sensitivity_ppm: 950_000,
                        false_positive_ppm: 1_000,
                        deadline_available_at: 200,
                    },
                ),
            },
        }
    }

    fn market(offset: u64) -> SourceRecord {
        SourceRecord {
            offset,
            mutation: SourceMutation::Evidence {
                key: "solar-flare".to_string(),
                evidence: envelope(
                    2,
                    30,
                    EventShockEvidence::MarketAssessment {
                        expected_edge_bps: 180,
                        confidence_ppm: 800_000,
                    },
                ),
            },
        }
    }

    #[test]
    fn bayesian_chain_emits_candidate_and_completes() {
        let mut agent = agent(40);
        let receipt = agent
            .process_batch(SourceBatch {
                records: vec![open(40), likelihood(41), market(42)],
            })
            .unwrap();

        assert_eq!(receipt.next_offset, 43);
        assert!(receipt.events.iter().any(|event| matches!(
            event,
            AuditEvent::ModelOutput {
                output: EventShockOutput::Candidate {
                    posterior_ppm: 487_429,
                    conviction_ppm: 389_943,
                    ..
                },
                ..
            }
        )));
        let status = agent.status().unwrap();
        assert_eq!(status.active_hypotheses, 0);
        assert_eq!(status.terminal_hypotheses, 1);
    }

    #[test]
    fn one_bad_record_rolls_back_the_whole_source_batch() {
        let mut agent = agent(40);
        let bad_market = SourceRecord {
            offset: 41,
            mutation: market(41).mutation,
        };
        assert!(matches!(
            agent.process_batch(SourceBatch {
                records: vec![open(40), bad_market],
            }),
            Err(AgentError::TransitionRejected { offset: 41, .. })
        ));
        let status = agent.status().unwrap();
        assert_eq!(status.next_offset, 40);
        assert_eq!(status.active_hypotheses, 0);
    }

    #[test]
    fn custom_snapshot_round_trip_preserves_offset_and_future_behavior() {
        let mut source = agent(40);
        source
            .process_batch(SourceBatch {
                records: vec![open(40)],
            })
            .unwrap();
        let bytes = source.snapshot_bytes().unwrap();

        let mut restored = agent(40);
        restored.restore_snapshot_bytes(bytes).unwrap();
        assert_eq!(restored.status().unwrap(), source.status().unwrap());

        let expected = source
            .process_batch(SourceBatch {
                records: vec![likelihood(41)],
            })
            .unwrap();
        let actual = restored
            .process_batch(SourceBatch {
                records: vec![likelihood(41)],
            })
            .unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn negative_observation_uses_the_complement_likelihood() {
        assert_eq!(
            bayesian_update(100_000, false, 900_000, 200_000),
            Ok(13_699)
        );
    }
}
