use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use helio_time::VenueSchedule;

use crate::{checked_notional, ExecutionMode, MoneyMicros, OrderIntent, OrderProposal};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RiskPolicy {
    pub version: String,
    pub live_enabled: bool,
    pub allowed_venues: BTreeSet<String>,
    pub max_market_data_age_ns: u64,
    pub max_portfolio_age_ns: u64,
    pub max_order_notional: MoneyMicros,
    pub max_gross_exposure: MoneyMicros,
    pub max_strategy_exposure: MoneyMicros,
    pub max_symbol_position_micros: u64,
    pub max_daily_orders: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortfolioRiskSnapshot {
    pub as_of_ns: u64,
    pub trading_day: i32,
    pub gross_exposure: MoneyMicros,
    pub strategy_exposure: BTreeMap<String, MoneyMicros>,
    pub symbol_positions_micros: BTreeMap<String, i128>,
    pub daily_order_count: u32,
}

impl PortfolioRiskSnapshot {
    pub fn empty(as_of_ns: u64, trading_day: i32) -> Self {
        Self {
            as_of_ns,
            trading_day,
            gross_exposure: MoneyMicros(0),
            strategy_exposure: BTreeMap::new(),
            symbol_positions_micros: BTreeMap::new(),
            daily_order_count: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RiskContext {
    pub now_ns: u64,
    pub market_data_at_ns: u64,
    pub venue_time_utc_sec: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum VenueSessionAuthorityError {
    #[error("requested venue does not match the loaded schedule")]
    VenueMismatch,
    #[error("venue calendar could not answer the requested timestamp")]
    CalendarUnavailable,
}

pub trait VenueSessionAuthority {
    fn active_session_label(
        &self,
        venue: &str,
        timestamp_utc_sec: i64,
    ) -> Result<Option<i32>, VenueSessionAuthorityError>;
}

impl VenueSessionAuthority for VenueSchedule {
    fn active_session_label(
        &self,
        venue: &str,
        timestamp_utc_sec: i64,
    ) -> Result<Option<i32>, VenueSessionAuthorityError> {
        if self.metadata.venue != venue {
            return Err(VenueSessionAuthorityError::VenueMismatch);
        }
        self.active_session_at(timestamp_utc_sec)
            .map(|session| session.map(|value| value.label.0))
            .map_err(|_| VenueSessionAuthorityError::CalendarUnavailable)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RiskRejection {
    KillSwitchActive,
    LiveExecutionDisabled,
    VenueNotAllowed,
    VenueSessionClosed,
    VenueCalendarUnavailable,
    StaleMarketData,
    StalePortfolio,
    TradingDayMismatch,
    OrderNotionalLimit,
    GrossExposureLimit,
    StrategyExposureLimit,
    SymbolPositionLimit,
    DailyOrderLimit,
    InvalidProposal,
    ArithmeticOverflow,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RiskDecision {
    Approved(Box<OrderIntent>),
    Rejected(RiskRejection),
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RiskAuthorityError {
    #[error("proposal identity was replayed with different contents")]
    ProposalIdentityConflict,
    #[error("portfolio snapshot is older than the latest accepted snapshot")]
    SnapshotRegression,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RecordedDecision {
    proposal: OrderProposal,
    decision: RiskDecision,
}

/// Stateful pre-trade authority. Decisions are idempotent by proposal identity.
#[derive(Debug, Clone)]
pub struct RiskAuthority<V> {
    policy: RiskPolicy,
    portfolio: PortfolioRiskSnapshot,
    venue_sessions: V,
    kill_switch: bool,
    decisions: BTreeMap<String, RecordedDecision>,
    reserved_gross: u64,
    reserved_by_strategy: BTreeMap<String, u64>,
    reserved_position_delta: BTreeMap<String, i128>,
    reserved_order_count: u32,
}

impl<V> RiskAuthority<V>
where
    V: VenueSessionAuthority,
{
    pub fn new(policy: RiskPolicy, portfolio: PortfolioRiskSnapshot, venue_sessions: V) -> Self {
        Self {
            policy,
            portfolio,
            venue_sessions,
            kill_switch: false,
            decisions: BTreeMap::new(),
            reserved_gross: 0,
            reserved_by_strategy: BTreeMap::new(),
            reserved_position_delta: BTreeMap::new(),
            reserved_order_count: 0,
        }
    }

    pub const fn policy(&self) -> &RiskPolicy {
        &self.policy
    }

    pub const fn kill_switch_active(&self) -> bool {
        self.kill_switch
    }

    pub fn set_kill_switch(&mut self, active: bool) {
        self.kill_switch = active;
    }

    /// Replace positions and realized exposure from an authoritative portfolio source.
    /// Outstanding reservations remain applied on top of the new snapshot.
    pub fn refresh_portfolio(
        &mut self,
        portfolio: PortfolioRiskSnapshot,
    ) -> Result<(), RiskAuthorityError> {
        if portfolio.as_of_ns < self.portfolio.as_of_ns {
            return Err(RiskAuthorityError::SnapshotRegression);
        }
        self.portfolio = portfolio;
        Ok(())
    }

    pub fn authorize(
        &mut self,
        proposal: OrderProposal,
        context: RiskContext,
    ) -> Result<RiskDecision, RiskAuthorityError> {
        if let Some(recorded) = self.decisions.get(&proposal.proposal_id) {
            if recorded.proposal == proposal {
                return Ok(recorded.decision.clone());
            }
            return Err(RiskAuthorityError::ProposalIdentityConflict);
        }

        let mut decision = self.evaluate(&proposal, context);
        if let RiskDecision::Approved(intent) = &decision {
            if let Err(reason) = self.try_reserve(intent) {
                decision = RiskDecision::Rejected(reason);
            }
        }
        self.decisions.insert(
            proposal.proposal_id.clone(),
            RecordedDecision {
                proposal,
                decision: decision.clone(),
            },
        );
        Ok(decision)
    }

    fn evaluate(&self, proposal: &OrderProposal, context: RiskContext) -> RiskDecision {
        let reject = |reason| RiskDecision::Rejected(reason);
        if proposal.proposal_id.trim().is_empty()
            || proposal.strategy_id.trim().is_empty()
            || proposal.symbol.trim().is_empty()
            || proposal.venue.trim().is_empty()
            || proposal.currency.trim().is_empty()
        {
            return reject(RiskRejection::InvalidProposal);
        }
        if self.kill_switch {
            return reject(RiskRejection::KillSwitchActive);
        }
        if proposal.mode == ExecutionMode::Live && !self.policy.live_enabled {
            return reject(RiskRejection::LiveExecutionDisabled);
        }
        if !self.policy.allowed_venues.contains(&proposal.venue) {
            return reject(RiskRejection::VenueNotAllowed);
        }
        match self
            .venue_sessions
            .active_session_label(&proposal.venue, context.venue_time_utc_sec)
        {
            Ok(Some(label)) if label == proposal.trading_day => {}
            Ok(Some(_)) => return reject(RiskRejection::TradingDayMismatch),
            Ok(None) => return reject(RiskRejection::VenueSessionClosed),
            Err(_) => return reject(RiskRejection::VenueCalendarUnavailable),
        }
        let Some(data_age) = context.now_ns.checked_sub(context.market_data_at_ns) else {
            return reject(RiskRejection::StaleMarketData);
        };
        if data_age > self.policy.max_market_data_age_ns {
            return reject(RiskRejection::StaleMarketData);
        }
        let Some(portfolio_age) = context.now_ns.checked_sub(self.portfolio.as_of_ns) else {
            return reject(RiskRejection::StalePortfolio);
        };
        if portfolio_age > self.policy.max_portfolio_age_ns {
            return reject(RiskRejection::StalePortfolio);
        }
        if proposal.trading_day != self.portfolio.trading_day {
            return reject(RiskRejection::TradingDayMismatch);
        }

        let Ok(notional) = checked_notional(proposal.limit_price, proposal.quantity) else {
            return reject(RiskRejection::ArithmeticOverflow);
        };
        if notional > self.policy.max_order_notional {
            return reject(RiskRejection::OrderNotionalLimit);
        }
        let Some(projected_gross) = self
            .portfolio
            .gross_exposure
            .0
            .checked_add(self.reserved_gross)
            .and_then(|value| value.checked_add(notional.0))
        else {
            return reject(RiskRejection::ArithmeticOverflow);
        };
        if projected_gross > self.policy.max_gross_exposure.0 {
            return reject(RiskRejection::GrossExposureLimit);
        }
        let strategy_current = self
            .portfolio
            .strategy_exposure
            .get(&proposal.strategy_id)
            .copied()
            .unwrap_or(MoneyMicros(0))
            .0;
        let strategy_reserved = self
            .reserved_by_strategy
            .get(&proposal.strategy_id)
            .copied()
            .unwrap_or(0);
        let Some(projected_strategy) = strategy_current
            .checked_add(strategy_reserved)
            .and_then(|value| value.checked_add(notional.0))
        else {
            return reject(RiskRejection::ArithmeticOverflow);
        };
        if projected_strategy > self.policy.max_strategy_exposure.0 {
            return reject(RiskRejection::StrategyExposureLimit);
        }

        let position = self
            .portfolio
            .symbol_positions_micros
            .get(&proposal.symbol)
            .copied()
            .unwrap_or(0);
        let reserved_delta = self
            .reserved_position_delta
            .get(&proposal.symbol)
            .copied()
            .unwrap_or(0);
        let Some(projected_position) = position
            .checked_add(reserved_delta)
            .and_then(|value| value.checked_add(proposal.side.signed_quantity(proposal.quantity)))
        else {
            return reject(RiskRejection::ArithmeticOverflow);
        };
        if projected_position.unsigned_abs() > u128::from(self.policy.max_symbol_position_micros) {
            return reject(RiskRejection::SymbolPositionLimit);
        }
        let Some(projected_count) = self
            .portfolio
            .daily_order_count
            .checked_add(self.reserved_order_count)
            .and_then(|value| value.checked_add(1))
        else {
            return reject(RiskRejection::ArithmeticOverflow);
        };
        if projected_count > self.policy.max_daily_orders {
            return reject(RiskRejection::DailyOrderLimit);
        }

        RiskDecision::Approved(Box::new(OrderIntent {
            client_order_id: proposal.proposal_id.clone(),
            proposal: proposal.clone(),
            authorized_notional: notional,
            risk_policy_version: self.policy.version.clone(),
            authorized_at_ns: context.now_ns,
        }))
    }

    fn try_reserve(&mut self, intent: &OrderIntent) -> Result<(), RiskRejection> {
        let reserved_gross = self
            .reserved_gross
            .checked_add(intent.authorized_notional.0)
            .ok_or(RiskRejection::ArithmeticOverflow)?;
        let strategy_reservation = self
            .reserved_by_strategy
            .get(&intent.proposal.strategy_id)
            .copied()
            .unwrap_or(0)
            .checked_add(intent.authorized_notional.0)
            .ok_or(RiskRejection::ArithmeticOverflow)?;
        let position_reservation = self
            .reserved_position_delta
            .get(&intent.proposal.symbol)
            .copied()
            .unwrap_or(0)
            .checked_add(
                intent
                    .proposal
                    .side
                    .signed_quantity(intent.proposal.quantity),
            )
            .ok_or(RiskRejection::ArithmeticOverflow)?;
        let reserved_order_count = self
            .reserved_order_count
            .checked_add(1)
            .ok_or(RiskRejection::ArithmeticOverflow)?;

        self.reserved_gross = reserved_gross;
        self.reserved_by_strategy
            .insert(intent.proposal.strategy_id.clone(), strategy_reservation);
        self.reserved_position_delta
            .insert(intent.proposal.symbol.clone(), position_reservation);
        self.reserved_order_count = reserved_order_count;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PriceMicros, QuantityMicros, Side};

    #[derive(Debug, Clone, Copy)]
    struct TestVenue(Option<i32>);

    impl VenueSessionAuthority for TestVenue {
        fn active_session_label(
            &self,
            venue: &str,
            _timestamp_utc_sec: i64,
        ) -> Result<Option<i32>, VenueSessionAuthorityError> {
            if venue == "XNYS" {
                Ok(self.0)
            } else {
                Err(VenueSessionAuthorityError::VenueMismatch)
            }
        }
    }

    fn policy() -> RiskPolicy {
        RiskPolicy {
            version: "risk-2026-08-30".into(),
            live_enabled: true,
            allowed_venues: BTreeSet::from(["XNYS".into()]),
            max_market_data_age_ns: 1_000,
            max_portfolio_age_ns: 2_000,
            max_order_notional: MoneyMicros(10_000_000),
            max_gross_exposure: MoneyMicros(15_000_000),
            max_strategy_exposure: MoneyMicros(12_000_000),
            max_symbol_position_micros: 2_000_000,
            max_daily_orders: 2,
        }
    }

    fn proposal(id: &str, quantity: u64) -> OrderProposal {
        OrderProposal {
            proposal_id: id.into(),
            strategy_id: "space-weather-v1".into(),
            symbol: "GRID".into(),
            venue: "XNYS".into(),
            currency: "USD".into(),
            side: Side::Buy,
            quantity: QuantityMicros(quantity),
            limit_price: PriceMicros(5_000_000),
            mode: ExecutionMode::Live,
            trading_day: 20_696,
        }
    }

    fn context() -> RiskContext {
        RiskContext {
            now_ns: 10_000,
            market_data_at_ns: 9_500,
            venue_time_utc_sec: 1_000,
        }
    }

    #[test]
    fn approvals_reserve_limits_and_are_idempotent() {
        let mut authority = RiskAuthority::new(
            policy(),
            PortfolioRiskSnapshot::empty(9_000, 20_696),
            TestVenue(Some(20_696)),
        );
        let first = authority
            .authorize(proposal("one", 1_000_000), context())
            .unwrap();
        assert!(matches!(first, RiskDecision::Approved(_)));
        assert_eq!(
            authority.authorize(proposal("one", 1_000_000), context()),
            Ok(first)
        );
        assert_eq!(
            authority.authorize(proposal("two", 1_100_000), context()),
            Ok(RiskDecision::Rejected(RiskRejection::SymbolPositionLimit))
        );
    }

    #[test]
    fn stale_closed_and_killed_paths_fail_closed() {
        let mut authority = RiskAuthority::new(
            policy(),
            PortfolioRiskSnapshot::empty(9_000, 20_696),
            TestVenue(Some(20_696)),
        );
        let mut stale = context();
        stale.market_data_at_ns = 1;
        assert_eq!(
            authority.authorize(proposal("stale", 1_000_000), stale),
            Ok(RiskDecision::Rejected(RiskRejection::StaleMarketData))
        );
        let mut closed = RiskAuthority::new(
            policy(),
            PortfolioRiskSnapshot::empty(9_000, 20_696),
            TestVenue(None),
        );
        assert_eq!(
            closed.authorize(proposal("closed", 1_000_000), context()),
            Ok(RiskDecision::Rejected(RiskRejection::VenueSessionClosed))
        );
        authority.set_kill_switch(true);
        assert_eq!(
            authority.authorize(proposal("killed", 1_000_000), context()),
            Ok(RiskDecision::Rejected(RiskRejection::KillSwitchActive))
        );
    }

    #[test]
    fn proposal_identity_cannot_change_on_retry() {
        let mut authority = RiskAuthority::new(
            policy(),
            PortfolioRiskSnapshot::empty(9_000, 20_696),
            TestVenue(Some(20_696)),
        );
        authority
            .authorize(proposal("same", 1_000_000), context())
            .unwrap();
        assert_eq!(
            authority.authorize(proposal("same", 2_000_000), context()),
            Err(RiskAuthorityError::ProposalIdentityConflict)
        );
    }
}
