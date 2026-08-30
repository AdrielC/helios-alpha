//! A hypothetical precursor-to-impact chain.
//!
//! The model is illustrative. It demonstrates conditional state, delayed external-model responses,
//! deadlines, and a research candidate. It does not claim that the precursor or probabilities are
//! scientifically calibrated.

use helio_hypothesis::{
    CausalEvidence, HypothesisConfig, HypothesisEngine, HypothesisEvent, HypothesisInput,
    HypothesisModel, HypothesisTransition, KeyedHypothesisMachine, TimerId,
};
use helio_time::{AvailableAt, EffectiveAt};
use serde::{Deserialize, Serialize};
use thiserror::Error;

const RESPONSE_DEADLINE: TimerId = TimerId(1);

#[derive(Debug, Clone)]
enum Evidence {
    Precursor { probability: f64 },
    Propagation { intersection_probability: f64 },
    InfrastructureImpact { disruption_probability: f64 },
    MarketResponse { expected_net_effect: f64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum Stage {
    AwaitingPropagation,
    AwaitingImpact,
    AwaitingMarketResponse,
    Candidate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct State {
    stage: Stage,
    joint_probability: f64,
}

#[derive(Debug, Clone)]
enum Action {
    RequestPropagationModel,
    RequestInfrastructureModel,
    RequestMarketModel,
    Candidate {
        joint_probability: f64,
        expected_net_effect: f64,
    },
    Expired,
}

#[derive(Debug, Clone, Copy, Error)]
enum ModelError {
    #[error("evidence is not valid for the current conditional stage")]
    UnexpectedEvidence,
    #[error("probability must be finite and between zero and one")]
    InvalidProbability,
    #[error("model state is invalid")]
    InvalidState,
}

#[derive(Debug, Clone, Copy)]
struct ConditionalShockModel;

impl ConditionalShockModel {
    fn probability(value: f64) -> Result<f64, ModelError> {
        if value.is_finite() && (0.0..=1.0).contains(&value) {
            Ok(value)
        } else {
            Err(ModelError::InvalidProbability)
        }
    }
}

impl HypothesisModel<String> for ConditionalShockModel {
    type Evidence = Evidence;
    type State = State;
    type Output = Action;
    type Error = ModelError;

    fn open(
        &self,
        _key: &String,
        evidence: CausalEvidence<Self::Evidence>,
    ) -> Result<HypothesisTransition<Self::State, Self::Output>, Self::Error> {
        let Evidence::Precursor { probability } = evidence.payload else {
            return Err(ModelError::UnexpectedEvidence);
        };
        let probability = Self::probability(probability)?;
        Ok(HypothesisTransition::new(State {
            stage: Stage::AwaitingPropagation,
            joint_probability: probability,
        })
        .emit(Action::RequestPropagationModel)
        .schedule(RESPONSE_DEADLINE, AvailableAt(evidence.available_at.0 + 30)))
    }

    fn update(
        &self,
        _key: &String,
        state: &Self::State,
        evidence: CausalEvidence<Self::Evidence>,
    ) -> Result<HypothesisTransition<Self::State, Self::Output>, Self::Error> {
        let mut state = state.clone();
        match (state.stage, evidence.payload) {
            (
                Stage::AwaitingPropagation,
                Evidence::Propagation {
                    intersection_probability,
                },
            ) => {
                state.joint_probability *= Self::probability(intersection_probability)?;
                state.stage = Stage::AwaitingImpact;
                Ok(HypothesisTransition::new(state)
                    .cancel(RESPONSE_DEADLINE)
                    .emit(Action::RequestInfrastructureModel)
                    .schedule(RESPONSE_DEADLINE, AvailableAt(evidence.available_at.0 + 30)))
            }
            (
                Stage::AwaitingImpact,
                Evidence::InfrastructureImpact {
                    disruption_probability,
                },
            ) => {
                state.joint_probability *= Self::probability(disruption_probability)?;
                state.stage = Stage::AwaitingMarketResponse;
                Ok(HypothesisTransition::new(state)
                    .cancel(RESPONSE_DEADLINE)
                    .emit(Action::RequestMarketModel)
                    .schedule(RESPONSE_DEADLINE, AvailableAt(evidence.available_at.0 + 30)))
            }
            (
                Stage::AwaitingMarketResponse,
                Evidence::MarketResponse {
                    expected_net_effect,
                },
            ) if expected_net_effect.is_finite() => {
                state.stage = Stage::Candidate;
                let joint_probability = state.joint_probability;
                Ok(HypothesisTransition::new(state)
                    .emit(Action::Candidate {
                        joint_probability,
                        expected_net_effect,
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
        _timer_id: TimerId,
        _at: AvailableAt,
    ) -> Result<HypothesisTransition<Self::State, Self::Output>, Self::Error> {
        Ok(HypothesisTransition::new(state.clone())
            .emit(Action::Expired)
            .complete())
    }

    fn validate(&self, _key: &String, state: &Self::State) -> Result<(), Self::Error> {
        if state.joint_probability.is_finite() && (0.0..=1.0).contains(&state.joint_probability) {
            Ok(())
        } else {
            Err(ModelError::InvalidState)
        }
    }
}

fn evidence(sequence: u64, available_at: i64, payload: Evidence) -> CausalEvidence<Evidence> {
    CausalEvidence::new(
        sequence,
        EffectiveAt(available_at - 1),
        AvailableAt(available_at),
        payload,
    )
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let machine = KeyedHypothesisMachine::try_new(
        ConditionalShockModel,
        HypothesisConfig::try_new(1_024, 4_096, 8, 16, 10_000)?,
    )?;
    let mut engine = HypothesisEngine::<String, _, String>::new(machine);
    let incident = "precursor/2026-08-29/001".to_string();

    let inputs = [
        HypothesisInput::Open {
            key: incident.clone(),
            evidence: evidence(0, 100, Evidence::Precursor { probability: 0.8 }),
        },
        HypothesisInput::Evidence {
            key: incident.clone(),
            evidence: evidence(
                1,
                104,
                Evidence::Propagation {
                    intersection_probability: 0.6,
                },
            ),
        },
        HypothesisInput::Evidence {
            key: incident.clone(),
            evidence: evidence(
                2,
                109,
                Evidence::InfrastructureImpact {
                    disruption_probability: 0.25,
                },
            ),
        },
        HypothesisInput::Evidence {
            key: incident,
            evidence: evidence(
                3,
                112,
                Evidence::MarketResponse {
                    expected_net_effect: 0.013,
                },
            ),
        },
    ];

    for input in inputs {
        for event in engine.process(input) {
            match event {
                HypothesisEvent::ModelOutput {
                    output:
                        Action::Candidate {
                            joint_probability,
                            expected_net_effect,
                        },
                    ..
                } => println!(
                    "candidate probability={joint_probability:.3} net_effect={expected_net_effect:.4}"
                ),
                other => println!("{other:?}"),
            }
        }
    }
    Ok(())
}
