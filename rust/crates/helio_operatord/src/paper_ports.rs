use helio_execution::{
    OrderProposal, PortfolioRiskSnapshot, RiskAuthority, RiskContext, RiskDecision, RiskPolicy,
};
use helio_oms::{
    CommandReceipt, OmsCommand, OmsCommandPort, OmsError, OmsEventEnvelope, OmsEventSource,
    OmsQueryPort, OrderSnapshot, ReferenceOms,
};
use helio_time::VenueSchedule;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RiskPortStatus {
    pub account_id: String,
    pub policy_version: String,
    pub portfolio_as_of_ns: u64,
    pub trading_day: i32,
    pub gross_exposure_micros: u64,
    pub reserved_gross_micros: u64,
    pub outstanding_reservations: u64,
    pub kill_switch_active: bool,
}

/// Account-scoped OMS authority used by the paper executor.
///
/// Implementations may be in-process for deterministic tests or a durable Golem agent in an
/// operational deployment. Broker I/O and message delivery do not belong in this port.
pub trait PaperOmsPort: Send {
    fn account_id(&self) -> &str;
    fn execute(&mut self, command: OmsCommand) -> Result<CommandReceipt, String>;
    fn order(&self, client_order_id: &str) -> Result<Option<OrderSnapshot>, String>;
    fn orders(&self, limit: usize) -> Result<Vec<OrderSnapshot>, String>;
    fn events_after(&self, cursor: u64, limit: usize) -> Result<Vec<OmsEventEnvelope>, String>;
}

impl PaperOmsPort for ReferenceOms {
    fn account_id(&self) -> &str {
        ReferenceOms::account_id(self)
    }

    fn execute(&mut self, command: OmsCommand) -> Result<CommandReceipt, String> {
        OmsCommandPort::execute(self, command).map_err(|error| error.to_string())
    }

    fn order(&self, client_order_id: &str) -> Result<Option<OrderSnapshot>, String> {
        OmsQueryPort::order(self, client_order_id).map_err(|error| error.to_string())
    }

    fn orders(&self, limit: usize) -> Result<Vec<OrderSnapshot>, String> {
        OmsQueryPort::orders(self, limit).map_err(|error| error.to_string())
    }

    fn events_after(&self, cursor: u64, limit: usize) -> Result<Vec<OmsEventEnvelope>, String> {
        OmsEventSource::events_after(self, cursor, limit).map_err(|error| error.to_string())
    }
}

/// Account-scoped risk authority. Authorization accepts the broker portfolio in the same call so
/// a durable implementation can refresh truth and reserve the approved order atomically.
pub trait PaperRiskPort: Send {
    fn account_id(&self) -> &str;
    fn authorize(
        &mut self,
        proposal: OrderProposal,
        context: RiskContext,
        portfolio: PortfolioRiskSnapshot,
    ) -> Result<RiskDecision, String>;
    fn refresh_portfolio(
        &mut self,
        portfolio: PortfolioRiskSnapshot,
        covered_client_order_ids: &[String],
    ) -> Result<RiskPortStatus, String>;
    fn set_kill_switch(&mut self, active: bool) -> Result<RiskPortStatus, String>;
    fn status(&self) -> Result<RiskPortStatus, String>;
}

pub struct LocalRiskPort {
    account_id: String,
    authority: RiskAuthority<VenueSchedule>,
}

impl LocalRiskPort {
    pub fn new(
        account_id: impl Into<String>,
        policy: RiskPolicy,
        portfolio: PortfolioRiskSnapshot,
        schedule: VenueSchedule,
    ) -> Result<Self, OmsError> {
        let account_id = account_id.into();
        if account_id.trim().is_empty() {
            return Err(OmsError::EmptyIdentity);
        }
        Ok(Self {
            account_id,
            authority: RiskAuthority::new(policy, portfolio, schedule),
        })
    }

    fn current_status(&self) -> Result<RiskPortStatus, String> {
        let portfolio = self.authority.portfolio();
        Ok(RiskPortStatus {
            account_id: self.account_id.clone(),
            policy_version: self.authority.policy().version.clone(),
            portfolio_as_of_ns: portfolio.as_of_ns,
            trading_day: portfolio.trading_day,
            gross_exposure_micros: portfolio.gross_exposure.0,
            reserved_gross_micros: self.authority.reserved_gross().0,
            outstanding_reservations: u64::try_from(self.authority.outstanding_reservation_count())
                .map_err(|_| "risk reservation count exceeded u64".to_owned())?,
            kill_switch_active: self.authority.kill_switch_active(),
        })
    }
}

impl PaperRiskPort for LocalRiskPort {
    fn account_id(&self) -> &str {
        &self.account_id
    }

    fn authorize(
        &mut self,
        proposal: OrderProposal,
        context: RiskContext,
        portfolio: PortfolioRiskSnapshot,
    ) -> Result<RiskDecision, String> {
        self.authority
            .refresh_portfolio(portfolio)
            .map_err(|error| error.to_string())?;
        self.authority
            .authorize(proposal, context)
            .map_err(|error| error.to_string())
    }

    fn refresh_portfolio(
        &mut self,
        portfolio: PortfolioRiskSnapshot,
        covered_client_order_ids: &[String],
    ) -> Result<RiskPortStatus, String> {
        if covered_client_order_ids.is_empty() {
            self.authority
                .refresh_portfolio(portfolio)
                .map_err(|error| error.to_string())?;
        } else {
            self.authority
                .refresh_portfolio_covering(portfolio, covered_client_order_ids)
                .map_err(|error| error.to_string())?;
        }
        self.current_status()
    }

    fn set_kill_switch(&mut self, active: bool) -> Result<RiskPortStatus, String> {
        self.authority.set_kill_switch(active);
        self.current_status()
    }

    fn status(&self) -> Result<RiskPortStatus, String> {
        self.current_status()
    }
}
