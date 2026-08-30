use helio_execution::{MoneyMicros, OrderIntent, PriceMicros, QuantityMicros};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeInForce {
    Day,
    GoodTillCanceled,
    ImmediateOrCancel,
    FillOrKill,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderState {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReconciledState {
    Working,
    Canceled,
    Rejected,
    Expired,
}

impl OrderState {
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Filled | Self::Canceled | Self::Rejected | Self::Expired
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OmsCommand {
    Submit {
        command_id: String,
        intent: OrderIntent,
        time_in_force: TimeInForce,
        at_ns: u64,
    },
    Acknowledge {
        command_id: String,
        client_order_id: String,
        broker_order_id: String,
        at_ns: u64,
    },
    Reject {
        command_id: String,
        client_order_id: String,
        reason: String,
        at_ns: u64,
    },
    RecordFill {
        command_id: String,
        client_order_id: String,
        broker_order_id: Option<String>,
        execution_id: String,
        venue_occurred_at: Option<String>,
        quantity: QuantityMicros,
        price: PriceMicros,
        at_ns: u64,
    },
    RequestCancel {
        command_id: String,
        client_order_id: String,
        at_ns: u64,
    },
    ConfirmCanceled {
        command_id: String,
        client_order_id: String,
        at_ns: u64,
    },
    RequestReplace {
        command_id: String,
        client_order_id: String,
        new_quantity: QuantityMicros,
        new_limit_price: PriceMicros,
        at_ns: u64,
    },
    ConfirmReplaced {
        command_id: String,
        client_order_id: String,
        broker_order_id: String,
        at_ns: u64,
    },
    RejectPendingAction {
        command_id: String,
        client_order_id: String,
        reason: String,
        at_ns: u64,
    },
    MarkExpired {
        command_id: String,
        client_order_id: String,
        at_ns: u64,
    },
    MarkUnknown {
        command_id: String,
        client_order_id: String,
        reason: String,
        at_ns: u64,
    },
    ReconcileUnknown {
        command_id: String,
        client_order_id: String,
        broker_order_id: Option<String>,
        state: ReconciledState,
        at_ns: u64,
    },
}

impl OmsCommand {
    pub fn command_id(&self) -> &str {
        match self {
            Self::Submit { command_id, .. }
            | Self::Acknowledge { command_id, .. }
            | Self::Reject { command_id, .. }
            | Self::RecordFill { command_id, .. }
            | Self::RequestCancel { command_id, .. }
            | Self::ConfirmCanceled { command_id, .. }
            | Self::RequestReplace { command_id, .. }
            | Self::ConfirmReplaced { command_id, .. }
            | Self::RejectPendingAction { command_id, .. }
            | Self::MarkExpired { command_id, .. }
            | Self::MarkUnknown { command_id, .. }
            | Self::ReconcileUnknown { command_id, .. } => command_id,
        }
    }

    pub fn client_order_id(&self) -> &str {
        match self {
            Self::Submit { intent, .. } => &intent.client_order_id,
            Self::Acknowledge {
                client_order_id, ..
            }
            | Self::Reject {
                client_order_id, ..
            }
            | Self::RecordFill {
                client_order_id, ..
            }
            | Self::RequestCancel {
                client_order_id, ..
            }
            | Self::ConfirmCanceled {
                client_order_id, ..
            }
            | Self::RequestReplace {
                client_order_id, ..
            }
            | Self::ConfirmReplaced {
                client_order_id, ..
            }
            | Self::RejectPendingAction {
                client_order_id, ..
            }
            | Self::MarkExpired {
                client_order_id, ..
            }
            | Self::MarkUnknown {
                client_order_id, ..
            }
            | Self::ReconcileUnknown {
                client_order_id, ..
            } => client_order_id,
        }
    }

    pub const fn observed_at_ns(&self) -> u64 {
        match self {
            Self::Submit { at_ns, .. }
            | Self::Acknowledge { at_ns, .. }
            | Self::Reject { at_ns, .. }
            | Self::RecordFill { at_ns, .. }
            | Self::RequestCancel { at_ns, .. }
            | Self::ConfirmCanceled { at_ns, .. }
            | Self::RequestReplace { at_ns, .. }
            | Self::ConfirmReplaced { at_ns, .. }
            | Self::RejectPendingAction { at_ns, .. }
            | Self::MarkExpired { at_ns, .. }
            | Self::MarkUnknown { at_ns, .. }
            | Self::ReconcileUnknown { at_ns, .. } => *at_ns,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OmsEvent {
    Submitted {
        intent: OrderIntent,
        time_in_force: TimeInForce,
        at_ns: u64,
    },
    Acknowledged {
        broker_order_id: String,
        at_ns: u64,
    },
    Rejected {
        reason: String,
        at_ns: u64,
    },
    FillRecorded {
        broker_order_id: Option<String>,
        execution_id: String,
        venue_occurred_at: Option<String>,
        quantity: QuantityMicros,
        price: PriceMicros,
        at_ns: u64,
    },
    CancelRequested {
        at_ns: u64,
    },
    Canceled {
        at_ns: u64,
    },
    ReplaceRequested {
        new_quantity: QuantityMicros,
        new_limit_price: PriceMicros,
        at_ns: u64,
    },
    Replaced {
        broker_order_id: String,
        at_ns: u64,
    },
    PendingActionRejected {
        reason: String,
        at_ns: u64,
    },
    Expired {
        at_ns: u64,
    },
    MarkedUnknown {
        reason: String,
        at_ns: u64,
    },
    UnknownReconciled {
        broker_order_id: Option<String>,
        state: ReconciledState,
        at_ns: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderSnapshot {
    pub client_order_id: String,
    pub broker_order_id: Option<String>,
    pub state: OrderState,
    pub intent: OrderIntent,
    pub time_in_force: TimeInForce,
    pub working_quantity: QuantityMicros,
    pub working_limit_price: PriceMicros,
    pub filled_quantity: QuantityMicros,
    pub average_fill_price: Option<PriceMicros>,
    pub filled_notional: MoneyMicros,
    pub version: u64,
    pub last_update_at_ns: u64,
    pub uncertainty_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandReceipt {
    pub command_id: String,
    pub client_order_id: String,
    pub version: u64,
    pub replayed: bool,
    pub event_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OmsCapabilities {
    pub protocol_version: u32,
    pub supports_limit_orders: bool,
    pub supports_cancel: bool,
    pub supports_replace: bool,
    pub supports_fractional_quantity: bool,
    pub supports_event_cursor: bool,
    pub time_in_force: Vec<TimeInForce>,
}

impl OmsCapabilities {
    pub fn helios_v1() -> Self {
        Self {
            protocol_version: 1,
            supports_limit_orders: true,
            supports_cancel: true,
            supports_replace: true,
            supports_fractional_quantity: true,
            supports_event_cursor: true,
            time_in_force: vec![
                TimeInForce::Day,
                TimeInForce::GoodTillCanceled,
                TimeInForce::ImmediateOrCancel,
                TimeInForce::FillOrKill,
            ],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum OmsError {
    #[error("command and order identities must not be empty")]
    EmptyIdentity,
    #[error("audit reasons must not be empty")]
    EmptyReason,
    #[error("unknown order {0}")]
    UnknownOrder(String),
    #[error("order {0} already exists with different contents")]
    OrderIdentityConflict(String),
    #[error("command {0} was replayed with different contents")]
    CommandIdentityConflict(String),
    #[error("invalid transition from {state:?}: {operation}")]
    InvalidTransition {
        state: OrderState,
        operation: String,
    },
    #[error("fill identity {0} was replayed with different contents")]
    ExecutionIdentityConflict(String),
    #[error("venue order identity conflicts with the existing order")]
    BrokerIdentityConflict,
    #[error("fill exceeds the order's remaining quantity")]
    Overfill,
    #[error("replacement quantity is below already filled quantity")]
    ReplaceBelowFilled,
    #[error("quantity and price must be nonzero")]
    ZeroValue,
    #[error("fixed-point arithmetic overflowed")]
    ArithmeticOverflow,
    #[error("order aggregate version overflowed")]
    VersionOverflow,
    #[error("event cursor capacity must be nonzero")]
    InvalidEventLimit,
    #[error("reconciliation report conflicts with recorded executions")]
    ReconciliationConflict,
    #[error("order observation time regressed")]
    ObservationTimeRegression,
}
