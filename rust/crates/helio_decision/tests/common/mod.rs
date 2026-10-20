#![allow(dead_code)]
use helio_decision::*;
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub fn request() -> DecisionRequest {
    DecisionRequest {
        schema_version: SCHEMA_VERSION,
        request_id: "source-revision-1:questions-v1".into(),
        effective_at_ms: 50,
        available_at_ms: 100,
        deadline_ms: 10_000,
        input: DecisionInput::State {
            value: json!({"event":"test","verified":true}),
        },
        questions: BTreeMap::from([(
            "verified".into(),
            Question::Predicate {
                instructions: json!("Is the observation verified?"),
                criteria: None,
            },
        )]),
        native_options: None,
    }
}
pub fn adapter(protocol: Protocol) -> ProviderAdapter {
    ProviderAdapter {
        protocol,
        model: match protocol {
            Protocol::OpenAi => "gpt-6-luna",
            Protocol::SystemOne => "jev-latest",
            Protocol::Cloudflare => "clef",
        }
        .into(),
    }
}
pub fn response(protocol: Protocol, answers: Value) -> Value {
    let result =
        json!({"model":adapter(protocol).model,"answers":answers,"usage":{"input_tokens":12}});
    if protocol == Protocol::Cloudflare {
        json!({"success":true,"result":result,"errors":[],"messages":[]})
    } else {
        result
    }
}
pub fn predicate_response(protocol: Protocol, p: f64) -> Value {
    response(
        protocol,
        if protocol == Protocol::OpenAi {
            json!([{"type":"predicate","name":"verified","probability":p}])
        } else {
            json!({"verified":{"type":"noul","noul":p}})
        },
    )
}
