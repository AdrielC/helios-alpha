use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
    subject_token, CommandReceipt, OmsCapabilities, OmsCommand, OmsCommandPort, OmsError, OmsEvent,
    OmsEventEnvelope, OmsEventSource, OmsQueryPort, OrderAggregate, OrderSnapshot,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AppliedCommand {
    command: OmsCommand,
    receipt: CommandReceipt,
}

/// Executable specification for any Helios-compatible OMS.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferenceOms {
    account_id: String,
    orders: BTreeMap<String, OrderAggregate>,
    commands: BTreeMap<String, AppliedCommand>,
    events: Vec<OmsEventEnvelope>,
}

impl ReferenceOms {
    pub fn try_new(account_id: impl Into<String>) -> Result<Self, OmsError> {
        let account_id = account_id.into();
        if account_id.trim().is_empty() {
            return Err(OmsError::EmptyIdentity);
        }
        Ok(Self {
            account_id,
            orders: BTreeMap::new(),
            commands: BTreeMap::new(),
            events: Vec::new(),
        })
    }

    pub fn next_cursor(&self) -> u64 {
        self.events.len() as u64
    }

    pub fn account_id(&self) -> &str {
        &self.account_id
    }

    fn execute_in_place(&mut self, command: OmsCommand) -> Result<CommandReceipt, OmsError> {
        if command.command_id().trim().is_empty() || command.client_order_id().trim().is_empty() {
            return Err(OmsError::EmptyIdentity);
        }
        if let Some(applied) = self.commands.get(command.command_id()) {
            if applied.command != command {
                return Err(OmsError::CommandIdentityConflict(
                    command.command_id().to_owned(),
                ));
            }
            let mut receipt = applied.receipt.clone();
            receipt.replayed = true;
            return Ok(receipt);
        }

        let events = if let OmsCommand::Submit {
            intent,
            time_in_force,
            at_ns,
            ..
        } = &command
        {
            if let Some(existing) = self.orders.get(&intent.client_order_id) {
                existing.decide(&command)?
            } else {
                vec![OmsEvent::Submitted {
                    intent: intent.clone(),
                    time_in_force: *time_in_force,
                    at_ns: *at_ns,
                }]
            }
        } else {
            self.orders
                .get(command.client_order_id())
                .ok_or_else(|| OmsError::UnknownOrder(command.client_order_id().to_owned()))?
                .decide(&command)?
        };
        self.commit(command, events)
    }

    fn commit(
        &mut self,
        command: OmsCommand,
        events: Vec<OmsEvent>,
    ) -> Result<CommandReceipt, OmsError> {
        let command_id = command.command_id().to_owned();
        let client_order_id = command.client_order_id().to_owned();
        let committed_at_ns = command.observed_at_ns();
        let event_count = u32::try_from(events.len()).map_err(|_| OmsError::ArithmeticOverflow)?;

        if matches!(command, OmsCommand::Submit { .. })
            && !self.orders.contains_key(&client_order_id)
        {
            let first = events
                .first()
                .ok_or_else(|| OmsError::OrderIdentityConflict(client_order_id.clone()))?;
            self.orders.insert(
                client_order_id.clone(),
                OrderAggregate::from_submission(first)?,
            );
            self.append_envelope(&client_order_id, 1, committed_at_ns, first.clone())?;
        } else {
            for event in events {
                let version = {
                    let aggregate = self
                        .orders
                        .get_mut(&client_order_id)
                        .ok_or_else(|| OmsError::UnknownOrder(client_order_id.clone()))?;
                    aggregate.apply(&event)?;
                    aggregate.version()
                };
                self.append_envelope(&client_order_id, version, committed_at_ns, event)?;
            }
        }

        let version = self
            .orders
            .get(&client_order_id)
            .ok_or_else(|| OmsError::UnknownOrder(client_order_id.clone()))?
            .version();
        let receipt = CommandReceipt {
            command_id: command_id.clone(),
            client_order_id,
            version,
            replayed: false,
            event_count,
        };
        self.commands.insert(
            command_id,
            AppliedCommand {
                command,
                receipt: receipt.clone(),
            },
        );
        Ok(receipt)
    }

    fn append_envelope(
        &mut self,
        client_order_id: &str,
        aggregate_version: u64,
        committed_at_ns: u64,
        event: OmsEvent,
    ) -> Result<(), OmsError> {
        let cursor = u64::try_from(self.events.len())
            .map_err(|_| OmsError::ArithmeticOverflow)?
            .checked_add(1)
            .ok_or(OmsError::ArithmeticOverflow)?;
        self.events.push(OmsEventEnvelope {
            schema_version: 1,
            cursor,
            event_id: format!(
                "oms:v1:{}:{}:{aggregate_version}",
                subject_token(&self.account_id),
                subject_token(client_order_id)
            ),
            account_id: self.account_id.clone(),
            client_order_id: client_order_id.to_owned(),
            aggregate_version,
            committed_at_ns,
            event,
        });
        Ok(())
    }
}

impl OmsCommandPort for ReferenceOms {
    fn execute(&mut self, command: OmsCommand) -> Result<CommandReceipt, OmsError> {
        self.execute_in_place(command)
    }
}

impl OmsQueryPort for ReferenceOms {
    fn capabilities(&self) -> OmsCapabilities {
        OmsCapabilities::helios_v1()
    }

    fn order(&self, client_order_id: &str) -> Result<Option<OrderSnapshot>, OmsError> {
        self.orders
            .get(client_order_id)
            .map(OrderAggregate::snapshot)
            .transpose()
    }

    fn orders(&self, limit: usize) -> Result<Vec<OrderSnapshot>, OmsError> {
        if limit == 0 {
            return Err(OmsError::InvalidOrderLimit);
        }
        self.orders
            .values()
            .take(limit)
            .map(OrderAggregate::snapshot)
            .collect()
    }
}

impl OmsEventSource for ReferenceOms {
    fn events_after(&self, cursor: u64, limit: usize) -> Result<Vec<OmsEventEnvelope>, OmsError> {
        if limit == 0 {
            return Err(OmsError::InvalidEventLimit);
        }
        let start = usize::try_from(cursor)
            .unwrap_or(usize::MAX)
            .min(self.events.len());
        Ok(self
            .events
            .iter()
            .skip(start)
            .take(limit)
            .cloned()
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use helio_execution::{
        ExecutionMode, MoneyMicros, OrderIntent, OrderProposal, PriceMicros, QuantityMicros, Side,
    };

    use super::*;
    use crate::{OrderState, TimeInForce};

    fn intent() -> OrderIntent {
        OrderIntent {
            client_order_id: "order-1".into(),
            proposal: OrderProposal {
                proposal_id: "proposal-1".into(),
                strategy_id: "strategy-1".into(),
                symbol: "SPY".into(),
                venue: "XNAS".into(),
                currency: "USD".into(),
                side: Side::Buy,
                quantity: QuantityMicros(2_000_000),
                limit_price: PriceMicros(50_000_000),
                mode: ExecutionMode::Paper,
                trading_day: 20260830,
            },
            authorized_notional: MoneyMicros(100_000_000),
            risk_policy_version: "risk-v1".into(),
            authorized_at_ns: 1,
        }
    }

    #[test]
    fn order_lifecycle_is_exact_replayable_and_cursor_driven() {
        let mut oms = ReferenceOms::try_new("paper-account").unwrap();
        let submit = OmsCommand::Submit {
            command_id: "cmd-submit".into(),
            intent: intent(),
            time_in_force: TimeInForce::Day,
            at_ns: 10,
        };
        assert!(!oms.execute(submit.clone()).unwrap().replayed);
        assert!(oms.execute(submit).unwrap().replayed);
        oms.execute(OmsCommand::Acknowledge {
            command_id: "cmd-ack".into(),
            client_order_id: "order-1".into(),
            broker_order_id: "venue-9".into(),
            at_ns: 11,
        })
        .unwrap();
        oms.execute(OmsCommand::RecordFill {
            command_id: "cmd-fill-1".into(),
            client_order_id: "order-1".into(),
            broker_order_id: None,
            execution_id: "exec-1".into(),
            venue_occurred_at: None,
            quantity: QuantityMicros(500_000),
            price: PriceMicros(49_500_000),
            at_ns: 12,
        })
        .unwrap();
        let snapshot = oms.order("order-1").unwrap().unwrap();
        assert_eq!(snapshot.state, OrderState::PartiallyFilled);
        assert_eq!(snapshot.average_fill_price, Some(PriceMicros(49_500_000)));
        assert_eq!(snapshot.filled_notional, MoneyMicros(24_750_000));
        assert_eq!(oms.events_after(1, 10).unwrap().len(), 2);
    }

    #[test]
    fn overfill_and_identity_conflicts_fail_closed() {
        let mut oms = ReferenceOms::try_new("paper-account").unwrap();
        oms.execute(OmsCommand::Submit {
            command_id: "submit".into(),
            intent: intent(),
            time_in_force: TimeInForce::Day,
            at_ns: 1,
        })
        .unwrap();
        oms.execute(OmsCommand::Acknowledge {
            command_id: "ack".into(),
            client_order_id: "order-1".into(),
            broker_order_id: "venue-1".into(),
            at_ns: 2,
        })
        .unwrap();
        assert_eq!(
            oms.execute(OmsCommand::RecordFill {
                command_id: "fill".into(),
                client_order_id: "order-1".into(),
                broker_order_id: None,
                execution_id: "exec".into(),
                venue_occurred_at: None,
                quantity: QuantityMicros(3_000_000),
                price: PriceMicros(50_000_000),
                at_ns: 3,
            }),
            Err(OmsError::Overfill)
        );
    }

    #[test]
    fn reference_oms_passes_the_external_contract() {
        let mut oms = ReferenceOms::try_new("conformance-account").unwrap();
        crate::verify_oms_conformance(&mut oms, intent(), 1_000).unwrap();
    }

    #[test]
    fn fills_preserve_pending_action_and_rejection_restores_working_state() {
        let mut oms = ReferenceOms::try_new("paper-account").unwrap();
        oms.execute(OmsCommand::Submit {
            command_id: "submit".into(),
            intent: intent(),
            time_in_force: TimeInForce::Day,
            at_ns: 1,
        })
        .unwrap();
        oms.execute(OmsCommand::Acknowledge {
            command_id: "ack".into(),
            client_order_id: "order-1".into(),
            broker_order_id: "venue-1".into(),
            at_ns: 2,
        })
        .unwrap();
        oms.execute(OmsCommand::RequestCancel {
            command_id: "cancel".into(),
            client_order_id: "order-1".into(),
            at_ns: 3,
        })
        .unwrap();
        oms.execute(OmsCommand::RecordFill {
            command_id: "fill".into(),
            client_order_id: "order-1".into(),
            broker_order_id: None,
            execution_id: "exec".into(),
            venue_occurred_at: None,
            quantity: QuantityMicros(500_000),
            price: PriceMicros(49_500_000),
            at_ns: 4,
        })
        .unwrap();
        assert_eq!(
            oms.order("order-1").unwrap().unwrap().state,
            OrderState::PendingCancel
        );
        oms.execute(OmsCommand::RejectPendingAction {
            command_id: "cancel-reject".into(),
            client_order_id: "order-1".into(),
            reason: "too late to cancel".into(),
            at_ns: 5,
        })
        .unwrap();
        assert_eq!(
            oms.order("order-1").unwrap().unwrap().state,
            OrderState::PartiallyFilled
        );
    }

    #[test]
    fn unknown_order_accepts_missing_fills_then_reconciles_explicitly() {
        let mut oms = ReferenceOms::try_new("paper-account").unwrap();
        oms.execute(OmsCommand::Submit {
            command_id: "submit".into(),
            intent: intent(),
            time_in_force: TimeInForce::Day,
            at_ns: 1,
        })
        .unwrap();
        oms.execute(OmsCommand::MarkUnknown {
            command_id: "unknown".into(),
            client_order_id: "order-1".into(),
            reason: "submission response was lost".into(),
            at_ns: 2,
        })
        .unwrap();
        oms.execute(OmsCommand::RecordFill {
            command_id: "recovered-fill".into(),
            client_order_id: "order-1".into(),
            broker_order_id: Some("venue-order".into()),
            execution_id: "venue-exec".into(),
            venue_occurred_at: Some("20260830-15:42:00.000".into()),
            quantity: QuantityMicros(2_000_000),
            price: PriceMicros(50_000_000),
            at_ns: 3,
        })
        .unwrap();
        assert_eq!(
            oms.order("order-1").unwrap().unwrap().state,
            OrderState::Unknown
        );
        oms.execute(OmsCommand::ReconcileUnknown {
            command_id: "reconcile".into(),
            client_order_id: "order-1".into(),
            broker_order_id: Some("venue-order".into()),
            state: crate::ReconciledState::Working,
            at_ns: 4,
        })
        .unwrap();
        let resolved = oms.order("order-1").unwrap().unwrap();
        assert_eq!(resolved.state, OrderState::Filled);
        assert_eq!(resolved.broker_order_id.as_deref(), Some("venue-order"));
        assert_eq!(resolved.uncertainty_reason, None);
    }

    #[test]
    fn observed_time_regression_is_rejected_without_mutation() {
        let mut oms = ReferenceOms::try_new("paper-account").unwrap();
        oms.execute(OmsCommand::Submit {
            command_id: "submit".into(),
            intent: intent(),
            time_in_force: TimeInForce::Day,
            at_ns: 10,
        })
        .unwrap();
        assert_eq!(
            oms.execute(OmsCommand::Acknowledge {
                command_id: "late-ack".into(),
                client_order_id: "order-1".into(),
                broker_order_id: "venue-order".into(),
                at_ns: 9,
            }),
            Err(OmsError::ObservationTimeRegression)
        );
        let snapshot = oms.order("order-1").unwrap().unwrap();
        assert_eq!(snapshot.state, OrderState::PendingSubmit);
        assert_eq!(snapshot.version, 1);
    }

    #[test]
    fn replacement_confirmation_rechecks_fills_that_arrived_while_pending() {
        let mut oms = ReferenceOms::try_new("paper-account").unwrap();
        oms.execute(OmsCommand::Submit {
            command_id: "submit".into(),
            intent: intent(),
            time_in_force: TimeInForce::Day,
            at_ns: 1,
        })
        .unwrap();
        oms.execute(OmsCommand::Acknowledge {
            command_id: "ack".into(),
            client_order_id: "order-1".into(),
            broker_order_id: "venue-order".into(),
            at_ns: 2,
        })
        .unwrap();
        oms.execute(OmsCommand::RequestReplace {
            command_id: "replace".into(),
            client_order_id: "order-1".into(),
            new_quantity: QuantityMicros(1_000_000),
            new_limit_price: PriceMicros(49_000_000),
            at_ns: 3,
        })
        .unwrap();
        oms.execute(OmsCommand::RecordFill {
            command_id: "fill".into(),
            client_order_id: "order-1".into(),
            broker_order_id: Some("venue-order".into()),
            execution_id: "execution".into(),
            venue_occurred_at: None,
            quantity: QuantityMicros(1_500_000),
            price: PriceMicros(49_500_000),
            at_ns: 4,
        })
        .unwrap();

        assert_eq!(
            oms.execute(OmsCommand::ConfirmReplaced {
                command_id: "replace-confirm".into(),
                client_order_id: "order-1".into(),
                broker_order_id: "venue-replacement".into(),
                at_ns: 5,
            }),
            Err(OmsError::ReplaceBelowFilled)
        );
        let snapshot = oms.order("order-1").unwrap().unwrap();
        assert_eq!(snapshot.state, OrderState::PendingReplace);
        assert_eq!(snapshot.working_quantity, QuantityMicros(2_000_000));
    }
}
