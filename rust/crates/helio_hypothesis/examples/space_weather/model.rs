//! Typed reference model for a space-weather event-shock hypothesis.
//!
//! This module deliberately lives under `examples`: the hypothesis runtime remains domain-free.

use helio_hypothesis::{CausalEvidence, HypothesisModel, HypothesisTransition, TimerId};
use helio_stats::LogProbability;
use helio_time::AvailableAt;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const RESPONSE_DEADLINE: TimerId = TimerId(1);
const RESPONSE_BUDGET_SECS: i64 = 30 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImpactSector {
    SatelliteOperations,
    ElectricGrid,
    NavigationAndRadio,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Evidence {
    SolarEruption {
        source_confidence: f64,
        radio_blackout_scale: u8,
    },
    CmePropagation {
        earth_intersection_probability: f64,
        arrival_start: i64,
        arrival_end: i64,
    },
    InfrastructureImpact {
        disruption_probability: f64,
        sector: ImpactSector,
    },
    MarketAssessment {
        expected_net_return: f64,
        forecast_stddev: f64,
        max_notional: f64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Stage {
    AwaitingPropagation,
    AwaitingInfrastructure,
    AwaitingMarketAssessment,
    Candidate,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct State {
    pub stage: Stage,
    pub joint_probability: LogProbability,
    pub radio_blackout_scale: u8,
    pub arrival_window: Option<(i64, i64)>,
    pub sector: Option<ImpactSector>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    /// Compatibility/display value. It may round to zero for an extremely long conditional chain.
    pub joint_probability: f64,
    /// Authoritative value for comparisons and downstream probability algebra.
    pub log_joint_probability: f64,
    pub expected_net_return: f64,
    pub forecast_stddev: f64,
    pub max_notional: f64,
    pub radio_blackout_scale: u8,
    pub arrival_window: (i64, i64),
    pub sector: ImpactSector,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    RequestCmePropagation,
    RequestInfrastructureImpact {
        arrival_window: (i64, i64),
    },
    RequestMarketAssessment {
        sector: ImpactSector,
        arrival_window: (i64, i64),
    },
    Candidate(Candidate),
    Expired {
        stage: Stage,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ModelError {
    #[error("evidence is not valid for the current space-weather stage")]
    UnexpectedEvidence,
    #[error("probability must be finite and between zero and one")]
    InvalidProbability,
    #[error("NOAA event scale must be between one and five")]
    InvalidEventScale,
    #[error("arrival window is invalid")]
    InvalidArrivalWindow,
    #[error("market assessment must contain finite return, uncertainty, and capacity")]
    InvalidMarketAssessment,
    #[error("deadline arithmetic overflowed")]
    TimeOverflow,
    #[error("space-weather model state is invalid")]
    InvalidState,
}

#[derive(Debug, Clone, Copy)]
pub struct SpaceWeatherModel;

impl SpaceWeatherModel {
    fn probability(value: f64) -> Result<LogProbability, ModelError> {
        LogProbability::try_from_probability(value).map_err(|_| ModelError::InvalidProbability)
    }

    fn deadline(available_at: AvailableAt) -> Result<AvailableAt, ModelError> {
        available_at
            .0
            .checked_add(RESPONSE_BUDGET_SECS)
            .map(AvailableAt)
            .ok_or(ModelError::TimeOverflow)
    }
}

impl HypothesisModel<String> for SpaceWeatherModel {
    type Evidence = Evidence;
    type State = State;
    type Output = Action;
    type Error = ModelError;

    fn open(
        &self,
        _key: &String,
        evidence: CausalEvidence<Self::Evidence>,
    ) -> Result<HypothesisTransition<Self::State, Self::Output>, Self::Error> {
        let Evidence::SolarEruption {
            source_confidence,
            radio_blackout_scale,
        } = evidence.payload
        else {
            return Err(ModelError::UnexpectedEvidence);
        };
        if !(1..=5).contains(&radio_blackout_scale) {
            return Err(ModelError::InvalidEventScale);
        }
        let state = State {
            stage: Stage::AwaitingPropagation,
            joint_probability: Self::probability(source_confidence)?,
            radio_blackout_scale,
            arrival_window: None,
            sector: None,
        };
        Ok(HypothesisTransition::new(state)
            .emit(Action::RequestCmePropagation)
            .schedule(RESPONSE_DEADLINE, Self::deadline(evidence.available_at)?))
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
                Evidence::CmePropagation {
                    earth_intersection_probability,
                    arrival_start,
                    arrival_end,
                },
            ) => {
                if arrival_start >= arrival_end || arrival_start <= evidence.available_at.0 {
                    return Err(ModelError::InvalidArrivalWindow);
                }
                state.joint_probability = state
                    .joint_probability
                    .try_product(Self::probability(earth_intersection_probability)?)
                    .map_err(|_| ModelError::InvalidProbability)?;
                state.stage = Stage::AwaitingInfrastructure;
                state.arrival_window = Some((arrival_start, arrival_end));
                Ok(HypothesisTransition::new(state)
                    .cancel(RESPONSE_DEADLINE)
                    .emit(Action::RequestInfrastructureImpact {
                        arrival_window: (arrival_start, arrival_end),
                    })
                    .schedule(RESPONSE_DEADLINE, Self::deadline(evidence.available_at)?))
            }
            (
                Stage::AwaitingInfrastructure,
                Evidence::InfrastructureImpact {
                    disruption_probability,
                    sector,
                },
            ) => {
                state.joint_probability = state
                    .joint_probability
                    .try_product(Self::probability(disruption_probability)?)
                    .map_err(|_| ModelError::InvalidProbability)?;
                state.stage = Stage::AwaitingMarketAssessment;
                state.sector = Some(sector);
                let arrival_window = state.arrival_window.ok_or(ModelError::InvalidState)?;
                Ok(HypothesisTransition::new(state)
                    .cancel(RESPONSE_DEADLINE)
                    .emit(Action::RequestMarketAssessment {
                        sector,
                        arrival_window,
                    })
                    .schedule(RESPONSE_DEADLINE, Self::deadline(evidence.available_at)?))
            }
            (
                Stage::AwaitingMarketAssessment,
                Evidence::MarketAssessment {
                    expected_net_return,
                    forecast_stddev,
                    max_notional,
                },
            ) => {
                let valid_market = expected_net_return.is_finite()
                    && forecast_stddev.is_finite()
                    && forecast_stddev >= 0.0
                    && max_notional.is_finite()
                    && max_notional >= 0.0;
                if !valid_market {
                    return Err(ModelError::InvalidMarketAssessment);
                }
                state.stage = Stage::Candidate;
                let candidate = Candidate {
                    joint_probability: state.joint_probability.probability(),
                    log_joint_probability: state.joint_probability.ln_probability(),
                    expected_net_return,
                    forecast_stddev,
                    max_notional,
                    radio_blackout_scale: state.radio_blackout_scale,
                    arrival_window: state.arrival_window.ok_or(ModelError::InvalidState)?,
                    sector: state.sector.ok_or(ModelError::InvalidState)?,
                };
                Ok(HypothesisTransition::new(state)
                    .emit(Action::Candidate(candidate))
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
        if timer_id != RESPONSE_DEADLINE {
            return Err(ModelError::UnexpectedEvidence);
        }
        Ok(HypothesisTransition::new(state.clone())
            .emit(Action::Expired { stage: state.stage })
            .complete())
    }

    fn validate(&self, _key: &String, state: &Self::State) -> Result<(), Self::Error> {
        state
            .joint_probability
            .validate()
            .map_err(|_| ModelError::InvalidState)?;
        if !(1..=5).contains(&state.radio_blackout_scale) {
            return Err(ModelError::InvalidState);
        }
        if let Some((start, end)) = state.arrival_window {
            if start >= end {
                return Err(ModelError::InvalidState);
            }
        }
        let shape_is_valid = match state.stage {
            Stage::AwaitingPropagation => state.arrival_window.is_none() && state.sector.is_none(),
            Stage::AwaitingInfrastructure => {
                state.arrival_window.is_some() && state.sector.is_none()
            }
            Stage::AwaitingMarketAssessment | Stage::Candidate => {
                state.arrival_window.is_some() && state.sector.is_some()
            }
        };
        if shape_is_valid {
            Ok(())
        } else {
            Err(ModelError::InvalidState)
        }
    }
}
