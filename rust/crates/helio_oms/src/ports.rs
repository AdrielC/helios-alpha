use crate::{
    CommandReceipt, OmsCapabilities, OmsCommand, OmsError, OmsEventEnvelope, OrderSnapshot,
};

pub trait OmsCommandPort {
    fn execute(&mut self, command: OmsCommand) -> Result<CommandReceipt, OmsError>;
}

pub trait OmsQueryPort {
    fn capabilities(&self) -> OmsCapabilities;
    fn order(&self, client_order_id: &str) -> Result<Option<OrderSnapshot>, OmsError>;
    /// Returns a deterministic, bounded account-order snapshot for reconciliation.
    fn orders(&self, limit: usize) -> Result<Vec<OrderSnapshot>, OmsError>;
}

pub trait OmsEventSource {
    fn events_after(&self, cursor: u64, limit: usize) -> Result<Vec<OmsEventEnvelope>, OmsError>;
}

/// Stable integration surface for the built-in OMS and independently deployed OMS products.
pub trait OmsPort: OmsCommandPort + OmsQueryPort + OmsEventSource {}

impl<T> OmsPort for T where T: OmsCommandPort + OmsQueryPort + OmsEventSource {}
