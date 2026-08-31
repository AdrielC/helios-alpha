use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use helio_time::VenueSchedule;

use crate::{
    checked_notional, ArithmeticError, ExecutionMode, MoneyMicros, OrderIntent, OrderProposal,
};

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
    ZeroOrderValue,
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
    #[error("portfolio snapshot claimed coverage of unknown reservation {0}")]
    UnknownCoveredReservation(String),
    #[error("risk reservation accounting invariant was violated")]
    ReservationAccountingCorrupt,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RecordedDecision {
    proposal: OrderProposal,
    decision: RiskDecision,
}

const RISK_AUTHORITY_SNAPSHOT_VERSION: u32 = 1;

/// Versioned durable representation of one account risk authority.
///
/// Fields remain private so callers cannot construct an unchecked authority. Persist the value
/// with `serde`, then restore it through [`RiskAuthority::try_from_snapshot`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RiskAuthoritySnapshot<V> {
    schema_version: u32,
    policy: RiskPolicy,
    portfolio: PortfolioRiskSnapshot,
    venue_sessions: V,
    kill_switch: bool,
    decisions: BTreeMap<String, RecordedDecision>,
    reservations: BTreeMap<String, RiskReservation>,
    reserved_gross: u64,
    reserved_by_strategy: BTreeMap<String, u64>,
    reserved_position_delta: BTreeMap<String, i128>,
    reserved_order_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RiskSnapshotError {
    #[error("risk snapshot schema version {0} is unsupported")]
    UnsupportedSchema(u32),
    #[error("risk snapshot contains an inconsistent decision identity")]
    DecisionIdentity,
    #[error("risk snapshot contains an inconsistent approved intent")]
    ApprovedIntent,
    #[error("risk snapshot contains an inconsistent reservation")]
    Reservation,
    #[error("risk snapshot reservation accounting does not balance")]
    ReservationAccounting,
    #[error("risk snapshot reservation accounting overflowed")]
    ArithmeticOverflow,
}

/// Exposure held between pre-trade authorization and authoritative portfolio reconciliation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RiskReservation {
    pub client_order_id: String,
    pub strategy_id: String,
    pub symbol: String,
    pub notional: MoneyMicros,
    pub position_delta_micros: i128,
}

/// Stateful pre-trade authority. Decisions are idempotent by proposal identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RiskAuthority<V> {
    policy: RiskPolicy,
    portfolio: PortfolioRiskSnapshot,
    venue_sessions: V,
    kill_switch: bool,
    decisions: BTreeMap<String, RecordedDecision>,
    reservations: BTreeMap<String, RiskReservation>,
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
            reservations: BTreeMap::new(),
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

    pub const fn portfolio(&self) -> &PortfolioRiskSnapshot {
        &self.portfolio
    }

    pub const fn venue_sessions(&self) -> &V {
        &self.venue_sessions
    }

    pub const fn reserved_gross(&self) -> MoneyMicros {
        MoneyMicros(self.reserved_gross)
    }

    pub const fn reserved_order_count(&self) -> u32 {
        self.reserved_order_count
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

    /// Replaces the portfolio and releases only reservations explicitly included in it.
    ///
    /// Every ID in `covered_client_order_ids` must refer to an approved decision. Outstanding
    /// reservations are released exactly once, while a repeated refresh for an already released
    /// approved ID is a no-op. This makes terminal broker reconciliation replay-safe across a crash
    /// after the durable risk update. Unknown and rejected IDs, snapshot regressions, and arithmetic
    /// inconsistencies reject the entire refresh without mutating authority state. Partial fills
    /// stay fully reserved until a later authoritative snapshot covers the terminal order.
    pub fn refresh_portfolio_covering<I, S>(
        &mut self,
        portfolio: PortfolioRiskSnapshot,
        covered_client_order_ids: I,
    ) -> Result<(), RiskAuthorityError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        if portfolio.as_of_ns < self.portfolio.as_of_ns {
            return Err(RiskAuthorityError::SnapshotRegression);
        }

        let covered = covered_client_order_ids
            .into_iter()
            .map(|value| value.as_ref().to_owned())
            .collect::<BTreeSet<_>>();
        for client_order_id in &covered {
            let was_approved = self
                .decisions
                .get(client_order_id)
                .is_some_and(|recorded| matches!(recorded.decision, RiskDecision::Approved(_)));
            if !was_approved {
                return Err(RiskAuthorityError::UnknownCoveredReservation(
                    client_order_id.clone(),
                ));
            }
        }

        let mut reservations = self.reservations.clone();
        let mut reserved_gross = self.reserved_gross;
        let mut reserved_by_strategy = self.reserved_by_strategy.clone();
        let mut reserved_position_delta = self.reserved_position_delta.clone();
        let mut reserved_order_count = self.reserved_order_count;

        for client_order_id in &covered {
            let Some(reservation) = reservations.remove(client_order_id) else {
                continue;
            };
            reserved_gross = reserved_gross
                .checked_sub(reservation.notional.0)
                .ok_or(RiskAuthorityError::ReservationAccountingCorrupt)?;
            subtract_reservation(
                &mut reserved_by_strategy,
                &reservation.strategy_id,
                reservation.notional.0,
            )?;
            subtract_signed_reservation(
                &mut reserved_position_delta,
                &reservation.symbol,
                reservation.position_delta_micros,
            )?;
            reserved_order_count = reserved_order_count
                .checked_sub(1)
                .ok_or(RiskAuthorityError::ReservationAccountingCorrupt)?;
        }

        self.portfolio = portfolio;
        self.reservations = reservations;
        self.reserved_gross = reserved_gross;
        self.reserved_by_strategy = reserved_by_strategy;
        self.reserved_position_delta = reserved_position_delta;
        self.reserved_order_count = reserved_order_count;
        Ok(())
    }

    pub fn reservation(&self, client_order_id: &str) -> Option<&RiskReservation> {
        self.reservations.get(client_order_id)
    }

    pub fn outstanding_reservation_count(&self) -> usize {
        self.reservations.len()
    }

    pub fn snapshot(&self) -> RiskAuthoritySnapshot<V>
    where
        V: Clone,
    {
        RiskAuthoritySnapshot {
            schema_version: RISK_AUTHORITY_SNAPSHOT_VERSION,
            policy: self.policy.clone(),
            portfolio: self.portfolio.clone(),
            venue_sessions: self.venue_sessions.clone(),
            kill_switch: self.kill_switch,
            decisions: self.decisions.clone(),
            reservations: self.reservations.clone(),
            reserved_gross: self.reserved_gross,
            reserved_by_strategy: self.reserved_by_strategy.clone(),
            reserved_position_delta: self.reserved_position_delta.clone(),
            reserved_order_count: self.reserved_order_count,
        }
    }

    pub fn try_from_snapshot(
        snapshot: RiskAuthoritySnapshot<V>,
    ) -> Result<Self, RiskSnapshotError> {
        if snapshot.schema_version != RISK_AUTHORITY_SNAPSHOT_VERSION {
            return Err(RiskSnapshotError::UnsupportedSchema(
                snapshot.schema_version,
            ));
        }
        for (identity, recorded) in &snapshot.decisions {
            if identity != &recorded.proposal.proposal_id {
                return Err(RiskSnapshotError::DecisionIdentity);
            }
            if let RiskDecision::Approved(intent) = &recorded.decision {
                let expected_notional =
                    checked_notional(recorded.proposal.limit_price, recorded.proposal.quantity)
                        .map_err(|_| RiskSnapshotError::ApprovedIntent)?;
                if intent.client_order_id != recorded.proposal.proposal_id
                    || intent.proposal != recorded.proposal
                    || intent.authorized_notional != expected_notional
                    || intent.risk_policy_version != snapshot.policy.version
                {
                    return Err(RiskSnapshotError::ApprovedIntent);
                }
            }
        }

        let mut reserved_gross = 0_u64;
        let mut reserved_by_strategy = BTreeMap::new();
        let mut reserved_position_delta = BTreeMap::new();
        for (identity, reservation) in &snapshot.reservations {
            let approved = snapshot.decisions.get(identity).and_then(|recorded| {
                if let RiskDecision::Approved(intent) = &recorded.decision {
                    Some(intent.as_ref())
                } else {
                    None
                }
            });
            let Some(intent) = approved else {
                return Err(RiskSnapshotError::Reservation);
            };
            if identity != &reservation.client_order_id
                || reservation.client_order_id != intent.client_order_id
                || reservation.strategy_id != intent.proposal.strategy_id
                || reservation.symbol != intent.proposal.symbol
                || reservation.notional != intent.authorized_notional
                || reservation.position_delta_micros
                    != intent
                        .proposal
                        .side
                        .signed_quantity(intent.proposal.quantity)
            {
                return Err(RiskSnapshotError::Reservation);
            }
            reserved_gross = reserved_gross
                .checked_add(reservation.notional.0)
                .ok_or(RiskSnapshotError::ArithmeticOverflow)?;
            add_unsigned(
                &mut reserved_by_strategy,
                &reservation.strategy_id,
                reservation.notional.0,
            )?;
            add_signed(
                &mut reserved_position_delta,
                &reservation.symbol,
                reservation.position_delta_micros,
            )?;
        }
        let reserved_order_count = u32::try_from(snapshot.reservations.len())
            .map_err(|_| RiskSnapshotError::ArithmeticOverflow)?;
        if reserved_gross != snapshot.reserved_gross
            || reserved_by_strategy != snapshot.reserved_by_strategy
            || reserved_position_delta != snapshot.reserved_position_delta
            || reserved_order_count != snapshot.reserved_order_count
        {
            return Err(RiskSnapshotError::ReservationAccounting);
        }

        Ok(Self {
            policy: snapshot.policy,
            portfolio: snapshot.portfolio,
            venue_sessions: snapshot.venue_sessions,
            kill_switch: snapshot.kill_switch,
            decisions: snapshot.decisions,
            reservations: snapshot.reservations,
            reserved_gross: snapshot.reserved_gross,
            reserved_by_strategy: snapshot.reserved_by_strategy,
            reserved_position_delta: snapshot.reserved_position_delta,
            reserved_order_count: snapshot.reserved_order_count,
        })
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

        let notional = match checked_notional(proposal.limit_price, proposal.quantity) {
            Ok(value) => value,
            Err(ArithmeticError::ZeroOrderValue) => return reject(RiskRejection::ZeroOrderValue),
            Err(ArithmeticError::Overflow) => return reject(RiskRejection::ArithmeticOverflow),
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
        if self.reservations.contains_key(&intent.client_order_id) {
            return Err(RiskRejection::InvalidProposal);
        }
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
        self.reservations.insert(
            intent.client_order_id.clone(),
            RiskReservation {
                client_order_id: intent.client_order_id.clone(),
                strategy_id: intent.proposal.strategy_id.clone(),
                symbol: intent.proposal.symbol.clone(),
                notional: intent.authorized_notional,
                position_delta_micros: intent
                    .proposal
                    .side
                    .signed_quantity(intent.proposal.quantity),
            },
        );
        Ok(())
    }
}

fn add_unsigned(
    values: &mut BTreeMap<String, u64>,
    key: &str,
    amount: u64,
) -> Result<(), RiskSnapshotError> {
    let next = values
        .get(key)
        .copied()
        .unwrap_or(0)
        .checked_add(amount)
        .ok_or(RiskSnapshotError::ArithmeticOverflow)?;
    values.insert(key.to_owned(), next);
    Ok(())
}

fn add_signed(
    values: &mut BTreeMap<String, i128>,
    key: &str,
    amount: i128,
) -> Result<(), RiskSnapshotError> {
    let next = values
        .get(key)
        .copied()
        .unwrap_or(0)
        .checked_add(amount)
        .ok_or(RiskSnapshotError::ArithmeticOverflow)?;
    values.insert(key.to_owned(), next);
    Ok(())
}

fn subtract_reservation(
    reservations: &mut BTreeMap<String, u64>,
    key: &str,
    amount: u64,
) -> Result<(), RiskAuthorityError> {
    let remaining = reservations
        .get(key)
        .copied()
        .ok_or(RiskAuthorityError::ReservationAccountingCorrupt)?
        .checked_sub(amount)
        .ok_or(RiskAuthorityError::ReservationAccountingCorrupt)?;
    if remaining == 0 {
        reservations.remove(key);
    } else {
        reservations.insert(key.to_owned(), remaining);
    }
    Ok(())
}

fn subtract_signed_reservation(
    reservations: &mut BTreeMap<String, i128>,
    key: &str,
    amount: i128,
) -> Result<(), RiskAuthorityError> {
    let remaining = reservations
        .get(key)
        .copied()
        .ok_or(RiskAuthorityError::ReservationAccountingCorrupt)?
        .checked_sub(amount)
        .ok_or(RiskAuthorityError::ReservationAccountingCorrupt)?;
    if remaining == 0 {
        reservations.remove(key);
    } else {
        reservations.insert(key.to_owned(), remaining);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PriceMicros, QuantityMicros, Side};

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
    fn zero_quantity_is_rejected_without_a_reservation() {
        let mut authority = RiskAuthority::new(
            policy(),
            PortfolioRiskSnapshot::empty(9_000, 20_696),
            TestVenue(Some(20_696)),
        );
        assert_eq!(
            authority.authorize(proposal("zero", 0), context()),
            Ok(RiskDecision::Rejected(RiskRejection::ZeroOrderValue))
        );
        assert_eq!(authority.outstanding_reservation_count(), 0);
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

    #[test]
    fn covered_terminal_order_moves_from_reservation_into_portfolio_truth() {
        let mut authority = RiskAuthority::new(
            policy(),
            PortfolioRiskSnapshot::empty(9_000, 20_696),
            TestVenue(Some(20_696)),
        );
        assert!(matches!(
            authority
                .authorize(proposal("one", 1_000_000), context())
                .unwrap(),
            RiskDecision::Approved(_)
        ));
        assert_eq!(authority.outstanding_reservation_count(), 1);

        let mut portfolio = PortfolioRiskSnapshot::empty(10_000, 20_696);
        portfolio.gross_exposure = MoneyMicros(5_000_000);
        portfolio
            .strategy_exposure
            .insert("space-weather-v1".into(), MoneyMicros(5_000_000));
        portfolio
            .symbol_positions_micros
            .insert("GRID".into(), 1_000_000);
        portfolio.daily_order_count = 1;
        authority
            .refresh_portfolio_covering(portfolio.clone(), ["one"])
            .unwrap();

        authority
            .refresh_portfolio_covering(portfolio, ["one"])
            .unwrap();

        assert_eq!(authority.outstanding_reservation_count(), 0);
        assert!(authority.reservation("one").is_none());
        assert!(matches!(
            authority
                .authorize(proposal("two", 1_000_000), context())
                .unwrap(),
            RiskDecision::Approved(_)
        ));
    }

    #[test]
    fn uncovered_refresh_keeps_reservation_and_unknown_coverage_is_atomic() {
        let mut authority = RiskAuthority::new(
            policy(),
            PortfolioRiskSnapshot::empty(9_000, 20_696),
            TestVenue(Some(20_696)),
        );
        authority
            .authorize(proposal("one", 1_000_000), context())
            .unwrap();
        authority
            .refresh_portfolio(PortfolioRiskSnapshot::empty(9_500, 20_696))
            .unwrap();
        assert_eq!(authority.outstanding_reservation_count(), 1);

        let before = authority.clone();
        assert_eq!(
            authority.refresh_portfolio_covering(
                PortfolioRiskSnapshot::empty(10_000, 20_696),
                ["unknown"]
            ),
            Err(RiskAuthorityError::UnknownCoveredReservation(
                "unknown".into()
            ))
        );
        assert_eq!(authority.portfolio, before.portfolio);
        assert_eq!(authority.reservations, before.reservations);
        assert_eq!(authority.reserved_gross, before.reserved_gross);
    }

    #[test]
    fn durable_snapshot_round_trip_validates_reservation_accounting() {
        let mut authority = RiskAuthority::new(
            policy(),
            PortfolioRiskSnapshot::empty(9_000, 20_696),
            TestVenue(Some(20_696)),
        );
        authority
            .authorize(proposal("one", 1_000_000), context())
            .unwrap();
        let encoded = serde_json::to_vec(&authority.snapshot()).unwrap();
        let snapshot = serde_json::from_slice(&encoded).unwrap();
        let restored = RiskAuthority::try_from_snapshot(snapshot).unwrap();
        assert_eq!(restored, authority);

        let mut corrupt = authority.snapshot();
        corrupt.reserved_gross += 1;
        assert_eq!(
            RiskAuthority::try_from_snapshot(corrupt),
            Err(RiskSnapshotError::ReservationAccounting)
        );
    }
}
