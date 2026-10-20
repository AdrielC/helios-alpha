mod common;
use common::*;
use helio_decision::*;
use proptest::prelude::*;
use serde_json::{json, Value};
use std::collections::BTreeMap;

const PROVIDERS: [Protocol; 3] = [Protocol::OpenAi, Protocol::SystemOne, Protocol::Cloudflare];

proptest! {
    #[test]
    fn predicate_probability_is_preserved(p in 0.0f64..=1.0) {
        for protocol in PROVIDERS {
            let result = adapter(protocol).decode(&request(), predicate_response(protocol, p)).unwrap();
            prop_assert_eq!(&result.answers["verified"], &Answer::Predicate { probability: p });
        }
    }

    #[test]
    fn invalid_probability_never_becomes_evidence(p in prop_oneof![(-1e10f64..0.0), (1.00001f64..1e10)]) {
        for protocol in PROVIDERS {
            prop_assert_eq!(adapter(protocol).decode(&request(), predicate_response(protocol, p)), Err(DecisionError::InvalidResponse));
        }
    }

    #[test]
    fn typed_boolean_choices_never_alias_text(p in 0.0f64..=1.0, confidence in 0.0f64..=1.0) {
        let mut r = request();
        r.questions = BTreeMap::from([("typed".into(), Question::Choice {
            instructions: json!("Which typed value?"),
            options: vec![
                ChoiceOption { value: ChoiceValue::Boolean(true), description: Value::Null },
                ChoiceOption { value: ChoiceValue::Text("true".into()), description: Value::Null },
            ],
        })]);
        for protocol in PROVIDERS {
            let raw = if protocol == Protocol::OpenAi {
                json!([{"type":"choice","name":"typed","choice":true,"confidence":confidence,
                    "probabilities":[{"value":true,"probability":p},{"value":"true","probability":1.0-p}]}])
            } else {
                json!({"typed":{"type":"choice","choice":"option_0","confidence":confidence,
                    "probabilities":{"option_0":p,"option_1":1.0-p}}})
            };
            let result = adapter(protocol).decode(&r, response(protocol, raw)).unwrap();
            let Answer::Choice { choice, confidence: c, probabilities } = &result.answers["typed"] else { panic!("wrong primitive"); };
            prop_assert_eq!(choice, &ChoiceValue::Boolean(true));
            prop_assert_eq!(*c, confidence);
            prop_assert_eq!(probabilities.len(), 2);
            prop_assert!(probabilities.iter().any(|p| p.value == ChoiceValue::Text("true".into())));
        }
    }

    #[test]
    fn admission_uses_availability_and_deadline(available in 1u64..1_000_000, width in 1u64..10_000, now in 0u64..1_020_000) {
        let mut r = request();
        r.available_at_ms = available;
        r.deadline_ms = available + width;
        let expected = if now < available { Err(DecisionError::FutureEvidence) }
            else if now >= available + width { Err(DecisionError::Expired) } else { Ok(()) };
        prop_assert_eq!(r.admit(now), expected);
    }

    #[test]
    fn arbitrary_json_cannot_panic_or_invent_an_answer(s in "\\PC{0,300}") {
        for protocol in PROVIDERS {
            let result = adapter(protocol).decode(&request(), json!({"answers":s}));
            prop_assert_eq!(result, Err(DecisionError::InvalidResponse));
        }
    }

    #[test]
    fn nested_object_order_does_not_change_identity(a in any::<i64>(), b in "\\PC{0,40}") {
        let x = json!({"b": {"z":b.clone(),"a":a}, "a": [a]});
        let y: Value = serde_json::from_str(&format!(r#"{{"a":[{a}],"b":{{"a":{a},"z":{}}}}}"#, json!(b))).unwrap();
        prop_assert_eq!(fingerprint(&x).unwrap(), fingerprint(&y).unwrap());
    }

    #[test]
    fn evidence_cannot_backdate_provider_arrival(available in 0u64..1_000_000, delay in 0u64..1_000_000, seq in any::<u64>()) {
        let receipt = DecisionReceipt {
            schema_version: SCHEMA_VERSION, request_id: "r".into(), fingerprint: "fp".into(),
            effective_at_ms: 0, available_at_ms: available, received_at_ms: available + delay,
            latency_ms: delay, outcome: Err(DecisionError::Expired),
        };
        let evidence = receipt.into_evidence(seq).unwrap();
        prop_assert_eq!(evidence.sequence, seq);
        prop_assert_eq!(evidence.available_at.0, (available + delay) as i64);
    }

    #[test]
    fn score_rubrics_and_weighted_means_remain_bound(
        weights in prop::collection::vec(1u32..10_000, 2..11),
        structured in any::<bool>(),
        confidence in 0.0f64..=1.0,
    ) {
        let total: f64 = weights.iter().map(|w| *w as f64).sum();
        let ps: Vec<f64> = weights.iter().map(|w| *w as f64 / total).collect();
        let expected: f64 = ps.iter().enumerate().map(|(i, p)| i as f64 * p).sum();
        let levels: Vec<_> = (0..weights.len()).map(|i| ScoreLevel {
            label: format!("level-{i}"),
            description: if structured { json!({"evidence":i}) } else { Value::Null },
        }).collect();
        let mut r = request();
        r.questions = BTreeMap::from([("impact".into(), Question::Score {
            instructions: json!("Rate impact"), levels: levels.clone(),
        })]);
        for protocol in PROVIDERS {
            let mut raw = if protocol == Protocol::OpenAi {
                json!([{"type":"score","name":"impact","score":expected,"confidence":confidence,
                    "probabilities":ps.iter().enumerate().map(|(i,p)| json!({
                        "value":i,"label":format!("level-{i}"),"probability":p
                    })).collect::<Vec<_>>()}])
            } else {
                let legend: BTreeMap<_,_> = levels.iter().enumerate().map(|(i,l)| {
                    let value = if structured {
                        json!({"label":l.label,"description":{"evidence":i}})
                    } else { json!(format!("level-{i}")) };
                    (i.to_string(), value)
                }).collect();
                let probabilities: BTreeMap<_,_> =
                    ps.iter().enumerate().map(|(i,p)| (i.to_string(), *p)).collect();
                json!({"impact":{"type":"score","score":expected,"confidence":confidence,
                    "legend":legend,"probabilities":probabilities}})
            };
            let decoded = adapter(protocol).decode(&r, response(protocol, raw.clone())).unwrap();
            let Answer::Score { score, .. } = decoded.answers["impact"] else { panic!("wrong answer type") };
            prop_assert!((score - expected).abs() < 1e-12);
            let maximum = (weights.len() - 1) as f64;
            let wrong_score = if expected < maximum / 2.0 { maximum } else { 0.0 };
            let mut wrong = raw.clone();
            if protocol == Protocol::OpenAi {
                wrong[0]["score"] = json!(wrong_score);
                raw[0]["probabilities"][0]["label"] = json!("different rubric");
            } else {
                wrong["impact"]["score"] = json!(wrong_score);
                raw["impact"]["legend"]["0"] = json!("different rubric");
            }
            prop_assert_eq!(adapter(protocol).decode(&r, response(protocol, wrong)), Err(DecisionError::InvalidResponse));
            prop_assert_eq!(adapter(protocol).decode(&r, response(protocol, raw)), Err(DecisionError::InvalidResponse));
        }
    }
}

#[test]
fn ordered_names_refusals_and_missing_answers_are_checked() {
    let mut r = request();
    r.questions
        .insert("another".into(), r.questions["verified"].clone());
    let a = adapter(Protocol::OpenAi);
    let raw = response(
        Protocol::OpenAi,
        json!([
            {"name":"another","type":"refusal"},
            {"name":"verified","type":"predicate","probability":0.7}
        ]),
    );
    let result = a.decode(&r, raw.clone()).unwrap();
    assert_eq!(result.answers["another"], Answer::Refusal);
    let mut swapped = raw.clone();
    swapped["answers"].as_array_mut().unwrap().swap(0, 1);
    assert_eq!(a.decode(&r, swapped), Err(DecisionError::InvalidResponse));
    let mut missing = raw;
    missing["answers"].as_array_mut().unwrap().pop();
    assert_eq!(a.decode(&r, missing), Err(DecisionError::InvalidResponse));
}

#[test]
fn score_index_binding_and_distributions_are_checked() {
    let mut r = request();
    r.questions = BTreeMap::from([(
        "impact".into(),
        Question::Score {
            instructions: json!("Rate operational impact"),
            levels: vec![
                ScoreLevel {
                    label: "none".into(),
                    description: Value::Null,
                },
                ScoreLevel {
                    label: "severe".into(),
                    description: Value::Null,
                },
            ],
        },
    )]);
    for protocol in PROVIDERS {
        let raw = if protocol == Protocol::OpenAi {
            json!([{"type":"score","name":"impact","score":0.8,"confidence":0.6,
                "probabilities":[{"value":1,"label":"severe","probability":0.8},{"value":0,"label":"none","probability":0.2}]}])
        } else {
            json!({"impact":{"type":"score","score":0.8,"confidence":0.6,
                "legend":{"0":"none","1":"severe"},"probabilities":{"0":0.2,"1":0.8}}})
        };
        assert!(adapter(protocol)
            .decode(&r, response(protocol, raw.clone()))
            .is_ok());
        let mut broken = raw;
        if protocol == Protocol::OpenAi {
            broken[0]["probabilities"][0]["value"] = json!(0);
        } else {
            broken["impact"]["probabilities"]["1"] = json!(1.8);
        }
        assert_eq!(
            adapter(protocol).decode(&r, response(protocol, broken)),
            Err(DecisionError::InvalidResponse)
        );
    }
}

#[test]
fn native_media_and_cache_options_are_preserved_but_cannot_override_binding() {
    let mut r = request();
    let messages = json!([{"role":"user","content":[
        {"type":"input_text","text":"Inspect image","prompt_cache_breakpoint":{"mode":"explicit"}},
        {"type":"input_image","image_url":"data:image/png;base64,AA=="}]}]);
    r.input = DecisionInput::Native {
        protocol: Protocol::OpenAi,
        value: messages.clone(),
    };
    r.native_options = Some(NativeOptions {
        protocol: Protocol::OpenAi,
        fields: BTreeMap::from([("prompt_cache_key".into(), json!("research-v1"))]),
    });
    assert_eq!(
        adapter(Protocol::OpenAi).encode(&r).unwrap()["input"],
        messages
    );
    assert_eq!(
        adapter(Protocol::SystemOne).encode(&r),
        Err(DecisionError::Unsupported)
    );
    r.native_options
        .as_mut()
        .unwrap()
        .fields
        .insert("model".into(), json!("different"));
    assert_eq!(
        adapter(Protocol::OpenAi).encode(&r),
        Err(DecisionError::Unsupported)
    );
}

#[test]
fn jev_image_localization_and_clef_capability_limits() {
    let mut r = request();
    r.questions = BTreeMap::from([(
        "object".into(),
        Question::Box {
            instructions: json!("Find the object in image-id"),
        },
    )]);
    r.native_options = Some(NativeOptions {
        protocol: Protocol::SystemOne,
        fields: BTreeMap::from([(
            "assets".into(),
            json!({"image-id":{"type":"image","format":"png","data_base64":"AA=="}}),
        )]),
    });
    assert!(adapter(Protocol::SystemOne)
        .encode(&r)
        .unwrap()
        .get("assets")
        .is_some());
    let raw = response(
        Protocol::SystemOne,
        json!({"object":{"type":"box","box":{"top":0.1,"bottom":0.6,"left":0.2,"right":0.8},"confidence":null}}),
    );
    assert!(adapter(Protocol::SystemOne).decode(&r, raw).is_ok());
    r.native_options = None;
    assert_eq!(
        adapter(Protocol::Cloudflare).encode(&r),
        Err(DecisionError::Unsupported)
    );
    let mut r = request();
    let q = r.questions["verified"].clone();
    r.questions = (0..65).map(|i| (format!("q{i}"), q.clone())).collect();
    assert_eq!(
        adapter(Protocol::Cloudflare).encode(&r),
        Err(DecisionError::Unsupported)
    );
    assert!(adapter(Protocol::OpenAi).encode(&r).is_ok());
}
