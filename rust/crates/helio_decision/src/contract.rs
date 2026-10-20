use std::collections::{BTreeMap, BTreeSet};

use helio_hypothesis::CausalEvidence;
use helio_time::{AvailableAt, EffectiveAt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    OpenAi,
    SystemOne,
    Cloudflare,
}

/// Boolean `true` and the string `"true"` remain distinct across providers.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ChoiceValue {
    Text(String),
    Boolean(bool),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChoiceOption {
    pub value: ChoiceValue,
    #[serde(default)]
    pub description: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScoreLevel {
    pub label: String,
    #[serde(default)]
    pub description: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Question {
    Predicate {
        instructions: Value,
        #[serde(default)]
        criteria: Option<BTreeMap<String, Value>>,
    },
    Choice {
        instructions: Value,
        options: Vec<ChoiceOption>,
    },
    Score {
        instructions: Value,
        levels: Vec<ScoreLevel>,
    },
    /// Jev's image-localization primitive. Other adapters reject it explicitly.
    Box { instructions: Value },
}

/// State is portable JSON. Native input supports e.g. OpenAI message/image/tool transcripts.
/// Provider-specific media and caching options are separately bound to a protocol.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum DecisionInput {
    State { value: Value },
    Native { protocol: Protocol, value: Value },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeOptions {
    pub protocol: Protocol,
    pub fields: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionRequest {
    pub schema_version: u32,
    /// Immutable logical identity, including source revision and question-set version.
    pub request_id: String,
    /// All clocks here use Unix milliseconds, including deadlines.
    pub effective_at_ms: u64,
    pub available_at_ms: u64,
    pub deadline_ms: u64,
    pub input: DecisionInput,
    pub questions: BTreeMap<String, Question>,
    #[serde(default)]
    pub native_options: Option<NativeOptions>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChoiceProbability {
    pub value: ChoiceValue,
    pub probability: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LevelProbability {
    pub index: usize,
    pub label: String,
    pub probability: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Answer {
    Predicate {
        probability: f64,
    },
    Choice {
        choice: ChoiceValue,
        confidence: f64,
        probabilities: Vec<ChoiceProbability>,
    },
    Score {
        score: f64,
        confidence: f64,
        probabilities: Vec<LevelProbability>,
    },
    Box {
        top: f64,
        bottom: f64,
        left: f64,
        right: f64,
        confidence: Option<f64>,
    },
    Refusal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionResponse {
    pub model: String,
    pub answers: BTreeMap<String, Answer>,
    /// Provider metadata stays intact; confidence is not silently recalibrated.
    pub usage: Value,
}

impl DecisionResponse {
    /// Enforce the same contract for provider codecs, custom adapters, and stored replies.
    pub fn validate_for(&self, request: &DecisionRequest) -> Result<(), DecisionError> {
        request.validate()?;
        if self.model.trim().is_empty() || self.answers.len() != request.questions.len() {
            return Err(DecisionError::InvalidResponse);
        }
        for (id, question) in &request.questions {
            let answer = self.answers.get(id).ok_or(DecisionError::InvalidResponse)?;
            match (question, answer) {
                (_, Answer::Refusal) => {}
                (Question::Predicate { .. }, Answer::Predicate { probability: p }) => {
                    probability(*p)?;
                }
                (
                    Question::Choice { options, .. },
                    Answer::Choice {
                        choice,
                        confidence,
                        probabilities,
                    },
                ) => {
                    let expected: BTreeSet<_> = options.iter().map(|o| &o.value).collect();
                    let actual: BTreeSet<_> = probabilities.iter().map(|p| &p.value).collect();
                    if probabilities.len() != options.len()
                        || expected != actual
                        || !expected.contains(choice)
                    {
                        return Err(DecisionError::InvalidResponse);
                    }
                    probability(*confidence)?;
                    distribution(probabilities.iter().map(|p| p.probability))?;
                }
                (
                    Question::Score { levels, .. },
                    Answer::Score {
                        score,
                        confidence,
                        probabilities,
                    },
                ) => {
                    let mut seen = BTreeSet::new();
                    if probabilities.len() != levels.len() {
                        return Err(DecisionError::InvalidResponse);
                    }
                    for p in probabilities {
                        if !seen.insert(p.index)
                            || levels.get(p.index).map(|l| &l.label) != Some(&p.label)
                        {
                            return Err(DecisionError::InvalidResponse);
                        }
                    }
                    probability(*confidence)?;
                    distribution(probabilities.iter().map(|p| p.probability))?;
                    let maximum = (levels.len() - 1) as f64;
                    let expected: f64 = probabilities
                        .iter()
                        .map(|p| p.index as f64 * p.probability)
                        .sum();
                    if !score.is_finite()
                        || !(0.0..=maximum).contains(score)
                        || (score - expected).abs() > 1e-5 * maximum.max(1.0)
                    {
                        return Err(DecisionError::InvalidResponse);
                    }
                }
                (
                    Question::Box { .. },
                    Answer::Box {
                        top,
                        bottom,
                        left,
                        right,
                        confidence,
                    },
                ) => {
                    for coordinate in [top, bottom, left, right] {
                        probability(*coordinate)?;
                    }
                    if top > bottom || left > right {
                        return Err(DecisionError::InvalidResponse);
                    }
                    if let Some(confidence) = confidence {
                        probability(*confidence)?;
                    }
                }
                _ => return Err(DecisionError::InvalidResponse),
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Error)]
#[serde(rename_all = "snake_case")]
pub enum DecisionError {
    #[error("invalid decision request")]
    InvalidRequest,
    #[error("provider does not support this request capability")]
    Unsupported,
    #[error("provider returned an invalid or mismatched answer")]
    InvalidResponse,
    #[error("provider authentication failed")]
    Authentication,
    #[error("provider rate limit or capacity limit reached")]
    RateLimited,
    #[error("provider rejected the request")]
    Rejected,
    #[error("provider is unavailable")]
    Unavailable,
    #[error("request or response exceeded its byte limit")]
    TooLarge,
    #[error("decision deadline expired")]
    Expired,
    #[error("evidence is not yet available")]
    FutureEvidence,
    #[error("decision service capacity reached")]
    Busy,
    #[error("request identity was reused with different content or configuration")]
    IdentityConflict,
    #[error("request is pending or its prior outcome is unknown; not resubmitting")]
    Pending,
    #[error("decision journal is unavailable or invalid")]
    Journal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionReceipt {
    pub schema_version: u32,
    pub request_id: String,
    pub fingerprint: String,
    pub effective_at_ms: u64,
    pub available_at_ms: u64,
    pub received_at_ms: u64,
    pub latency_ms: u64,
    pub outcome: Result<DecisionResponse, DecisionError>,
}

impl DecisionReceipt {
    pub fn validate_for(&self, request: &DecisionRequest) -> Result<(), DecisionError> {
        request.validate()?;
        if self.schema_version != SCHEMA_VERSION
            || self.request_id != request.request_id
            || self.effective_at_ms != request.effective_at_ms
            || self.available_at_ms != request.available_at_ms
            || self.received_at_ms < request.available_at_ms
            || self.received_at_ms > i64::MAX as u64
        {
            return Err(DecisionError::InvalidResponse);
        }
        if let Ok(response) = &self.outcome {
            if self.received_at_ms >= request.deadline_ms {
                return Err(DecisionError::InvalidResponse);
            }
            response.validate_for(request)?;
        }
        Ok(())
    }

    /// The caller owns the next contiguous hypothesis sequence. Availability is receipt time,
    /// not the original event time: a delayed model answer cannot travel backwards in a replay.
    pub fn into_evidence(self, sequence: u64) -> Result<CausalEvidence<Self>, DecisionError> {
        let effective =
            i64::try_from(self.effective_at_ms).map_err(|_| DecisionError::InvalidRequest)?;
        let received =
            i64::try_from(self.received_at_ms).map_err(|_| DecisionError::InvalidResponse)?;
        if self.received_at_ms < self.available_at_ms {
            return Err(DecisionError::InvalidResponse);
        }
        Ok(CausalEvidence::new(
            sequence,
            EffectiveAt(effective),
            AvailableAt(received),
            self,
        ))
    }
}

impl DecisionRequest {
    pub fn validate(&self) -> Result<(), DecisionError> {
        if self.schema_version != SCHEMA_VERSION
            || self.request_id.trim().is_empty()
            || self.request_id.len() > 256
            || self.effective_at_ms > i64::MAX as u64
            || self.available_at_ms > i64::MAX as u64
            || self.deadline_ms > i64::MAX as u64
            || self.deadline_ms <= self.available_at_ms
            || self.questions.is_empty()
            || self.questions.len() > 400
        {
            return Err(DecisionError::InvalidRequest);
        }
        for (id, q) in &self.questions {
            if id.trim().is_empty() || id.len() > 256 {
                return Err(DecisionError::InvalidRequest);
            }
            match q {
                Question::Choice { options, .. } => {
                    let unique: BTreeSet<_> = options.iter().map(|o| &o.value).collect();
                    if options.is_empty() || options.len() > 255 || unique.len() != options.len() {
                        return Err(DecisionError::InvalidRequest);
                    }
                }
                Question::Score { levels, .. } => {
                    let unique: BTreeSet<_> = levels.iter().map(|l| &l.label).collect();
                    if levels.is_empty() || levels.len() > 255 || unique.len() != levels.len() {
                        return Err(DecisionError::InvalidRequest);
                    }
                }
                Question::Predicate {
                    criteria: Some(criteria),
                    ..
                } => {
                    if criteria.keys().any(|k| k != "true" && k != "false") {
                        return Err(DecisionError::InvalidRequest);
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    pub fn admit(&self, now_ms: u64) -> Result<(), DecisionError> {
        self.validate()?;
        if now_ms < self.available_at_ms {
            Err(DecisionError::FutureEvidence)
        } else if now_ms >= self.deadline_ms {
            Err(DecisionError::Expired)
        } else {
            Ok(())
        }
    }
}

pub fn fingerprint<T: Serialize>(value: &T) -> Result<String, DecisionError> {
    let mut canonical = serde_json::to_value(value).map_err(|_| DecisionError::InvalidRequest)?;
    canonical.sort_all_objects();
    let bytes = serde_json::to_vec(&canonical).map_err(|_| DecisionError::InvalidRequest)?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

pub(crate) fn probability(p: f64) -> Result<f64, DecisionError> {
    if p.is_finite() && (0.0..=1.0).contains(&p) {
        Ok(p)
    } else {
        Err(DecisionError::InvalidResponse)
    }
}

pub(crate) fn distribution(ps: impl Iterator<Item = f64>) -> Result<(), DecisionError> {
    let mut total = 0.0;
    for p in ps {
        total += probability(p)?;
    }
    if (total - 1.0).abs() > 1e-5 {
        Err(DecisionError::InvalidResponse)
    } else {
        Ok(())
    }
}
