use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::OmsEvent;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OmsEventEnvelope {
    pub schema_version: u32,
    pub cursor: u64,
    pub event_id: String,
    pub account_id: String,
    pub client_order_id: String,
    pub aggregate_version: u64,
    pub committed_at_ns: u64,
    pub event: OmsEvent,
}

impl OmsEventEnvelope {
    pub fn nats_subject(&self) -> String {
        format!(
            "helios.oms.v1.account.{}.order.{}.event",
            subject_token(&self.account_id),
            subject_token(&self.client_order_id)
        )
    }

    /// Stable JetStream de-duplication value for the `Nats-Msg-Id` header.
    pub fn nats_message_id(&self) -> &str {
        &self.event_id
    }
}

pub fn subject_token(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len() * 2 + 1);
    encoded.push('x');
    for byte in value.as_bytes() {
        use std::fmt::Write;
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EventPublishError {
    #[error("event transport is unavailable")]
    Unavailable,
    #[error("event transport rejected committed cursor {0}")]
    Rejected(u64),
}

/// Non-authoritative projection port. Implementations publish only already committed OMS events.
pub trait OmsEventPublisher {
    fn publish(&mut self, event: &OmsEventEnvelope) -> Result<(), EventPublishError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subject_tokens_cannot_inject_nats_wildcards() {
        assert_eq!(subject_token("desk.*.>"), "x6465736b2e2a2e3e");
    }
}
