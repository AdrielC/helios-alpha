use std::collections::BTreeMap;

use golem_rust::{Schema, agent_definition, agent_implementation};
use helio_execution::{
    ExecutionMode, MoneyMicros, OrderProposal, PortfolioRiskSnapshot, PriceMicros, QuantityMicros,
    RiskAuthority, RiskAuthoritySnapshot, RiskContext, RiskDecision, RiskPolicy, RiskRejection,
};
use helio_time::VenueSchedule;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{ExecutionModeInput, OrderIntentInput, SideInput};

const SNAPSHOT_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Schema, Serialize, Deserialize)]
pub struct NamedExposureInput {
    pub name: String,
    pub amount_micros: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Schema, Serialize, Deserialize)]
pub struct SymbolPositionInput {
    pub symbol: String,
    pub quantity_micros: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Schema, Serialize, Deserialize)]
pub struct PortfolioRiskInput {
    pub as_of_ns: u64,
    pub trading_day: i32,
    pub gross_exposure_micros: u64,
    pub strategy_exposure: Vec<NamedExposureInput>,
    pub symbol_positions: Vec<SymbolPositionInput>,
    pub daily_order_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Schema, Serialize, Deserialize)]
pub struct ConfigureRiskInput {
    pub risk_policy_json: String,
    pub venue_schedule_json: String,
    pub initial_portfolio: PortfolioRiskInput,
}

#[derive(Debug, Clone, PartialEq, Eq, Schema, Serialize, Deserialize)]
pub struct RiskProposalInput {
    pub proposal_id: String,
    pub strategy_id: String,
    pub symbol: String,
    pub venue: String,
    pub currency: String,
    pub side: SideInput,
    pub quantity_micros: u64,
    pub limit_price_micros: u64,
    pub execution_mode: ExecutionModeInput,
    pub trading_day: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Schema, Serialize, Deserialize)]
pub struct RiskContextInput {
    pub now_ns: u64,
    pub market_data_at_ns: u64,
    pub venue_time_utc_sec: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Schema, Serialize, Deserialize)]
pub struct AuthorizeRiskInput {
    pub proposal: RiskProposalInput,
    pub context: RiskContextInput,
    pub portfolio: PortfolioRiskInput,
}

#[derive(Debug, Clone, PartialEq, Eq, Schema, Serialize, Deserialize)]
pub struct RefreshPortfolioInput {
    pub portfolio: PortfolioRiskInput,
    pub covered_client_order_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Schema, Serialize, Deserialize)]
pub enum RiskDecisionOutput {
    Approved { intent: OrderIntentInput },
    Rejected { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Schema, Serialize, Deserialize)]
pub struct RiskStatusOutput {
    pub account_id: String,
    pub configuration_fingerprint: String,
    pub policy_version: String,
    pub portfolio_as_of_ns: u64,
    pub trading_day: i32,
    pub gross_exposure_micros: u64,
    pub reserved_gross_micros: u64,
    pub reserved_order_count: u32,
    pub outstanding_reservations: u64,
    pub kill_switch_active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Schema, Serialize, Deserialize)]
pub enum RiskAgentError {
    NotConfigured {
        detail: String,
    },
    InvalidConfiguration {
        detail: String,
    },
    ConfigurationConflict {
        existing_fingerprint: String,
        proposed_fingerprint: String,
    },
    InvalidPortfolio {
        detail: String,
    },
    AuthorityRejected {
        detail: String,
    },
}

#[agent_definition(snapshotting = "periodic(30s)")]
pub trait RiskAccountAgent {
    /// Account identity is stable across policy refreshes and process restarts.
    fn new(account_id: String) -> Self;

    /// Initial configuration is one-time and replay-safe. A different policy requires an explicit
    /// migration method or a new account, never an accidental constructor identity.
    fn configure(&mut self, input: ConfigureRiskInput) -> Result<RiskStatusOutput, RiskAgentError>;

    fn authorize(
        &mut self,
        input: AuthorizeRiskInput,
    ) -> Result<RiskDecisionOutput, RiskAgentError>;

    fn refresh_portfolio(
        &mut self,
        input: RefreshPortfolioInput,
    ) -> Result<RiskStatusOutput, RiskAgentError>;

    fn set_kill_switch(&mut self, active: bool) -> Result<RiskStatusOutput, RiskAgentError>;

    fn status(&self) -> Result<RiskStatusOutput, RiskAgentError>;
}

struct RiskAccountAgentImpl {
    account_id: String,
    configuration_fingerprint: Option<String>,
    authority: Option<RiskAuthority<VenueSchedule>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AgentSnapshot {
    format_version: u32,
    account_id: String,
    configuration_fingerprint: Option<String>,
    authority: Option<RiskAuthoritySnapshot<VenueSchedule>>,
}

#[agent_implementation]
impl RiskAccountAgent for RiskAccountAgentImpl {
    fn new(account_id: String) -> Self {
        Self {
            account_id,
            configuration_fingerprint: None,
            authority: None,
        }
    }

    fn configure(&mut self, input: ConfigureRiskInput) -> Result<RiskStatusOutput, RiskAgentError> {
        if self.account_id.trim().is_empty() {
            return Err(invalid_configuration("account identity must not be empty"));
        }
        let policy: RiskPolicy = serde_json::from_str(&input.risk_policy_json)
            .map_err(|error| invalid_configuration(error.to_string()))?;
        let schedule: VenueSchedule = serde_json::from_str(&input.venue_schedule_json)
            .map_err(|error| invalid_configuration(error.to_string()))?;
        schedule
            .validate()
            .map_err(|error| invalid_configuration(error.to_string()))?;
        let portfolio = portfolio(input.initial_portfolio)?;
        let canonical = serde_json::to_vec(&(policy.clone(), schedule.clone(), portfolio.clone()))
            .map_err(|error| invalid_configuration(error.to_string()))?;
        let fingerprint = hex::encode(Sha256::digest(canonical));
        if let Some(existing) = &self.configuration_fingerprint {
            if existing == &fingerprint {
                return self.status();
            }
            return Err(RiskAgentError::ConfigurationConflict {
                existing_fingerprint: existing.clone(),
                proposed_fingerprint: fingerprint,
            });
        }
        self.authority = Some(RiskAuthority::new(policy, portfolio, schedule));
        self.configuration_fingerprint = Some(fingerprint);
        self.status()
    }

    fn authorize(
        &mut self,
        input: AuthorizeRiskInput,
    ) -> Result<RiskDecisionOutput, RiskAgentError> {
        let portfolio = portfolio(input.portfolio)?;
        let authority = self.authority_mut()?;
        authority
            .refresh_portfolio(portfolio)
            .map_err(authority_error)?;
        authority
            .authorize(input.proposal.into(), input.context.into())
            .map_err(authority_error)
            .map(RiskDecisionOutput::from)
    }

    fn refresh_portfolio(
        &mut self,
        input: RefreshPortfolioInput,
    ) -> Result<RiskStatusOutput, RiskAgentError> {
        let portfolio = portfolio(input.portfolio)?;
        if input.covered_client_order_ids.is_empty() {
            self.authority_mut()?
                .refresh_portfolio(portfolio)
                .map_err(authority_error)?;
        } else {
            self.authority_mut()?
                .refresh_portfolio_covering(portfolio, input.covered_client_order_ids)
                .map_err(authority_error)?;
        }
        self.status()
    }

    fn set_kill_switch(&mut self, active: bool) -> Result<RiskStatusOutput, RiskAgentError> {
        self.authority_mut()?.set_kill_switch(active);
        self.status()
    }

    fn status(&self) -> Result<RiskStatusOutput, RiskAgentError> {
        let authority = self.authority_ref()?;
        let portfolio = authority.portfolio();
        Ok(RiskStatusOutput {
            account_id: self.account_id.clone(),
            configuration_fingerprint: self
                .configuration_fingerprint
                .clone()
                .ok_or_else(not_configured)?,
            policy_version: authority.policy().version.clone(),
            portfolio_as_of_ns: portfolio.as_of_ns,
            trading_day: portfolio.trading_day,
            gross_exposure_micros: portfolio.gross_exposure.0,
            reserved_gross_micros: authority.reserved_gross().0,
            reserved_order_count: authority.reserved_order_count(),
            outstanding_reservations: u64::try_from(authority.outstanding_reservation_count())
                .map_err(|_| invalid_portfolio("reservation count exceeded u64"))?,
            kill_switch_active: authority.kill_switch_active(),
        })
    }

    async fn save_snapshot(&self) -> Result<Vec<u8>, String> {
        serde_json::to_vec(&AgentSnapshot {
            format_version: SNAPSHOT_FORMAT_VERSION,
            account_id: self.account_id.clone(),
            configuration_fingerprint: self.configuration_fingerprint.clone(),
            authority: self.authority.as_ref().map(RiskAuthority::snapshot),
        })
        .map_err(|error| format!("failed to encode risk account snapshot: {error}"))
    }

    async fn load_snapshot(&mut self, bytes: Vec<u8>) -> Result<(), String> {
        let snapshot: AgentSnapshot = serde_json::from_slice(&bytes)
            .map_err(|error| format!("failed to decode risk account snapshot: {error}"))?;
        if snapshot.format_version != SNAPSHOT_FORMAT_VERSION {
            return Err(format!(
                "unsupported risk snapshot version {}; expected {}",
                snapshot.format_version, SNAPSHOT_FORMAT_VERSION
            ));
        }
        if snapshot.account_id != self.account_id {
            return Err("risk snapshot account does not match agent identity".into());
        }
        if snapshot.configuration_fingerprint.is_some() != snapshot.authority.is_some() {
            return Err("risk snapshot configuration and authority are inconsistent".into());
        }
        let authority = snapshot
            .authority
            .map(|snapshot| {
                let restored = RiskAuthority::try_from_snapshot(snapshot)
                    .map_err(|error| format!("risk snapshot failed validation: {error}"))?;
                restored
                    .venue_sessions()
                    .validate()
                    .map_err(|error| format!("risk venue schedule failed validation: {error}"))?;
                Ok::<_, String>(restored)
            })
            .transpose()?;
        self.configuration_fingerprint = snapshot.configuration_fingerprint;
        self.authority = authority;
        Ok(())
    }
}

impl RiskAccountAgentImpl {
    fn authority_ref(&self) -> Result<&RiskAuthority<VenueSchedule>, RiskAgentError> {
        self.authority.as_ref().ok_or_else(not_configured)
    }

    fn authority_mut(&mut self) -> Result<&mut RiskAuthority<VenueSchedule>, RiskAgentError> {
        self.authority.as_mut().ok_or_else(not_configured)
    }
}

impl From<RiskProposalInput> for OrderProposal {
    fn from(input: RiskProposalInput) -> Self {
        Self {
            proposal_id: input.proposal_id,
            strategy_id: input.strategy_id,
            symbol: input.symbol,
            venue: input.venue,
            currency: input.currency,
            side: input.side.into(),
            quantity: QuantityMicros(input.quantity_micros),
            limit_price: PriceMicros(input.limit_price_micros),
            mode: match input.execution_mode {
                ExecutionModeInput::Paper => ExecutionMode::Paper,
                ExecutionModeInput::Live => ExecutionMode::Live,
            },
            trading_day: input.trading_day,
        }
    }
}

impl From<RiskContextInput> for RiskContext {
    fn from(input: RiskContextInput) -> Self {
        Self {
            now_ns: input.now_ns,
            market_data_at_ns: input.market_data_at_ns,
            venue_time_utc_sec: input.venue_time_utc_sec,
        }
    }
}

impl From<RiskDecision> for RiskDecisionOutput {
    fn from(decision: RiskDecision) -> Self {
        match decision {
            RiskDecision::Approved(intent) => Self::Approved {
                intent: (*intent).into(),
            },
            RiskDecision::Rejected(reason) => Self::Rejected {
                reason: risk_rejection_code(reason).into(),
            },
        }
    }
}

fn portfolio(input: PortfolioRiskInput) -> Result<PortfolioRiskSnapshot, RiskAgentError> {
    let mut strategy_exposure = BTreeMap::new();
    for entry in input.strategy_exposure {
        if entry.name.trim().is_empty()
            || strategy_exposure
                .insert(entry.name, MoneyMicros(entry.amount_micros))
                .is_some()
        {
            return Err(invalid_portfolio(
                "strategy exposure names must be non-empty and unique",
            ));
        }
    }
    let mut symbol_positions_micros = BTreeMap::new();
    for entry in input.symbol_positions {
        if entry.symbol.trim().is_empty()
            || symbol_positions_micros
                .insert(entry.symbol, i128::from(entry.quantity_micros))
                .is_some()
        {
            return Err(invalid_portfolio(
                "symbol position names must be non-empty and unique",
            ));
        }
    }
    Ok(PortfolioRiskSnapshot {
        as_of_ns: input.as_of_ns,
        trading_day: input.trading_day,
        gross_exposure: MoneyMicros(input.gross_exposure_micros),
        strategy_exposure,
        symbol_positions_micros,
        daily_order_count: input.daily_order_count,
    })
}

fn risk_rejection_code(reason: RiskRejection) -> &'static str {
    match reason {
        RiskRejection::KillSwitchActive => "kill_switch_active",
        RiskRejection::LiveExecutionDisabled => "live_execution_disabled",
        RiskRejection::VenueNotAllowed => "venue_not_allowed",
        RiskRejection::VenueSessionClosed => "venue_session_closed",
        RiskRejection::VenueCalendarUnavailable => "venue_calendar_unavailable",
        RiskRejection::StaleMarketData => "stale_market_data",
        RiskRejection::StalePortfolio => "stale_portfolio",
        RiskRejection::TradingDayMismatch => "trading_day_mismatch",
        RiskRejection::OrderNotionalLimit => "order_notional_limit",
        RiskRejection::GrossExposureLimit => "gross_exposure_limit",
        RiskRejection::StrategyExposureLimit => "strategy_exposure_limit",
        RiskRejection::SymbolPositionLimit => "symbol_position_limit",
        RiskRejection::DailyOrderLimit => "daily_order_limit",
        RiskRejection::ZeroOrderValue => "zero_order_value",
        RiskRejection::InvalidProposal => "invalid_proposal",
        RiskRejection::ArithmeticOverflow => "arithmetic_overflow",
    }
}

fn not_configured() -> RiskAgentError {
    RiskAgentError::NotConfigured {
        detail: "durable risk authority has not been configured".into(),
    }
}

fn invalid_configuration(detail: impl Into<String>) -> RiskAgentError {
    RiskAgentError::InvalidConfiguration {
        detail: detail.into(),
    }
}

fn invalid_portfolio(detail: impl Into<String>) -> RiskAgentError {
    RiskAgentError::InvalidPortfolio {
        detail: detail.into(),
    }
}

fn authority_error(error: impl ToString) -> RiskAgentError {
    RiskAgentError::AuthorityRejected {
        detail: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use helio_scan::SessionDate;
    use helio_time::{VenueScheduleMetadata, VenueSession, compute_source_sha256};
    use std::collections::BTreeSet;

    fn schedule() -> VenueSchedule {
        let sessions = vec![VenueSession {
            label: SessionDate(1),
            open_utc: 9_500,
            close_utc: 10_500,
            breaks: Vec::new(),
        }];
        let mut metadata = VenueScheduleMetadata {
            schema_version: 1,
            venue: "XNYS".into(),
            timezone: "America/New_York".into(),
            source: "test".into(),
            source_version: "1".into(),
            source_sha256: "0".repeat(64),
            generated_at_utc: 9_000,
            valid_from_utc: 9_000,
            valid_until_utc: 11_000,
        };
        metadata.source_sha256 = compute_source_sha256(&metadata, &sessions).unwrap();
        VenueSchedule::try_new(metadata, sessions).unwrap()
    }

    fn policy() -> RiskPolicy {
        RiskPolicy {
            version: "paper-v1".into(),
            live_enabled: false,
            allowed_venues: BTreeSet::from(["XNYS".into()]),
            max_market_data_age_ns: 1_000,
            max_portfolio_age_ns: 1_000,
            max_order_notional: MoneyMicros(100_000_000),
            max_gross_exposure: MoneyMicros(1_000_000_000),
            max_strategy_exposure: MoneyMicros(500_000_000),
            max_symbol_position_micros: 10_000_000,
            max_daily_orders: 10,
        }
    }

    fn portfolio_input() -> PortfolioRiskInput {
        PortfolioRiskInput {
            as_of_ns: 10_000_000_000_000 - 100,
            trading_day: 1,
            gross_exposure_micros: 0,
            strategy_exposure: Vec::new(),
            symbol_positions: Vec::new(),
            daily_order_count: 0,
        }
    }

    fn configure() -> ConfigureRiskInput {
        ConfigureRiskInput {
            risk_policy_json: serde_json::to_string(&policy()).unwrap(),
            venue_schedule_json: serde_json::to_string(&schedule()).unwrap(),
            initial_portfolio: portfolio_input(),
        }
    }

    #[test]
    fn durable_risk_agent_is_config_replay_safe_and_reserves_once() {
        let mut agent = RiskAccountAgentImpl::new("paper-account".into());
        let first = agent.configure(configure()).unwrap();
        assert_eq!(agent.configure(configure()).unwrap(), first);
        let input = AuthorizeRiskInput {
            proposal: RiskProposalInput {
                proposal_id: "order-1".into(),
                strategy_id: "manual".into(),
                symbol: "SPY".into(),
                venue: "XNYS".into(),
                currency: "USD".into(),
                side: SideInput::Buy,
                quantity_micros: 1_000_000,
                limit_price_micros: 25_000_000,
                execution_mode: ExecutionModeInput::Paper,
                trading_day: 1,
            },
            context: RiskContextInput {
                now_ns: 10_000_000_000_000,
                market_data_at_ns: 10_000_000_000_000 - 100,
                venue_time_utc_sec: 10_000,
            },
            portfolio: portfolio_input(),
        };
        let first = agent.authorize(input.clone()).unwrap();
        assert_eq!(agent.authorize(input).unwrap(), first);
        assert!(matches!(first, RiskDecisionOutput::Approved { .. }));
        assert_eq!(agent.status().unwrap().outstanding_reservations, 1);
    }
}
