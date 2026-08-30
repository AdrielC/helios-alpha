use std::collections::{BTreeMap, BTreeSet};

use helio_scan::{IdempotentSink, OutputId, SinkDelivery};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{checked_notional, CapitalAuthorization, ExecutionMode, OrderIntent};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrokerAcknowledgement {
    pub broker_order_id: String,
    pub client_order_id: String,
    pub accepted_at_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BrokerError {
    #[error("broker rejected order: {0}")]
    Rejected(String),
    #[error("broker outcome is ambiguous")]
    AmbiguousOutcome,
    #[error("broker is unavailable")]
    Unavailable,
}

/// Adapter contract: the venue-facing implementation must deduplicate `client_order_id`.
pub trait BrokerPort {
    fn submit(&mut self, intent: &OrderIntent) -> Result<BrokerAcknowledgement, BrokerError>;

    fn lookup_by_client_order_id(
        &mut self,
        client_order_id: &str,
    ) -> Result<Option<BrokerAcknowledgement>, BrokerError>;
}

/// Canonical, non-negative decimal returned by a broker.
///
/// Broker executions often carry more precision than Helios order micros. Keeping the canonical
/// decimal text here avoids routing fills through `f64` before a venue-specific settlement policy
/// decides how to round them.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct BrokerDecimal(String);

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BrokerDecimalError {
    #[error("broker decimal must be a non-negative base-10 value without an exponent")]
    Invalid,
}

impl BrokerDecimal {
    pub fn try_new(value: impl Into<String>) -> Result<Self, BrokerDecimalError> {
        let value = value.into();
        let (whole, fraction) = match value.split_once('.') {
            Some((whole, fraction)) => (whole, Some(fraction)),
            None => (value.as_str(), None),
        };
        if whole.is_empty()
            || !whole.bytes().all(|byte| byte.is_ascii_digit())
            || fraction.is_some_and(|digits| {
                digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit())
            })
        {
            return Err(BrokerDecimalError::Invalid);
        }

        let whole = whole.trim_start_matches('0');
        let whole = if whole.is_empty() { "0" } else { whole };
        let fraction = fraction.map(|digits| digits.trim_end_matches('0'));
        let canonical = match fraction {
            Some("") | None => whole.to_owned(),
            Some(digits) => format!("{whole}.{digits}"),
        };
        Ok(Self(canonical))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for BrokerDecimal {
    type Error = BrokerDecimalError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::try_new(value)
    }
}

impl From<BrokerDecimal> for String {
    fn from(value: BrokerDecimal) -> Self {
        value.0
    }
}

/// Normalized broker-side order lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BrokerOrderState {
    Pending,
    Open,
    PartiallyFilled,
    Filled,
    Canceled,
    Failed,
}

impl BrokerOrderState {
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Filled | Self::Canceled | Self::Failed)
    }
}

/// One exact execution reported by a broker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrokerExecution {
    pub execution_id: String,
    pub effective_price: BrokerDecimal,
    pub quantity: BrokerDecimal,
    /// Broker timestamp retained verbatim for audit and deterministic reconciliation.
    pub occurred_at: String,
}

/// Current broker truth for one order, suitable for polling or push-update normalization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrokerOrderSnapshot {
    pub acknowledgement: BrokerAcknowledgement,
    pub state: BrokerOrderState,
    pub executions: Vec<BrokerExecution>,
    pub filled_quantity: BrokerDecimal,
    pub average_price: Option<BrokerDecimal>,
    pub updated_at: String,
}

/// Optional lifecycle surface for brokers that expose order status and cancellation.
///
/// This is deliberately separate from [`BrokerPort`]. The idempotent submission gateway stays
/// minimal while execution reconciliation can require the richer contract.
pub trait BrokerLifecyclePort: BrokerPort {
    fn fetch_order_by_client_order_id(
        &mut self,
        client_order_id: &str,
    ) -> Result<Option<BrokerOrderSnapshot>, BrokerError>;

    fn cancel_by_client_order_id(
        &mut self,
        client_order_id: &str,
    ) -> Result<BrokerOrderSnapshot, BrokerError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GatewayOrderStatus {
    PendingReconciliation,
    Accepted(BrokerAcknowledgement),
    Rejected(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayOrderRecord {
    pub intent: OrderIntent,
    pub attempts: u32,
    pub status: GatewayOrderStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayReceipt {
    pub acknowledgement: BrokerAcknowledgement,
    pub replayed: bool,
    pub reconciled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderGatewayPolicy {
    pub environment: String,
    pub max_risk_authorization_age_ns: u64,
    pub allowed_risk_policy_versions: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum GatewayError {
    #[error("live execution requires a current production capital authorization")]
    CapitalAuthorizationRequired,
    #[error("live order carries a stale or future risk authorization")]
    StaleRiskAuthorization,
    #[error("live order risk policy version is not allowed by the gateway")]
    RiskPolicyNotAllowed,
    #[error("order identity was replayed with different contents")]
    OrderIdentityConflict,
    #[error("order intent notional does not match its price and quantity")]
    InvalidAuthorizedNotional,
    #[error("output identity does not equal the order client identity")]
    OutputIdentityMismatch,
    #[error("gateway has no record for client order {0}")]
    UnknownOrder(String),
    #[error("gateway attempt counter overflowed")]
    AttemptOverflow,
    #[error("broker acknowledgement identity does not match the submitted client order")]
    BrokerIdentityMismatch,
    #[error("broker order remains pending reconciliation")]
    PendingReconciliation,
    #[error(transparent)]
    Broker(#[from] BrokerError),
}

/// Idempotent broker gateway with a durable-state-shaped journal.
///
/// A production storage adapter persists `orders` before calling the broker. The reference
/// implementation makes the state machine and fault semantics executable without credentials.
#[derive(Debug, Clone)]
pub struct OrderGateway<B> {
    policy: OrderGatewayPolicy,
    broker: B,
    orders: BTreeMap<String, GatewayOrderRecord>,
}

impl<B> OrderGateway<B>
where
    B: BrokerPort,
{
    pub fn new(policy: OrderGatewayPolicy, broker: B) -> Self {
        Self {
            policy,
            broker,
            orders: BTreeMap::new(),
        }
    }

    pub fn dispatch(
        &mut self,
        intent: &OrderIntent,
        capital_authorization: Option<&CapitalAuthorization>,
        now_ns: u64,
    ) -> Result<GatewayReceipt, GatewayError> {
        if !matches!(
            checked_notional(intent.proposal.limit_price, intent.proposal.quantity),
            Ok(notional) if notional == intent.authorized_notional
        ) {
            return Err(GatewayError::InvalidAuthorizedNotional);
        }
        if intent.proposal.mode == ExecutionMode::Live {
            if !capital_authorization.is_some_and(|authorization| {
                authorization.permits(&self.policy.environment, now_ns)
            }) {
                return Err(GatewayError::CapitalAuthorizationRequired);
            }
            if !self
                .policy
                .allowed_risk_policy_versions
                .contains(&intent.risk_policy_version)
            {
                return Err(GatewayError::RiskPolicyNotAllowed);
            }
            let Some(risk_age) = now_ns.checked_sub(intent.authorized_at_ns) else {
                return Err(GatewayError::StaleRiskAuthorization);
            };
            if risk_age > self.policy.max_risk_authorization_age_ns {
                return Err(GatewayError::StaleRiskAuthorization);
            }
        }
        if let Some(record) = self.orders.get(&intent.client_order_id) {
            if record.intent != *intent {
                return Err(GatewayError::OrderIdentityConflict);
            }
            if let GatewayOrderStatus::Accepted(acknowledgement) = &record.status {
                return Ok(GatewayReceipt {
                    acknowledgement: acknowledgement.clone(),
                    replayed: true,
                    reconciled: false,
                });
            }
        } else {
            self.orders.insert(
                intent.client_order_id.clone(),
                GatewayOrderRecord {
                    intent: intent.clone(),
                    attempts: 0,
                    status: GatewayOrderStatus::PendingReconciliation,
                },
            );
        }
        self.submit_or_reconcile(&intent.client_order_id)
    }

    pub fn reconcile(&mut self, client_order_id: &str) -> Result<GatewayReceipt, GatewayError> {
        self.submit_or_reconcile(client_order_id)
    }

    pub fn pending_reconciliation_count(&self) -> usize {
        self.orders
            .values()
            .filter(|record| matches!(record.status, GatewayOrderStatus::PendingReconciliation))
            .count()
    }

    pub fn record(&self, client_order_id: &str) -> Option<&GatewayOrderRecord> {
        self.orders.get(client_order_id)
    }

    pub const fn broker(&self) -> &B {
        &self.broker
    }

    fn submit_or_reconcile(
        &mut self,
        client_order_id: &str,
    ) -> Result<GatewayReceipt, GatewayError> {
        let record = self
            .orders
            .get(client_order_id)
            .ok_or_else(|| GatewayError::UnknownOrder(client_order_id.to_owned()))?;
        match &record.status {
            GatewayOrderStatus::Accepted(acknowledgement) => {
                return Ok(GatewayReceipt {
                    acknowledgement: acknowledgement.clone(),
                    replayed: true,
                    reconciled: false,
                });
            }
            GatewayOrderStatus::Rejected(reason) => {
                return Err(GatewayError::Broker(BrokerError::Rejected(reason.clone())));
            }
            GatewayOrderStatus::PendingReconciliation => {}
        }
        if let Some(acknowledgement) = self
            .broker
            .lookup_by_client_order_id(client_order_id)
            .map_err(|_| GatewayError::PendingReconciliation)?
        {
            self.accept(client_order_id, acknowledgement.clone())?;
            return Ok(GatewayReceipt {
                acknowledgement,
                replayed: true,
                reconciled: true,
            });
        }

        let intent = self
            .orders
            .get(client_order_id)
            .ok_or_else(|| GatewayError::UnknownOrder(client_order_id.to_owned()))?
            .intent
            .clone();
        self.increment_attempts(client_order_id)?;
        match self.broker.submit(&intent) {
            Ok(acknowledgement) => {
                self.accept(client_order_id, acknowledgement.clone())?;
                Ok(GatewayReceipt {
                    acknowledgement,
                    replayed: false,
                    reconciled: false,
                })
            }
            Err(BrokerError::AmbiguousOutcome) => {
                match self.broker.lookup_by_client_order_id(client_order_id) {
                    Ok(Some(acknowledgement)) => {
                        self.accept(client_order_id, acknowledgement.clone())?;
                        Ok(GatewayReceipt {
                            acknowledgement,
                            replayed: false,
                            reconciled: true,
                        })
                    }
                    Ok(None) | Err(_) => Err(GatewayError::PendingReconciliation),
                }
            }
            Err(BrokerError::Rejected(reason)) => {
                if let Some(record) = self.orders.get_mut(client_order_id) {
                    record.status = GatewayOrderStatus::Rejected(reason.clone());
                }
                Err(GatewayError::Broker(BrokerError::Rejected(reason)))
            }
            Err(BrokerError::Unavailable) => Err(GatewayError::PendingReconciliation),
        }
    }

    fn increment_attempts(&mut self, client_order_id: &str) -> Result<(), GatewayError> {
        let record = self
            .orders
            .get_mut(client_order_id)
            .ok_or_else(|| GatewayError::UnknownOrder(client_order_id.to_owned()))?;
        record.attempts = record
            .attempts
            .checked_add(1)
            .ok_or(GatewayError::AttemptOverflow)?;
        Ok(())
    }

    fn accept(
        &mut self,
        client_order_id: &str,
        acknowledgement: BrokerAcknowledgement,
    ) -> Result<(), GatewayError> {
        if acknowledgement.client_order_id != client_order_id
            || acknowledgement.broker_order_id.trim().is_empty()
        {
            return Err(GatewayError::BrokerIdentityMismatch);
        }
        let record = self
            .orders
            .get_mut(client_order_id)
            .ok_or_else(|| GatewayError::UnknownOrder(client_order_id.to_owned()))?;
        record.status = GatewayOrderStatus::Accepted(acknowledgement);
        Ok(())
    }
}

impl<B> IdempotentSink<OrderIntent> for OrderGateway<B>
where
    B: BrokerPort,
{
    type Acknowledgement = BrokerAcknowledgement;
    type Error = GatewayError;

    fn deliver(
        &mut self,
        id: &OutputId,
        intent: &OrderIntent,
    ) -> Result<SinkDelivery<Self::Acknowledgement>, Self::Error> {
        if id.as_str() != intent.client_order_id {
            return Err(GatewayError::OutputIdentityMismatch);
        }
        if intent.proposal.mode != ExecutionMode::Paper {
            return Err(GatewayError::CapitalAuthorizationRequired);
        }
        let existed = self.orders.contains_key(&intent.client_order_id);
        let receipt = self.dispatch(intent, None, intent.authorized_at_ns)?;
        Ok(if existed || receipt.replayed {
            SinkDelivery::Duplicate(receipt.acknowledgement)
        } else {
            SinkDelivery::Applied(receipt.acknowledgement)
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaperBrokerFault {
    None,
    AcceptThenTimeoutOnce,
    UnavailableBeforeAcceptOnce,
    RejectBeforeAcceptOnce,
}

#[derive(Debug, Clone)]
pub struct PaperBroker {
    now_ns: u64,
    next_order_id: u64,
    orders: BTreeMap<String, BrokerAcknowledgement>,
    fault: PaperBrokerFault,
}

impl PaperBroker {
    pub fn new(now_ns: u64) -> Self {
        Self {
            now_ns,
            next_order_id: 0,
            orders: BTreeMap::new(),
            fault: PaperBrokerFault::None,
        }
    }

    pub fn inject_fault(&mut self, fault: PaperBrokerFault) {
        self.fault = fault;
    }

    pub fn accepted_order_count(&self) -> usize {
        self.orders.len()
    }
}

impl BrokerPort for PaperBroker {
    fn submit(&mut self, intent: &OrderIntent) -> Result<BrokerAcknowledgement, BrokerError> {
        if let Some(existing) = self.orders.get(&intent.client_order_id) {
            return Ok(existing.clone());
        }
        if self.fault == PaperBrokerFault::UnavailableBeforeAcceptOnce {
            self.fault = PaperBrokerFault::None;
            return Err(BrokerError::Unavailable);
        }
        if self.fault == PaperBrokerFault::RejectBeforeAcceptOnce {
            self.fault = PaperBrokerFault::None;
            return Err(BrokerError::Rejected("paper rejection fault".into()));
        }
        let acknowledgement = BrokerAcknowledgement {
            broker_order_id: format!("paper-{}", self.next_order_id),
            client_order_id: intent.client_order_id.clone(),
            accepted_at_ns: self.now_ns,
        };
        self.next_order_id = self
            .next_order_id
            .checked_add(1)
            .ok_or(BrokerError::Rejected(
                "paper order identity overflow".into(),
            ))?;
        self.orders
            .insert(intent.client_order_id.clone(), acknowledgement.clone());
        if self.fault == PaperBrokerFault::AcceptThenTimeoutOnce {
            self.fault = PaperBrokerFault::None;
            return Err(BrokerError::AmbiguousOutcome);
        }
        Ok(acknowledgement)
    }

    fn lookup_by_client_order_id(
        &mut self,
        client_order_id: &str,
    ) -> Result<Option<BrokerAcknowledgement>, BrokerError> {
        Ok(self.orders.get(client_order_id).cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MoneyMicros, OrderProposal, PriceMicros, QuantityMicros, Side};

    fn intent(mode: ExecutionMode) -> OrderIntent {
        OrderIntent {
            client_order_id: "order-1".into(),
            proposal: OrderProposal {
                proposal_id: "order-1".into(),
                strategy_id: "strategy".into(),
                symbol: "GRID".into(),
                venue: "XNYS".into(),
                currency: "USD".into(),
                side: Side::Buy,
                quantity: QuantityMicros(1_000_000),
                limit_price: PriceMicros(10_000_000),
                mode,
                trading_day: 1,
            },
            authorized_notional: MoneyMicros(10_000_000),
            risk_policy_version: "risk-1".into(),
            authorized_at_ns: 100,
        }
    }

    fn gateway(broker: PaperBroker) -> OrderGateway<PaperBroker> {
        OrderGateway::new(
            OrderGatewayPolicy {
                environment: "production".into(),
                max_risk_authorization_age_ns: 1_000,
                allowed_risk_policy_versions: BTreeSet::from(["risk-1".into()]),
            },
            broker,
        )
    }

    #[test]
    fn ambiguous_acceptance_is_reconciled_without_duplicate_order() {
        let mut broker = PaperBroker::new(100);
        broker.inject_fault(PaperBrokerFault::AcceptThenTimeoutOnce);
        let mut gateway = gateway(broker);
        let first = gateway
            .dispatch(&intent(ExecutionMode::Paper), None, 100)
            .unwrap();
        assert!(first.reconciled);
        let replay = gateway
            .dispatch(&intent(ExecutionMode::Paper), None, 100)
            .unwrap();
        assert!(replay.replayed);
        assert_eq!(gateway.broker().accepted_order_count(), 1);
    }

    #[test]
    fn unproven_live_capital_never_reaches_broker() {
        let mut gateway = gateway(PaperBroker::new(100));
        assert_eq!(
            gateway.dispatch(&intent(ExecutionMode::Live), None, 100),
            Err(GatewayError::CapitalAuthorizationRequired)
        );
        assert_eq!(gateway.broker().accepted_order_count(), 0);
    }

    #[test]
    fn definite_unavailability_stays_pending_until_reconciled() {
        let mut broker = PaperBroker::new(100);
        broker.inject_fault(PaperBrokerFault::UnavailableBeforeAcceptOnce);
        let mut gateway = gateway(broker);
        assert_eq!(
            gateway.dispatch(&intent(ExecutionMode::Paper), None, 100),
            Err(GatewayError::PendingReconciliation)
        );
        assert_eq!(gateway.pending_reconciliation_count(), 1);
        assert!(gateway.reconcile("order-1").is_ok());
        assert_eq!(gateway.broker().accepted_order_count(), 1);
    }

    #[test]
    fn definitive_rejection_is_never_resubmitted() {
        let mut broker = PaperBroker::new(100);
        broker.inject_fault(PaperBrokerFault::RejectBeforeAcceptOnce);
        let mut gateway = gateway(broker);
        let first = gateway.dispatch(&intent(ExecutionMode::Paper), None, 100);
        let replay = gateway.dispatch(&intent(ExecutionMode::Paper), None, 100);
        assert_eq!(first, replay);
        assert_eq!(gateway.record("order-1").unwrap().attempts, 1);
        assert_eq!(gateway.broker().accepted_order_count(), 0);
    }

    #[test]
    fn malformed_authorized_notional_never_reaches_broker() {
        let mut gateway = gateway(PaperBroker::new(100));
        let mut malformed = intent(ExecutionMode::Paper);
        malformed.authorized_notional = MoneyMicros(1);
        assert_eq!(
            gateway.dispatch(&malformed, None, 100),
            Err(GatewayError::InvalidAuthorizedNotional)
        );
        assert_eq!(gateway.broker().accepted_order_count(), 0);
        assert!(gateway.record("order-1").is_none());
    }
}
