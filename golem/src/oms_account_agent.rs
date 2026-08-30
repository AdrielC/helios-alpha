use golem_rust::{Schema, agent_definition, agent_implementation};
use helio_execution::{
    ExecutionMode, MoneyMicros, OrderIntent, OrderProposal, PriceMicros, QuantityMicros, Side,
};
use helio_oms::{
    CommandReceipt, OmsCommand, OmsCommandPort, OmsEventSource, OmsQueryPort, OrderSnapshot,
    OrderState, ReconciledState, ReferenceOms, TimeInForce,
};
use serde::{Deserialize, Serialize};

const SNAPSHOT_FORMAT_VERSION: u32 = 1;
const MAX_EVENT_BATCH_SIZE: u32 = 1_024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Schema, Serialize, Deserialize)]
pub enum SideInput {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Schema, Serialize, Deserialize)]
pub enum ExecutionModeInput {
    Paper,
    Live,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Schema, Serialize, Deserialize)]
pub enum TimeInForceInput {
    Day,
    GoodTillCanceled,
    ImmediateOrCancel,
    FillOrKill,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Schema, Serialize, Deserialize)]
pub enum ReconciledStateInput {
    Working,
    Canceled,
    Rejected,
    Expired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Schema, Serialize, Deserialize)]
pub enum OrderStateOutput {
    PendingSubmit,
    Working,
    PartiallyFilled,
    PendingCancel,
    PendingReplace,
    Filled,
    Canceled,
    Rejected,
    Expired,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Schema, Serialize, Deserialize)]
pub struct OrderIntentInput {
    pub client_order_id: String,
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
    pub authorized_notional_micros: u64,
    pub risk_policy_version: String,
    pub authorized_at_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Schema, Serialize, Deserialize)]
pub struct SubmitOrderInput {
    pub command_id: String,
    pub intent: OrderIntentInput,
    pub time_in_force: TimeInForceInput,
    pub at_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Schema, Serialize, Deserialize)]
pub struct VenueAcknowledgementInput {
    pub command_id: String,
    pub client_order_id: String,
    pub broker_order_id: String,
    pub at_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Schema, Serialize, Deserialize)]
pub struct FillInput {
    pub command_id: String,
    pub client_order_id: String,
    pub broker_order_id: Option<String>,
    pub execution_id: String,
    pub venue_occurred_at: Option<String>,
    pub quantity_micros: u64,
    pub price_micros: u64,
    pub at_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Schema, Serialize, Deserialize)]
pub struct OrderActionInput {
    pub command_id: String,
    pub client_order_id: String,
    pub at_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Schema, Serialize, Deserialize)]
pub struct OrderReasonInput {
    pub command_id: String,
    pub client_order_id: String,
    pub reason: String,
    pub at_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Schema, Serialize, Deserialize)]
pub struct ReplaceOrderInput {
    pub command_id: String,
    pub client_order_id: String,
    pub new_quantity_micros: u64,
    pub new_limit_price_micros: u64,
    pub at_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Schema, Serialize, Deserialize)]
pub struct ConfirmReplaceInput {
    pub command_id: String,
    pub client_order_id: String,
    pub broker_order_id: String,
    pub at_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Schema, Serialize, Deserialize)]
pub struct RejectPendingActionInput {
    pub command_id: String,
    pub client_order_id: String,
    pub reason: String,
    pub at_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Schema, Serialize, Deserialize)]
pub struct ReconcileUnknownInput {
    pub command_id: String,
    pub client_order_id: String,
    pub broker_order_id: Option<String>,
    pub state: ReconciledStateInput,
    pub at_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Schema, Serialize, Deserialize)]
pub struct CommandReceiptOutput {
    pub command_id: String,
    pub client_order_id: String,
    pub version: u64,
    pub replayed: bool,
    pub event_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Schema, Serialize, Deserialize)]
pub struct OrderView {
    pub client_order_id: String,
    pub broker_order_id: Option<String>,
    pub state: OrderStateOutput,
    pub symbol: String,
    pub venue: String,
    pub side: SideInput,
    pub time_in_force: TimeInForceInput,
    pub working_quantity_micros: u64,
    pub working_limit_price_micros: u64,
    pub filled_quantity_micros: u64,
    pub average_fill_price_micros: Option<u64>,
    pub filled_notional_micros: u64,
    pub version: u64,
    pub last_update_at_ns: u64,
    pub uncertainty_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Schema, Serialize, Deserialize)]
pub struct EventBatchOutput {
    pub next_cursor: u64,
    /// Canonical `OmsEventEnvelope` JSON. Schema versioning lives inside each envelope.
    pub events_json: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Schema, Serialize, Deserialize)]
pub enum OmsAgentError {
    NotInitialized { detail: String },
    CommandRejected { detail: String },
    SerializationFailed { detail: String },
    EventBatchCapacityExceeded { found: u32, capacity: u32 },
}

#[agent_definition(snapshotting = "periodic(30s)")]
pub trait OmsAccountAgent {
    /// The account identifier is the durable placement key and the OMS tenancy boundary.
    fn new(account_id: String) -> Self;

    fn submit(&mut self, input: SubmitOrderInput) -> Result<CommandReceiptOutput, OmsAgentError>;
    fn acknowledge(
        &mut self,
        input: VenueAcknowledgementInput,
    ) -> Result<CommandReceiptOutput, OmsAgentError>;
    fn reject(&mut self, input: OrderReasonInput) -> Result<CommandReceiptOutput, OmsAgentError>;
    fn record_fill(&mut self, input: FillInput) -> Result<CommandReceiptOutput, OmsAgentError>;
    fn request_cancel(
        &mut self,
        input: OrderActionInput,
    ) -> Result<CommandReceiptOutput, OmsAgentError>;
    fn confirm_canceled(
        &mut self,
        input: OrderActionInput,
    ) -> Result<CommandReceiptOutput, OmsAgentError>;
    fn request_replace(
        &mut self,
        input: ReplaceOrderInput,
    ) -> Result<CommandReceiptOutput, OmsAgentError>;
    fn confirm_replaced(
        &mut self,
        input: ConfirmReplaceInput,
    ) -> Result<CommandReceiptOutput, OmsAgentError>;
    fn reject_pending_action(
        &mut self,
        input: RejectPendingActionInput,
    ) -> Result<CommandReceiptOutput, OmsAgentError>;
    fn mark_expired(
        &mut self,
        input: OrderActionInput,
    ) -> Result<CommandReceiptOutput, OmsAgentError>;
    fn mark_unknown(
        &mut self,
        input: OrderReasonInput,
    ) -> Result<CommandReceiptOutput, OmsAgentError>;
    fn reconcile_unknown(
        &mut self,
        input: ReconcileUnknownInput,
    ) -> Result<CommandReceiptOutput, OmsAgentError>;
    fn order(&self, client_order_id: String) -> Result<Option<OrderView>, OmsAgentError>;
    fn events_after(&self, cursor: u64, limit: u32) -> Result<EventBatchOutput, OmsAgentError>;
}

struct OmsAccountAgentImpl {
    account_id: String,
    oms: Option<ReferenceOms>,
    initialization_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AgentSnapshot {
    format_version: u32,
    account_id: String,
    oms: ReferenceOms,
}

#[agent_implementation]
impl OmsAccountAgent for OmsAccountAgentImpl {
    fn new(account_id: String) -> Self {
        let result = ReferenceOms::try_new(account_id.clone());
        let (oms, initialization_error) = match result {
            Ok(oms) => (Some(oms), None),
            Err(error) => (None, Some(error.to_string())),
        };
        Self {
            account_id,
            oms,
            initialization_error,
        }
    }

    fn submit(&mut self, input: SubmitOrderInput) -> Result<CommandReceiptOutput, OmsAgentError> {
        self.execute(OmsCommand::Submit {
            command_id: input.command_id,
            intent: input.intent.into(),
            time_in_force: input.time_in_force.into(),
            at_ns: input.at_ns,
        })
    }

    fn acknowledge(
        &mut self,
        input: VenueAcknowledgementInput,
    ) -> Result<CommandReceiptOutput, OmsAgentError> {
        self.execute(OmsCommand::Acknowledge {
            command_id: input.command_id,
            client_order_id: input.client_order_id,
            broker_order_id: input.broker_order_id,
            at_ns: input.at_ns,
        })
    }

    fn reject(&mut self, input: OrderReasonInput) -> Result<CommandReceiptOutput, OmsAgentError> {
        self.execute(OmsCommand::Reject {
            command_id: input.command_id,
            client_order_id: input.client_order_id,
            reason: input.reason,
            at_ns: input.at_ns,
        })
    }

    fn record_fill(&mut self, input: FillInput) -> Result<CommandReceiptOutput, OmsAgentError> {
        self.execute(OmsCommand::RecordFill {
            command_id: input.command_id,
            client_order_id: input.client_order_id,
            broker_order_id: input.broker_order_id,
            execution_id: input.execution_id,
            venue_occurred_at: input.venue_occurred_at,
            quantity: QuantityMicros(input.quantity_micros),
            price: PriceMicros(input.price_micros),
            at_ns: input.at_ns,
        })
    }

    fn request_cancel(
        &mut self,
        input: OrderActionInput,
    ) -> Result<CommandReceiptOutput, OmsAgentError> {
        self.execute(OmsCommand::RequestCancel {
            command_id: input.command_id,
            client_order_id: input.client_order_id,
            at_ns: input.at_ns,
        })
    }

    fn confirm_canceled(
        &mut self,
        input: OrderActionInput,
    ) -> Result<CommandReceiptOutput, OmsAgentError> {
        self.execute(OmsCommand::ConfirmCanceled {
            command_id: input.command_id,
            client_order_id: input.client_order_id,
            at_ns: input.at_ns,
        })
    }

    fn request_replace(
        &mut self,
        input: ReplaceOrderInput,
    ) -> Result<CommandReceiptOutput, OmsAgentError> {
        self.execute(OmsCommand::RequestReplace {
            command_id: input.command_id,
            client_order_id: input.client_order_id,
            new_quantity: QuantityMicros(input.new_quantity_micros),
            new_limit_price: PriceMicros(input.new_limit_price_micros),
            at_ns: input.at_ns,
        })
    }

    fn confirm_replaced(
        &mut self,
        input: ConfirmReplaceInput,
    ) -> Result<CommandReceiptOutput, OmsAgentError> {
        self.execute(OmsCommand::ConfirmReplaced {
            command_id: input.command_id,
            client_order_id: input.client_order_id,
            broker_order_id: input.broker_order_id,
            at_ns: input.at_ns,
        })
    }

    fn reject_pending_action(
        &mut self,
        input: RejectPendingActionInput,
    ) -> Result<CommandReceiptOutput, OmsAgentError> {
        self.execute(OmsCommand::RejectPendingAction {
            command_id: input.command_id,
            client_order_id: input.client_order_id,
            reason: input.reason,
            at_ns: input.at_ns,
        })
    }

    fn mark_expired(
        &mut self,
        input: OrderActionInput,
    ) -> Result<CommandReceiptOutput, OmsAgentError> {
        self.execute(OmsCommand::MarkExpired {
            command_id: input.command_id,
            client_order_id: input.client_order_id,
            at_ns: input.at_ns,
        })
    }

    fn mark_unknown(
        &mut self,
        input: OrderReasonInput,
    ) -> Result<CommandReceiptOutput, OmsAgentError> {
        self.execute(OmsCommand::MarkUnknown {
            command_id: input.command_id,
            client_order_id: input.client_order_id,
            reason: input.reason,
            at_ns: input.at_ns,
        })
    }

    fn reconcile_unknown(
        &mut self,
        input: ReconcileUnknownInput,
    ) -> Result<CommandReceiptOutput, OmsAgentError> {
        self.execute(OmsCommand::ReconcileUnknown {
            command_id: input.command_id,
            client_order_id: input.client_order_id,
            broker_order_id: input.broker_order_id,
            state: input.state.into(),
            at_ns: input.at_ns,
        })
    }

    fn order(&self, client_order_id: String) -> Result<Option<OrderView>, OmsAgentError> {
        self.oms_ref()?
            .order(&client_order_id)
            .map_err(agent_error)
            .map(|snapshot| snapshot.map(OrderView::from))
    }

    fn events_after(&self, cursor: u64, limit: u32) -> Result<EventBatchOutput, OmsAgentError> {
        if limit > MAX_EVENT_BATCH_SIZE {
            return Err(OmsAgentError::EventBatchCapacityExceeded {
                found: limit,
                capacity: MAX_EVENT_BATCH_SIZE,
            });
        }
        let events = self
            .oms_ref()?
            .events_after(cursor, limit as usize)
            .map_err(agent_error)?;
        let next_cursor = events.last().map_or(cursor, |event| event.cursor);
        let events_json = events
            .iter()
            .map(|event| {
                serde_json::to_string(event).map_err(|error| OmsAgentError::SerializationFailed {
                    detail: error.to_string(),
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(EventBatchOutput {
            next_cursor,
            events_json,
        })
    }

    async fn save_snapshot(&self) -> Result<Vec<u8>, String> {
        let oms = self.oms_ref().map_err(|error| format!("{error:?}"))?;
        serde_json::to_vec(&AgentSnapshot {
            format_version: SNAPSHOT_FORMAT_VERSION,
            account_id: self.account_id.clone(),
            oms: oms.clone(),
        })
        .map_err(|error| format!("failed to encode OMS account snapshot: {error}"))
    }

    async fn load_snapshot(&mut self, bytes: Vec<u8>) -> Result<(), String> {
        let snapshot: AgentSnapshot = serde_json::from_slice(&bytes)
            .map_err(|error| format!("failed to decode OMS account snapshot: {error}"))?;
        if snapshot.format_version != SNAPSHOT_FORMAT_VERSION {
            return Err(format!(
                "unsupported OMS snapshot version {}; expected {}",
                snapshot.format_version, SNAPSHOT_FORMAT_VERSION
            ));
        }
        if snapshot.account_id != self.account_id {
            return Err("OMS snapshot account does not match the agent identity".into());
        }
        self.oms = Some(snapshot.oms);
        self.initialization_error = None;
        Ok(())
    }
}

impl OmsAccountAgentImpl {
    fn execute(&mut self, command: OmsCommand) -> Result<CommandReceiptOutput, OmsAgentError> {
        self.oms_mut()?
            .execute(command)
            .map(CommandReceiptOutput::from)
            .map_err(agent_error)
    }

    fn oms_ref(&self) -> Result<&ReferenceOms, OmsAgentError> {
        self.oms
            .as_ref()
            .ok_or_else(|| OmsAgentError::NotInitialized {
                detail: self
                    .initialization_error
                    .clone()
                    .unwrap_or_else(|| "durable OMS state is unavailable".into()),
            })
    }

    fn oms_mut(&mut self) -> Result<&mut ReferenceOms, OmsAgentError> {
        let detail = self
            .initialization_error
            .clone()
            .unwrap_or_else(|| "durable OMS state is unavailable".into());
        self.oms
            .as_mut()
            .ok_or(OmsAgentError::NotInitialized { detail })
    }
}

impl From<OrderIntentInput> for OrderIntent {
    fn from(input: OrderIntentInput) -> Self {
        Self {
            client_order_id: input.client_order_id,
            proposal: OrderProposal {
                proposal_id: input.proposal_id,
                strategy_id: input.strategy_id,
                symbol: input.symbol,
                venue: input.venue,
                currency: input.currency,
                side: input.side.into(),
                quantity: QuantityMicros(input.quantity_micros),
                limit_price: PriceMicros(input.limit_price_micros),
                mode: input.execution_mode.into(),
                trading_day: input.trading_day,
            },
            authorized_notional: MoneyMicros(input.authorized_notional_micros),
            risk_policy_version: input.risk_policy_version,
            authorized_at_ns: input.authorized_at_ns,
        }
    }
}

impl From<SideInput> for Side {
    fn from(side: SideInput) -> Self {
        match side {
            SideInput::Buy => Self::Buy,
            SideInput::Sell => Self::Sell,
        }
    }
}

impl From<Side> for SideInput {
    fn from(side: Side) -> Self {
        match side {
            Side::Buy => Self::Buy,
            Side::Sell => Self::Sell,
        }
    }
}

impl From<ExecutionModeInput> for ExecutionMode {
    fn from(mode: ExecutionModeInput) -> Self {
        match mode {
            ExecutionModeInput::Paper => Self::Paper,
            ExecutionModeInput::Live => Self::Live,
        }
    }
}

impl From<TimeInForceInput> for TimeInForce {
    fn from(time_in_force: TimeInForceInput) -> Self {
        match time_in_force {
            TimeInForceInput::Day => Self::Day,
            TimeInForceInput::GoodTillCanceled => Self::GoodTillCanceled,
            TimeInForceInput::ImmediateOrCancel => Self::ImmediateOrCancel,
            TimeInForceInput::FillOrKill => Self::FillOrKill,
        }
    }
}

impl From<TimeInForce> for TimeInForceInput {
    fn from(time_in_force: TimeInForce) -> Self {
        match time_in_force {
            TimeInForce::Day => Self::Day,
            TimeInForce::GoodTillCanceled => Self::GoodTillCanceled,
            TimeInForce::ImmediateOrCancel => Self::ImmediateOrCancel,
            TimeInForce::FillOrKill => Self::FillOrKill,
        }
    }
}

impl From<OrderState> for OrderStateOutput {
    fn from(state: OrderState) -> Self {
        match state {
            OrderState::PendingSubmit => Self::PendingSubmit,
            OrderState::Working => Self::Working,
            OrderState::PartiallyFilled => Self::PartiallyFilled,
            OrderState::PendingCancel => Self::PendingCancel,
            OrderState::PendingReplace => Self::PendingReplace,
            OrderState::Filled => Self::Filled,
            OrderState::Canceled => Self::Canceled,
            OrderState::Rejected => Self::Rejected,
            OrderState::Expired => Self::Expired,
            OrderState::Unknown => Self::Unknown,
        }
    }
}

impl From<ReconciledStateInput> for ReconciledState {
    fn from(state: ReconciledStateInput) -> Self {
        match state {
            ReconciledStateInput::Working => Self::Working,
            ReconciledStateInput::Canceled => Self::Canceled,
            ReconciledStateInput::Rejected => Self::Rejected,
            ReconciledStateInput::Expired => Self::Expired,
        }
    }
}

impl From<CommandReceipt> for CommandReceiptOutput {
    fn from(receipt: CommandReceipt) -> Self {
        Self {
            command_id: receipt.command_id,
            client_order_id: receipt.client_order_id,
            version: receipt.version,
            replayed: receipt.replayed,
            event_count: receipt.event_count,
        }
    }
}

impl From<OrderSnapshot> for OrderView {
    fn from(snapshot: OrderSnapshot) -> Self {
        Self {
            client_order_id: snapshot.client_order_id,
            broker_order_id: snapshot.broker_order_id,
            state: snapshot.state.into(),
            symbol: snapshot.intent.proposal.symbol,
            venue: snapshot.intent.proposal.venue,
            side: snapshot.intent.proposal.side.into(),
            time_in_force: snapshot.time_in_force.into(),
            working_quantity_micros: snapshot.working_quantity.0,
            working_limit_price_micros: snapshot.working_limit_price.0,
            filled_quantity_micros: snapshot.filled_quantity.0,
            average_fill_price_micros: snapshot.average_fill_price.map(|price| price.0),
            filled_notional_micros: snapshot.filled_notional.0,
            version: snapshot.version,
            last_update_at_ns: snapshot.last_update_at_ns,
            uncertainty_reason: snapshot.uncertainty_reason,
        }
    }
}

fn agent_error(error: impl ToString) -> OmsAgentError {
    OmsAgentError::CommandRejected {
        detail: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn intent() -> OrderIntentInput {
        OrderIntentInput {
            client_order_id: "golem-order-1".into(),
            proposal_id: "proposal-1".into(),
            strategy_id: "strategy-1".into(),
            symbol: "SPY".into(),
            venue: "XNAS".into(),
            currency: "USD".into(),
            side: SideInput::Buy,
            quantity_micros: 2_000_000,
            limit_price_micros: 50_000_000,
            execution_mode: ExecutionModeInput::Paper,
            trading_day: 20260830,
            authorized_notional_micros: 100_000_000,
            risk_policy_version: "risk-v1".into(),
            authorized_at_ns: 1,
        }
    }

    #[test]
    fn typed_agent_boundary_preserves_replay_fill_and_event_cursor() {
        let mut agent = OmsAccountAgentImpl::new("paper-account".into());
        let submit = SubmitOrderInput {
            command_id: "submit-1".into(),
            intent: intent(),
            time_in_force: TimeInForceInput::Day,
            at_ns: 10,
        };
        assert!(!agent.submit(submit.clone()).unwrap().replayed);
        assert!(agent.submit(submit).unwrap().replayed);
        agent
            .acknowledge(VenueAcknowledgementInput {
                command_id: "ack-1".into(),
                client_order_id: "golem-order-1".into(),
                broker_order_id: "venue-order-1".into(),
                at_ns: 11,
            })
            .unwrap();
        agent
            .record_fill(FillInput {
                command_id: "fill-1".into(),
                client_order_id: "golem-order-1".into(),
                broker_order_id: Some("venue-order-1".into()),
                execution_id: "execution-1".into(),
                venue_occurred_at: Some("20260830-15:42:00.000".into()),
                quantity_micros: 500_000,
                price_micros: 49_500_000,
                at_ns: 12,
            })
            .unwrap();

        let order = agent.order("golem-order-1".into()).unwrap().unwrap();
        assert_eq!(order.state, OrderStateOutput::PartiallyFilled);
        assert_eq!(order.filled_notional_micros, 24_750_000);
        let events = agent.events_after(0, 16).unwrap();
        assert_eq!(events.next_cursor, 3);
        assert_eq!(events.events_json.len(), 3);
    }
}
