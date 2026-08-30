use std::collections::BTreeMap;

use helio_execution::{MoneyMicros, OrderIntent, PriceMicros, QuantityMicros};
use serde::{Deserialize, Serialize};

use crate::{
    OmsCommand, OmsError, OmsEvent, OrderSnapshot, OrderState, ReconciledState, TimeInForce,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderAggregate {
    intent: OrderIntent,
    time_in_force: TimeInForce,
    state: OrderState,
    broker_order_id: Option<String>,
    working_quantity: QuantityMicros,
    working_limit_price: PriceMicros,
    pending_replace: Option<(QuantityMicros, PriceMicros)>,
    filled_quantity: QuantityMicros,
    fill_value_scaled: u128,
    executions: BTreeMap<String, (Option<String>, Option<String>, QuantityMicros, PriceMicros)>,
    version: u64,
    last_update_at_ns: u64,
    uncertainty_reason: Option<String>,
}

impl OrderAggregate {
    pub fn from_submission(event: &OmsEvent) -> Result<Self, OmsError> {
        let OmsEvent::Submitted {
            intent,
            time_in_force,
            at_ns,
        } = event
        else {
            return Err(OmsError::InvalidTransition {
                state: OrderState::PendingSubmit,
                operation: "create without submission".into(),
            });
        };
        if intent.client_order_id.trim().is_empty() {
            return Err(OmsError::EmptyIdentity);
        }
        if intent.proposal.quantity.0 == 0 || intent.proposal.limit_price.0 == 0 {
            return Err(OmsError::ZeroValue);
        }
        Ok(Self {
            intent: intent.clone(),
            time_in_force: *time_in_force,
            state: OrderState::PendingSubmit,
            broker_order_id: None,
            working_quantity: intent.proposal.quantity,
            working_limit_price: intent.proposal.limit_price,
            pending_replace: None,
            filled_quantity: QuantityMicros(0),
            fill_value_scaled: 0,
            executions: BTreeMap::new(),
            version: 1,
            last_update_at_ns: *at_ns,
            uncertainty_reason: None,
        })
    }

    pub fn decide(&self, command: &OmsCommand) -> Result<Vec<OmsEvent>, OmsError> {
        if command.observed_at_ns() < self.last_update_at_ns {
            return Err(OmsError::ObservationTimeRegression);
        }
        match command {
            OmsCommand::Submit { intent, .. } => {
                if *intent == self.intent {
                    Ok(Vec::new())
                } else {
                    Err(OmsError::OrderIdentityConflict(
                        intent.client_order_id.clone(),
                    ))
                }
            }
            OmsCommand::Acknowledge {
                broker_order_id,
                at_ns,
                ..
            } => {
                if broker_order_id.trim().is_empty() {
                    return Err(OmsError::EmptyIdentity);
                }
                if self.broker_order_id.as_ref() == Some(broker_order_id)
                    && matches!(
                        self.state,
                        OrderState::Working
                            | OrderState::PartiallyFilled
                            | OrderState::PendingCancel
                            | OrderState::PendingReplace
                    )
                {
                    return Ok(Vec::new());
                }
                self.require_state(&[OrderState::PendingSubmit], "acknowledge")?;
                Ok(vec![OmsEvent::Acknowledged {
                    broker_order_id: broker_order_id.clone(),
                    at_ns: *at_ns,
                }])
            }
            OmsCommand::Reject { reason, at_ns, .. } => {
                self.require_state(&[OrderState::PendingSubmit], "reject")?;
                if reason.trim().is_empty() {
                    return Err(OmsError::EmptyReason);
                }
                Ok(vec![OmsEvent::Rejected {
                    reason: reason.clone(),
                    at_ns: *at_ns,
                }])
            }
            OmsCommand::RecordFill {
                broker_order_id,
                execution_id,
                venue_occurred_at,
                quantity,
                price,
                at_ns,
                ..
            } => {
                if execution_id.trim().is_empty() {
                    return Err(OmsError::EmptyIdentity);
                }
                if broker_order_id
                    .as_ref()
                    .is_some_and(|order_id| order_id.trim().is_empty())
                {
                    return Err(OmsError::EmptyIdentity);
                }
                if venue_occurred_at
                    .as_ref()
                    .is_some_and(|occurred_at| occurred_at.trim().is_empty())
                {
                    return Err(OmsError::EmptyIdentity);
                }
                if let (Some(existing), Some(reported)) =
                    (self.broker_order_id.as_ref(), broker_order_id.as_ref())
                {
                    if existing != reported {
                        return Err(OmsError::BrokerIdentityConflict);
                    }
                }
                if self.state == OrderState::PendingSubmit && broker_order_id.is_none() {
                    return Err(OmsError::BrokerIdentityConflict);
                }
                if quantity.0 == 0 || price.0 == 0 {
                    return Err(OmsError::ZeroValue);
                }
                if let Some(existing) = self.executions.get(execution_id) {
                    return if existing
                        == &(
                            broker_order_id.clone(),
                            venue_occurred_at.clone(),
                            *quantity,
                            *price,
                        ) {
                        Ok(Vec::new())
                    } else {
                        Err(OmsError::ExecutionIdentityConflict(execution_id.clone()))
                    };
                }
                self.require_state(
                    &[
                        OrderState::Working,
                        OrderState::PendingSubmit,
                        OrderState::PartiallyFilled,
                        OrderState::PendingCancel,
                        OrderState::PendingReplace,
                        OrderState::Unknown,
                    ],
                    "record fill",
                )?;
                let total = self
                    .filled_quantity
                    .0
                    .checked_add(quantity.0)
                    .ok_or(OmsError::ArithmeticOverflow)?;
                if total > self.working_quantity.0 {
                    return Err(OmsError::Overfill);
                }
                Ok(vec![OmsEvent::FillRecorded {
                    broker_order_id: broker_order_id.clone(),
                    execution_id: execution_id.clone(),
                    venue_occurred_at: venue_occurred_at.clone(),
                    quantity: *quantity,
                    price: *price,
                    at_ns: *at_ns,
                }])
            }
            OmsCommand::RequestCancel { at_ns, .. } => {
                self.require_state(
                    &[OrderState::Working, OrderState::PartiallyFilled],
                    "request cancel",
                )?;
                Ok(vec![OmsEvent::CancelRequested { at_ns: *at_ns }])
            }
            OmsCommand::ConfirmCanceled { at_ns, .. } => {
                self.require_state(&[OrderState::PendingCancel], "confirm cancel")?;
                Ok(vec![OmsEvent::Canceled { at_ns: *at_ns }])
            }
            OmsCommand::RequestReplace {
                new_quantity,
                new_limit_price,
                at_ns,
                ..
            } => {
                self.require_state(
                    &[OrderState::Working, OrderState::PartiallyFilled],
                    "request replace",
                )?;
                if new_quantity.0 == 0 || new_limit_price.0 == 0 {
                    return Err(OmsError::ZeroValue);
                }
                if new_quantity.0 < self.filled_quantity.0 {
                    return Err(OmsError::ReplaceBelowFilled);
                }
                Ok(vec![OmsEvent::ReplaceRequested {
                    new_quantity: *new_quantity,
                    new_limit_price: *new_limit_price,
                    at_ns: *at_ns,
                }])
            }
            OmsCommand::ConfirmReplaced {
                broker_order_id,
                at_ns,
                ..
            } => {
                self.require_state(&[OrderState::PendingReplace], "confirm replace")?;
                if broker_order_id.trim().is_empty() {
                    return Err(OmsError::EmptyIdentity);
                }
                let (new_quantity, _) =
                    self.pending_replace
                        .as_ref()
                        .ok_or(OmsError::InvalidTransition {
                            state: self.state,
                            operation: "confirm replace without pending values".into(),
                        })?;
                if new_quantity.0 < self.filled_quantity.0 {
                    return Err(OmsError::ReplaceBelowFilled);
                }
                Ok(vec![OmsEvent::Replaced {
                    broker_order_id: broker_order_id.clone(),
                    at_ns: *at_ns,
                }])
            }
            OmsCommand::RejectPendingAction { reason, at_ns, .. } => {
                self.require_state(
                    &[OrderState::PendingCancel, OrderState::PendingReplace],
                    "reject pending action",
                )?;
                if reason.trim().is_empty() {
                    return Err(OmsError::EmptyReason);
                }
                Ok(vec![OmsEvent::PendingActionRejected {
                    reason: reason.clone(),
                    at_ns: *at_ns,
                }])
            }
            OmsCommand::MarkExpired { at_ns, .. } => {
                self.require_state(
                    &[
                        OrderState::PendingSubmit,
                        OrderState::Working,
                        OrderState::PartiallyFilled,
                        OrderState::PendingCancel,
                        OrderState::PendingReplace,
                    ],
                    "expire",
                )?;
                Ok(vec![OmsEvent::Expired { at_ns: *at_ns }])
            }
            OmsCommand::MarkUnknown { reason, at_ns, .. } => {
                if self.state.is_terminal() {
                    return Err(OmsError::InvalidTransition {
                        state: self.state,
                        operation: "mark unknown".into(),
                    });
                }
                if reason.trim().is_empty() {
                    return Err(OmsError::EmptyReason);
                }
                Ok(vec![OmsEvent::MarkedUnknown {
                    reason: reason.clone(),
                    at_ns: *at_ns,
                }])
            }
            OmsCommand::ReconcileUnknown {
                broker_order_id,
                state,
                at_ns,
                ..
            } => {
                self.require_state(&[OrderState::Unknown], "reconcile unknown order")?;
                if broker_order_id
                    .as_ref()
                    .is_some_and(|order_id| order_id.trim().is_empty())
                {
                    return Err(OmsError::EmptyIdentity);
                }
                if let (Some(existing), Some(reported)) =
                    (self.broker_order_id.as_ref(), broker_order_id.as_ref())
                {
                    if existing != reported {
                        return Err(OmsError::BrokerIdentityConflict);
                    }
                }
                if *state == ReconciledState::Working
                    && broker_order_id.is_none()
                    && self.broker_order_id.is_none()
                {
                    return Err(OmsError::BrokerIdentityConflict);
                }
                if *state == ReconciledState::Rejected && self.filled_quantity.0 > 0 {
                    return Err(OmsError::ReconciliationConflict);
                }
                Ok(vec![OmsEvent::UnknownReconciled {
                    broker_order_id: broker_order_id.clone(),
                    state: *state,
                    at_ns: *at_ns,
                }])
            }
        }
    }

    pub fn apply(&mut self, event: &OmsEvent) -> Result<(), OmsError> {
        match event {
            OmsEvent::Submitted { .. } => {
                return Err(OmsError::OrderIdentityConflict(
                    self.intent.client_order_id.clone(),
                ));
            }
            OmsEvent::Acknowledged {
                broker_order_id,
                at_ns,
            } => {
                self.broker_order_id = Some(broker_order_id.clone());
                self.state = self.open_state();
                self.last_update_at_ns = *at_ns;
            }
            OmsEvent::Rejected { at_ns, .. } => {
                self.state = OrderState::Rejected;
                self.last_update_at_ns = *at_ns;
            }
            OmsEvent::FillRecorded {
                broker_order_id,
                execution_id,
                venue_occurred_at,
                quantity,
                price,
                at_ns,
            } => {
                if let Some(broker_order_id) = broker_order_id {
                    self.broker_order_id = Some(broker_order_id.clone());
                }
                self.filled_quantity.0 = self
                    .filled_quantity
                    .0
                    .checked_add(quantity.0)
                    .ok_or(OmsError::ArithmeticOverflow)?;
                self.fill_value_scaled = self
                    .fill_value_scaled
                    .checked_add(
                        u128::from(quantity.0)
                            .checked_mul(u128::from(price.0))
                            .ok_or(OmsError::ArithmeticOverflow)?,
                    )
                    .ok_or(OmsError::ArithmeticOverflow)?;
                self.executions.insert(
                    execution_id.clone(),
                    (
                        broker_order_id.clone(),
                        venue_occurred_at.clone(),
                        *quantity,
                        *price,
                    ),
                );
                self.state = match self.state {
                    OrderState::Unknown => OrderState::Unknown,
                    _ if self.filled_quantity.0 == self.working_quantity.0 => OrderState::Filled,
                    OrderState::PendingCancel => OrderState::PendingCancel,
                    OrderState::PendingReplace => OrderState::PendingReplace,
                    _ => self.open_state(),
                };
                self.last_update_at_ns = *at_ns;
            }
            OmsEvent::CancelRequested { at_ns } => {
                self.state = OrderState::PendingCancel;
                self.last_update_at_ns = *at_ns;
            }
            OmsEvent::Canceled { at_ns } => {
                self.state = OrderState::Canceled;
                self.last_update_at_ns = *at_ns;
            }
            OmsEvent::ReplaceRequested {
                new_quantity,
                new_limit_price,
                at_ns,
            } => {
                self.pending_replace = Some((*new_quantity, *new_limit_price));
                self.state = OrderState::PendingReplace;
                self.last_update_at_ns = *at_ns;
            }
            OmsEvent::Replaced {
                broker_order_id,
                at_ns,
            } => {
                let (new_quantity, new_limit_price) =
                    self.pending_replace
                        .take()
                        .ok_or(OmsError::InvalidTransition {
                            state: self.state,
                            operation: "replace without pending values".into(),
                        })?;
                self.working_quantity = new_quantity;
                self.working_limit_price = new_limit_price;
                self.broker_order_id = Some(broker_order_id.clone());
                self.state = self.open_state();
                self.last_update_at_ns = *at_ns;
            }
            OmsEvent::PendingActionRejected { at_ns, .. } => {
                self.pending_replace = None;
                self.state = self.open_state();
                self.last_update_at_ns = *at_ns;
            }
            OmsEvent::Expired { at_ns } => {
                self.state = OrderState::Expired;
                self.last_update_at_ns = *at_ns;
            }
            OmsEvent::MarkedUnknown { reason, at_ns } => {
                self.state = OrderState::Unknown;
                self.uncertainty_reason = Some(reason.clone());
                self.last_update_at_ns = *at_ns;
            }
            OmsEvent::UnknownReconciled {
                broker_order_id,
                state,
                at_ns,
            } => {
                if let Some(broker_order_id) = broker_order_id {
                    self.broker_order_id = Some(broker_order_id.clone());
                }
                self.pending_replace = None;
                self.uncertainty_reason = None;
                self.state = match state {
                    ReconciledState::Working => self.open_state(),
                    ReconciledState::Canceled => OrderState::Canceled,
                    ReconciledState::Rejected => OrderState::Rejected,
                    ReconciledState::Expired => OrderState::Expired,
                };
                self.last_update_at_ns = *at_ns;
            }
        }
        self.version = self
            .version
            .checked_add(1)
            .ok_or(OmsError::VersionOverflow)?;
        Ok(())
    }

    pub fn snapshot(&self) -> Result<OrderSnapshot, OmsError> {
        let average_fill_price = if self.filled_quantity.0 == 0 {
            None
        } else {
            let rounded = self
                .fill_value_scaled
                .checked_add(u128::from(self.filled_quantity.0 / 2))
                .ok_or(OmsError::ArithmeticOverflow)?
                / u128::from(self.filled_quantity.0);
            Some(PriceMicros(
                u64::try_from(rounded).map_err(|_| OmsError::ArithmeticOverflow)?,
            ))
        };
        let notional = self
            .fill_value_scaled
            .checked_add(999_999)
            .ok_or(OmsError::ArithmeticOverflow)?
            / 1_000_000;
        Ok(OrderSnapshot {
            client_order_id: self.intent.client_order_id.clone(),
            broker_order_id: self.broker_order_id.clone(),
            state: self.state,
            intent: self.intent.clone(),
            time_in_force: self.time_in_force,
            working_quantity: self.working_quantity,
            working_limit_price: self.working_limit_price,
            filled_quantity: self.filled_quantity,
            average_fill_price,
            filled_notional: MoneyMicros(
                u64::try_from(notional).map_err(|_| OmsError::ArithmeticOverflow)?,
            ),
            version: self.version,
            last_update_at_ns: self.last_update_at_ns,
            uncertainty_reason: self.uncertainty_reason.clone(),
        })
    }

    fn open_state(&self) -> OrderState {
        if self.filled_quantity.0 == self.working_quantity.0 {
            OrderState::Filled
        } else if self.filled_quantity.0 > 0 {
            OrderState::PartiallyFilled
        } else {
            OrderState::Working
        }
    }

    fn require_state(&self, allowed: &[OrderState], operation: &str) -> Result<(), OmsError> {
        if allowed.contains(&self.state) {
            Ok(())
        } else {
            Err(OmsError::InvalidTransition {
                state: self.state,
                operation: operation.into(),
            })
        }
    }

    pub const fn version(&self) -> u64 {
        self.version
    }
}
