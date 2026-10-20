use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::contract::probability;
use crate::*;

/// Pure protocol boundary. Implement this port for additional decision APIs.
pub trait DecisionAdapter: Send + Sync {
    fn identity(&self) -> Value;
    fn encode(&self, request: &DecisionRequest) -> Result<Value, DecisionError>;
    fn decode(
        &self,
        request: &DecisionRequest,
        response: Value,
    ) -> Result<DecisionResponse, DecisionError>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderAdapter {
    pub protocol: Protocol,
    pub model: String,
}

fn text(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string())
}

fn choice_keys(options: &[ChoiceOption]) -> Vec<String> {
    // Preserve native text labels when possible. If any value is Boolean, use
    // unambiguous ordinal keys for every option and carry the value in criteria.
    if options
        .iter()
        .all(|o| matches!(o.value, ChoiceValue::Text(_)))
    {
        options
            .iter()
            .map(|o| match &o.value {
                ChoiceValue::Text(s) => s.clone(),
                _ => unreachable!(),
            })
            .collect()
    } else {
        (0..options.len()).map(|i| format!("option_{i}")).collect()
    }
}

fn score_criterion(level: &ScoreLevel) -> Value {
    if level.description.is_null() {
        json!(level.label)
    } else {
        json!({"label":level.label,"description":level.description})
    }
}

impl DecisionAdapter for ProviderAdapter {
    fn identity(&self) -> Value {
        json!({"adapter_version": 2, "protocol": self.protocol, "model": self.model})
    }

    fn encode(&self, request: &DecisionRequest) -> Result<Value, DecisionError> {
        request.validate()?;
        if self.model.trim().is_empty() {
            return Err(DecisionError::InvalidRequest);
        }
        if self.protocol == Protocol::Cloudflare
            && (request.questions.len() > 64
                || request.questions.keys().any(|id| {
                    id.len() > 100
                        || !id
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
                }))
        {
            return Err(DecisionError::Unsupported);
        }
        let input = match &request.input {
            DecisionInput::State { value } => {
                if self.protocol == Protocol::OpenAi {
                    json!(text(value))
                } else {
                    value.clone()
                }
            }
            DecisionInput::Native { protocol, value } if *protocol == self.protocol => {
                value.clone()
            }
            _ => return Err(DecisionError::Unsupported),
        };
        let mut body = Map::new();
        if self.protocol != Protocol::OpenAi
            && !matches!(input, Value::String(_) | Value::Object(_) | Value::Array(_))
        {
            return Err(DecisionError::Unsupported);
        }
        if self.protocol == Protocol::OpenAi && !matches!(input, Value::String(_) | Value::Array(_))
        {
            return Err(DecisionError::Unsupported);
        }
        body.insert("model".into(), json!(self.model));
        body.insert(
            if self.protocol == Protocol::OpenAi {
                "input"
            } else {
                "state"
            }
            .into(),
            input,
        );
        let mut mapped = Map::new();
        let mut ordered = Vec::new();
        for (id, question) in &request.questions {
            if self.protocol == Protocol::Cloudflare {
                let instructions = match question {
                    Question::Predicate { instructions, .. }
                    | Question::Choice { instructions, .. }
                    | Question::Score { instructions, .. }
                    | Question::Box { instructions } => instructions,
                };
                if !matches!(instructions, Value::Object(_) | Value::Array(_))
                    && instructions.as_str().is_none_or(|s| s.trim().is_empty())
                {
                    return Err(DecisionError::Unsupported);
                }
            }
            let encoded = if self.protocol == Protocol::OpenAi {
                match question {
                    Question::Predicate {
                        instructions,
                        criteria,
                    } => {
                        let instruction = match criteria {
                            None => text(instructions),
                            Some(c) => {
                                format!("{}\nBoolean criteria: {}", text(instructions), json!(c))
                            }
                        };
                        json!({"type":"predicate","name":id,"instructions":instruction})
                    }
                    Question::Choice {
                        instructions,
                        options,
                    } => {
                        if options.len() < 2 {
                            return Err(DecisionError::Unsupported);
                        }
                        let choices: Vec<_> = options
                            .iter()
                            .map(|o| {
                                let mut c = json!({"value":o.value});
                                if !o.description.is_null() {
                                    c["description"] = json!(text(&o.description));
                                }
                                c
                            })
                            .collect();
                        json!({"type":"choice","name":id,"instructions":text(instructions),"choices":choices})
                    }
                    Question::Score {
                        instructions,
                        levels,
                    } => {
                        let levels: Vec<_> = levels
                            .iter()
                            .map(|l| {
                                let mut v = json!({"label":l.label});
                                if !l.description.is_null() {
                                    v["description"] = json!(text(&l.description));
                                }
                                v
                            })
                            .collect();
                        json!({"type":"score","name":id,"instructions":text(instructions),"levels":levels})
                    }
                    Question::Box { .. } => return Err(DecisionError::Unsupported),
                }
            } else {
                match question {
                    Question::Predicate {
                        instructions,
                        criteria,
                    } => {
                        let mut q = json!({"type":"noul","instructions":instructions});
                        if let Some(c) = criteria {
                            q["criteria"] = json!(c);
                        }
                        q
                    }
                    Question::Choice {
                        instructions,
                        options,
                    } => {
                        if self.protocol == Protocol::Cloudflare
                            && (options.len() < 2
                                || options.iter().any(
                                    |o| matches!(&o.value, ChoiceValue::Text(s) if s.is_empty()),
                                ))
                        {
                            return Err(DecisionError::Unsupported);
                        }
                        let keys = choice_keys(options);
                        let typed = options
                            .iter()
                            .any(|o| matches!(o.value, ChoiceValue::Boolean(_)));
                        let criteria: Map<_, _> = keys
                            .into_iter()
                            .zip(options.iter())
                            .map(|(k, o)| {
                                let v = if typed {
                                    json!({"value":o.value,"description":o.description})
                                } else {
                                    o.description.clone()
                                };
                                (k, v)
                            })
                            .collect();
                        json!({"type":"choice","instructions":instructions,"criteria":criteria})
                    }
                    Question::Score {
                        instructions,
                        levels,
                    } => {
                        if levels.len() > 10
                            || (self.protocol == Protocol::Cloudflare && levels.len() < 2)
                        {
                            return Err(DecisionError::Unsupported);
                        }
                        let criteria: Vec<_> = levels.iter().map(score_criterion).collect();
                        json!({"type":"score","instructions":instructions,"criteria":criteria})
                    }
                    Question::Box { instructions } if self.protocol == Protocol::SystemOne => {
                        json!({"type":"box","instructions":instructions})
                    }
                    Question::Box { .. } => return Err(DecisionError::Unsupported),
                }
            };
            if self.protocol == Protocol::OpenAi {
                ordered.push(encoded);
            } else {
                mapped.insert(id.clone(), encoded);
            }
        }
        body.insert(
            "questions".into(),
            if self.protocol == Protocol::OpenAi {
                Value::Array(ordered)
            } else {
                Value::Object(mapped)
            },
        );
        if let Some(options) = &request.native_options {
            if options.protocol != self.protocol {
                return Err(DecisionError::Unsupported);
            }
            // Explicitly allow documented protocol extensions. Never let an extension
            // replace the question binding, identity, endpoint or credentials.
            let allowed: &[&str] = match self.protocol {
                Protocol::OpenAi => &[
                    "prompt_cache_key",
                    "prompt_cache_options",
                    "safety_identifier",
                ],
                Protocol::SystemOne => &["assets"],
                Protocol::Cloudflare => &["images"],
            };
            for (key, value) in &options.fields {
                if !allowed.contains(&key.as_str()) {
                    return Err(DecisionError::Unsupported);
                }
                body.insert(key.clone(), value.clone());
            }
        }
        Ok(Value::Object(body))
    }

    fn decode(
        &self,
        request: &DecisionRequest,
        response: Value,
    ) -> Result<DecisionResponse, DecisionError> {
        // Validate capabilities before interpreting a reply, including during offline replay.
        self.encode(request)?;
        let response = if self.protocol == Protocol::Cloudflare {
            if response.get("success").and_then(Value::as_bool) != Some(true) {
                return Err(DecisionError::InvalidResponse);
            }
            response
                .get("result")
                .ok_or(DecisionError::InvalidResponse)?
        } else {
            &response
        };
        let model = response["model"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or(DecisionError::InvalidResponse)?
            .to_owned();
        let raw = &response["answers"];
        let mut answers = BTreeMap::new();
        if self.protocol == Protocol::OpenAi {
            let raw = raw.as_array().ok_or(DecisionError::InvalidResponse)?;
            if raw.len() != request.questions.len() {
                return Err(DecisionError::InvalidResponse);
            }
            for ((id, q), answer) in request.questions.iter().zip(raw) {
                if answer["name"].as_str() != Some(id.as_str()) {
                    return Err(DecisionError::InvalidResponse);
                }
                answers.insert(id.clone(), decode_answer(q, answer, true)?);
            }
        } else {
            let raw = raw.as_object().ok_or(DecisionError::InvalidResponse)?;
            if raw.len() != request.questions.len() {
                return Err(DecisionError::InvalidResponse);
            }
            for (id, q) in &request.questions {
                let answer = raw.get(id).ok_or(DecisionError::InvalidResponse)?;
                answers.insert(id.clone(), decode_answer(q, answer, false)?);
            }
        }
        let decoded = DecisionResponse {
            model,
            answers,
            usage: response.get("usage").cloned().unwrap_or(Value::Null),
        };
        decoded.validate_for(request)?;
        Ok(decoded)
    }
}

fn number(value: &Value) -> Result<f64, DecisionError> {
    value
        .as_f64()
        .filter(|v| v.is_finite())
        .ok_or(DecisionError::InvalidResponse)
}
fn prob(value: &Value) -> Result<f64, DecisionError> {
    probability(number(value)?)
}

fn decode_answer(q: &Question, raw: &Value, openai: bool) -> Result<Answer, DecisionError> {
    let kind = raw["type"].as_str().ok_or(DecisionError::InvalidResponse)?;
    if openai && kind == "refusal" {
        return Ok(Answer::Refusal);
    }
    match q {
        Question::Predicate { .. } if kind == if openai { "predicate" } else { "noul" } => {
            Ok(Answer::Predicate {
                probability: prob(&raw[if openai { "probability" } else { "noul" }])?,
            })
        }
        Question::Choice { options, .. } if kind == "choice" => {
            let keys = choice_keys(options);
            let (choice, probabilities) = if openai {
                let choice: ChoiceValue = serde_json::from_value(raw["choice"].clone())
                    .map_err(|_| DecisionError::InvalidResponse)?;
                let ps: Vec<ChoiceProbability> =
                    serde_json::from_value(raw["probabilities"].clone())
                        .map_err(|_| DecisionError::InvalidResponse)?;
                (choice, ps)
            } else {
                let chosen = keys
                    .iter()
                    .position(|k| raw["choice"].as_str() == Some(k))
                    .ok_or(DecisionError::InvalidResponse)?;
                let ps = raw["probabilities"]
                    .as_object()
                    .ok_or(DecisionError::InvalidResponse)?;
                if ps.len() != options.len() {
                    return Err(DecisionError::InvalidResponse);
                }
                let probabilities = keys
                    .iter()
                    .zip(options)
                    .map(|(key, option)| {
                        Ok(ChoiceProbability {
                            value: option.value.clone(),
                            probability: prob(&ps[key])?,
                        })
                    })
                    .collect::<Result<Vec<_>, DecisionError>>()?;
                (options[chosen].value.clone(), probabilities)
            };
            // The normalized contract checks membership and distributions below.
            Ok(Answer::Choice {
                choice,
                confidence: prob(&raw["confidence"])?,
                probabilities,
            })
        }
        Question::Score { levels, .. } if kind == "score" => {
            let score = number(&raw["score"])?;
            let mut probabilities = Vec::with_capacity(levels.len());
            if openai {
                let ps = raw["probabilities"]
                    .as_array()
                    .ok_or(DecisionError::InvalidResponse)?;
                if ps.len() != levels.len() {
                    return Err(DecisionError::InvalidResponse);
                }
                let mut seen = BTreeSet::new();
                for p in ps {
                    let index = p["value"]
                        .as_u64()
                        .and_then(|v| usize::try_from(v).ok())
                        .filter(|i| *i < levels.len())
                        .ok_or(DecisionError::InvalidResponse)?;
                    if !seen.insert(index) || p["label"].as_str() != Some(&levels[index].label) {
                        return Err(DecisionError::InvalidResponse);
                    }
                    probabilities.push(LevelProbability {
                        index,
                        label: levels[index].label.clone(),
                        probability: prob(&p["probability"])?,
                    });
                }
            } else {
                let ps = raw["probabilities"]
                    .as_object()
                    .ok_or(DecisionError::InvalidResponse)?;
                if ps.len() != levels.len() {
                    return Err(DecisionError::InvalidResponse);
                }
                let legend = raw["legend"]
                    .as_object()
                    .ok_or(DecisionError::InvalidResponse)?;
                if legend.len() != levels.len() {
                    return Err(DecisionError::InvalidResponse);
                }
                for (index, level) in levels.iter().enumerate() {
                    if legend.get(&index.to_string()) != Some(&score_criterion(level)) {
                        return Err(DecisionError::InvalidResponse);
                    }
                    probabilities.push(LevelProbability {
                        index,
                        label: level.label.clone(),
                        probability: prob(&ps[&index.to_string()])?,
                    });
                }
            }
            Ok(Answer::Score {
                score,
                confidence: prob(&raw["confidence"])?,
                probabilities,
            })
        }
        Question::Box { .. } if !openai && kind == "box" => {
            let b = &raw["box"];
            let (top, bottom, left, right) = (
                prob(&b["top"])?,
                prob(&b["bottom"])?,
                prob(&b["left"])?,
                prob(&b["right"])?,
            );
            let confidence = if raw["confidence"].is_null() {
                None
            } else {
                Some(prob(&raw["confidence"])?)
            };
            Ok(Answer::Box {
                top,
                bottom,
                left,
                right,
                confidence,
            })
        }
        _ => Err(DecisionError::InvalidResponse),
    }
}
