#[path = "../examples/space_weather/model.rs"]
mod model;

use helio_hypothesis::{
    CausalEvidence, HypothesisConfig, HypothesisEngine, HypothesisEvent, HypothesisInput,
    KeyedHypothesisMachine,
};
use helio_time::{AvailableAt, EffectiveAt};
use model::{Action, Evidence, ImpactSector, SpaceWeatherModel};

fn machine() -> KeyedHypothesisMachine<String, SpaceWeatherModel, String> {
    KeyedHypothesisMachine::try_new(
        SpaceWeatherModel,
        HypothesisConfig::try_new(32, 64, 4, 8, 128).unwrap(),
    )
    .unwrap()
}

fn evidence(sequence: u64, available_at: i64, payload: Evidence) -> CausalEvidence<Evidence> {
    CausalEvidence::new(
        sequence,
        EffectiveAt(available_at - 1),
        AvailableAt(available_at),
        payload,
    )
}

fn inputs(probability: f64) -> Vec<HypothesisInput<String, Evidence, String>> {
    let key = "space-weather/test/001".to_string();
    vec![
        HypothesisInput::Open {
            key: key.clone(),
            evidence: evidence(
                0,
                100,
                Evidence::SolarEruption {
                    source_confidence: probability,
                    radio_blackout_scale: 3,
                },
            ),
        },
        HypothesisInput::Evidence {
            key: key.clone(),
            evidence: evidence(
                1,
                120,
                Evidence::CmePropagation {
                    earth_intersection_probability: probability,
                    arrival_start: 3_600,
                    arrival_end: 7_200,
                },
            ),
        },
        HypothesisInput::Evidence {
            key: key.clone(),
            evidence: evidence(
                2,
                140,
                Evidence::InfrastructureImpact {
                    disruption_probability: probability,
                    sector: ImpactSector::ElectricGrid,
                },
            ),
        },
        HypothesisInput::Evidence {
            key,
            evidence: evidence(
                3,
                160,
                Evidence::MarketAssessment {
                    expected_net_return: 0.004,
                    forecast_stddev: 0.019,
                    max_notional: 25_000.0,
                },
            ),
        },
    ]
}

#[test]
fn checkpoint_resume_matches_continuous_space_weather_chain() {
    let stream = inputs(0.5);
    let mut continuous = HypothesisEngine::new(machine());
    let expected: Vec<_> = stream
        .clone()
        .into_iter()
        .flat_map(|input| continuous.process(input))
        .collect();

    let mut resumed = HypothesisEngine::new(machine());
    let mut actual = Vec::new();
    for input in stream[..2].iter().cloned() {
        actual.extend(resumed.process(input));
    }
    let encoded = serde_json::to_vec(&resumed.snapshot()).unwrap();
    let snapshot = serde_json::from_slice(&encoded).unwrap();
    let mut resumed = HypothesisEngine::try_from_snapshot(machine(), snapshot).unwrap();
    for input in stream[2..].iter().cloned() {
        actual.extend(resumed.process(input));
    }
    assert_eq!(actual, expected);
}

#[test]
fn tiny_conditional_probabilities_do_not_poison_state() {
    let mut engine = HypothesisEngine::new(machine());
    let events: Vec<_> = inputs(1e-200)
        .into_iter()
        .flat_map(|input| engine.process(input))
        .collect();
    let candidate = events.iter().find_map(|event| match event {
        HypothesisEvent::ModelOutput {
            output: Action::Candidate(candidate),
            ..
        } => Some(candidate),
        _ => None,
    });
    let candidate = candidate.unwrap_or_else(|| panic!("candidate output in {events:#?}"));
    assert_eq!(candidate.joint_probability, 0.0);
    assert!(candidate.log_joint_probability.is_finite());
    assert!(candidate.log_joint_probability < -1_000.0);
}

#[test]
fn invalid_market_forecast_is_rejected_without_state_mutation() {
    let mut stream = inputs(0.5);
    let bad = stream.pop().unwrap();
    let mut engine = HypothesisEngine::new(machine());
    for input in stream {
        engine.process(input);
    }
    let before = engine.snapshot();
    let HypothesisInput::Evidence { key, evidence: e } = bad else {
        unreachable!();
    };
    let events = engine.process(HypothesisInput::Evidence {
        key,
        evidence: evidence(
            e.sequence,
            e.available_at.0,
            Evidence::MarketAssessment {
                expected_net_return: f64::NAN,
                forecast_stddev: 0.019,
                max_notional: 25_000.0,
            },
        ),
    });
    assert!(events
        .iter()
        .any(|event| matches!(event, HypothesisEvent::Rejected { .. })));
    assert_eq!(engine.snapshot(), before);
}

#[test]
fn non_future_arrival_window_is_rejected_without_state_mutation() {
    let mut engine = HypothesisEngine::new(machine());
    engine.process(inputs(0.5).remove(0));
    let before = engine.snapshot();
    let events = engine.process(HypothesisInput::Evidence {
        key: "space-weather/test/001".to_string(),
        evidence: evidence(
            1,
            120,
            Evidence::CmePropagation {
                earth_intersection_probability: 0.5,
                arrival_start: 120,
                arrival_end: 7_200,
            },
        ),
    });
    assert!(events
        .iter()
        .any(|event| matches!(event, HypothesisEvent::Rejected { .. })));
    assert_eq!(engine.snapshot(), before);
}
