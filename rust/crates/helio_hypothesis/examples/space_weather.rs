//! Executable space-weather reference chain.
//!
//! This proves the orchestration boundary. The probabilities and market assessment are synthetic;
//! no output in this example has order authority.

#[path = "space_weather/model.rs"]
mod model;

use helio_hypothesis::{
    CausalEvidence, HypothesisConfig, HypothesisEngine, HypothesisEvent, HypothesisInput,
    KeyedHypothesisMachine,
};
use helio_time::{AvailableAt, EffectiveAt};
use model::{Action, Evidence, ImpactSector, SpaceWeatherModel};

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
        SpaceWeatherModel,
        HypothesisConfig::try_new(1_024, 4_096, 8, 16, 10_000)?,
    )?;
    let mut engine = HypothesisEngine::<String, _, String>::new(machine);
    let incident = "space-weather/2026-08-29/001".to_string();
    let inputs = [
        HypothesisInput::Open {
            key: incident.clone(),
            evidence: evidence(
                0,
                100,
                Evidence::SolarEruption {
                    source_confidence: 0.92,
                    radio_blackout_scale: 3,
                },
            ),
        },
        HypothesisInput::Evidence {
            key: incident.clone(),
            evidence: evidence(
                1,
                180,
                Evidence::CmePropagation {
                    earth_intersection_probability: 0.55,
                    arrival_start: 3_600,
                    arrival_end: 7_200,
                },
            ),
        },
        HypothesisInput::Evidence {
            key: incident.clone(),
            evidence: evidence(
                2,
                240,
                Evidence::InfrastructureImpact {
                    disruption_probability: 0.20,
                    sector: ImpactSector::SatelliteOperations,
                },
            ),
        },
        HypothesisInput::Evidence {
            key: incident,
            evidence: evidence(
                3,
                300,
                Evidence::MarketAssessment {
                    expected_net_return: 0.006,
                    forecast_stddev: 0.021,
                    max_notional: 100_000.0,
                },
            ),
        },
    ];

    for input in inputs {
        for event in engine.process(input) {
            match event {
                HypothesisEvent::ModelOutput {
                    output: Action::Candidate(candidate),
                    ..
                } => println!(
                    "candidate only: ln(p)={:.6} p={:.6} net_return={:.4} sigma={:.4} max_notional={:.0}",
                    candidate.log_joint_probability,
                    candidate.joint_probability,
                    candidate.expected_net_return,
                    candidate.forecast_stddev,
                    candidate.max_notional,
                ),
                other => println!("{other:?}"),
            }
        }
    }
    Ok(())
}
