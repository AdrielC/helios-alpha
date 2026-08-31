use std::collections::HashSet;

use async_trait::async_trait;
use helio_oms::OmsEventEnvelope;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[cfg(feature = "golem")]
pub mod golem;
#[cfg(feature = "native-nats")]
pub mod nats;

pub const OMS_EVENT_SCHEMA_VERSION: u32 = 1;
pub const MAX_RELAY_BATCH_SIZE: usize = 1_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OmsEventBatch {
    pub next_cursor: u64,
    pub events: Vec<OmsEventEnvelope>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionCursorStatus {
    pub account_id: String,
    pub projection_id: String,
    pub cursor: u64,
    pub last_event_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishAck {
    pub stream_sequence: u64,
    pub duplicate: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CursorAdvanceReceipt {
    pub cursor: u64,
    pub event_id: String,
    pub replayed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelayRunReceipt {
    pub start_cursor: u64,
    pub end_cursor: u64,
    pub published: u32,
    pub duplicate_acks: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RelayPortError {
    #[error("OMS event source failed: {0}")]
    Source(String),
    #[error("acknowledged publisher failed: {0}")]
    Publisher(String),
    #[error("durable projection cursor failed: {0}")]
    Cursor(String),
}

#[async_trait]
pub trait DurableOmsEventSource: Send + Sync {
    async fn events_after(
        &self,
        cursor: u64,
        limit: usize,
    ) -> Result<OmsEventBatch, RelayPortError>;
}

#[async_trait]
pub trait AcknowledgedEventPublisher: Send + Sync {
    /// Resolves only after the persistence layer has acknowledged the event.
    async fn publish_and_ack(
        &self,
        event: &OmsEventEnvelope,
        payload: &[u8],
    ) -> Result<PublishAck, RelayPortError>;
}

#[async_trait]
pub trait DurableProjectionCursor: Send + Sync {
    async fn status(&self) -> Result<ProjectionCursorStatus, RelayPortError>;
    async fn advance(
        &self,
        expected_cursor: u64,
        next_cursor: u64,
        event_id: &str,
    ) -> Result<CursorAdvanceReceipt, RelayPortError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RelayError {
    #[error("relay account and projection identifiers must be non-empty")]
    InvalidIdentity,
    #[error("batch size must be between 1 and {MAX_RELAY_BATCH_SIZE}")]
    InvalidBatchSize,
    #[error(transparent)]
    Port(#[from] RelayPortError),
    #[error("cursor store identity did not match the relay configuration")]
    CursorIdentityMismatch,
    #[error("source returned {actual} events, exceeding the requested limit {limit}")]
    OversizedBatch { actual: usize, limit: usize },
    #[error("source returned schema version {actual} at cursor {cursor}, expected {expected}")]
    UnsupportedSchema {
        cursor: u64,
        expected: u32,
        actual: u32,
    },
    #[error("source returned an event for a different account at cursor {cursor}")]
    ForeignAccount { cursor: u64 },
    #[error("source returned empty event identity at cursor {cursor}")]
    EmptyEventIdentity { cursor: u64 },
    #[error("source returned duplicate event identity in one batch at cursor {cursor}")]
    DuplicateEventIdentity { cursor: u64 },
    #[error("source cursor gap: expected {expected}, received {actual}")]
    CursorGap { expected: u64, actual: u64 },
    #[error("source batch next cursor {actual} did not equal validated cursor {expected}")]
    BatchCursorMismatch { expected: u64, actual: u64 },
    #[error("cursor receipt did not match acknowledged event at cursor {cursor}")]
    CursorReceiptMismatch { cursor: u64 },
    #[error("OMS event serialization failed: {0}")]
    Serialization(String),
}

pub struct OmsEventRelay<S, P, C> {
    account_id: String,
    projection_id: String,
    batch_size: usize,
    source: S,
    publisher: P,
    cursor: C,
}

impl<S, P, C> OmsEventRelay<S, P, C>
where
    S: DurableOmsEventSource,
    P: AcknowledgedEventPublisher,
    C: DurableProjectionCursor,
{
    pub fn try_new(
        account_id: impl Into<String>,
        projection_id: impl Into<String>,
        batch_size: usize,
        source: S,
        publisher: P,
        cursor: C,
    ) -> Result<Self, RelayError> {
        let account_id = account_id.into();
        let projection_id = projection_id.into();
        if account_id.trim().is_empty() || projection_id.trim().is_empty() {
            return Err(RelayError::InvalidIdentity);
        }
        if batch_size == 0 || batch_size > MAX_RELAY_BATCH_SIZE {
            return Err(RelayError::InvalidBatchSize);
        }
        Ok(Self {
            account_id,
            projection_id,
            batch_size,
            source,
            publisher,
            cursor,
        })
    }

    pub async fn run_once(&self) -> Result<RelayRunReceipt, RelayError> {
        let status = self.cursor.status().await?;
        if status.account_id != self.account_id || status.projection_id != self.projection_id {
            return Err(RelayError::CursorIdentityMismatch);
        }

        let batch = self
            .source
            .events_after(status.cursor, self.batch_size)
            .await?;
        self.validate_batch(status.cursor, &batch)?;

        let start_cursor = status.cursor;
        let mut cursor = start_cursor;
        let mut duplicate_acks = 0_u32;
        for event in &batch.events {
            let payload = serde_json::to_vec(event)
                .map_err(|error| RelayError::Serialization(error.to_string()))?;
            let ack = self.publisher.publish_and_ack(event, &payload).await?;
            duplicate_acks = duplicate_acks.saturating_add(u32::from(ack.duplicate));

            let receipt = self
                .cursor
                .advance(cursor, event.cursor, &event.event_id)
                .await?;
            if receipt.cursor != event.cursor || receipt.event_id != event.event_id {
                return Err(RelayError::CursorReceiptMismatch {
                    cursor: event.cursor,
                });
            }
            cursor = event.cursor;
        }

        Ok(RelayRunReceipt {
            start_cursor,
            end_cursor: cursor,
            published: u32::try_from(batch.events.len()).unwrap_or(u32::MAX),
            duplicate_acks,
        })
    }

    fn validate_batch(&self, start_cursor: u64, batch: &OmsEventBatch) -> Result<(), RelayError> {
        if batch.events.len() > self.batch_size {
            return Err(RelayError::OversizedBatch {
                actual: batch.events.len(),
                limit: self.batch_size,
            });
        }

        let mut expected_cursor = start_cursor;
        let mut event_ids = HashSet::with_capacity(batch.events.len());
        for event in &batch.events {
            expected_cursor = expected_cursor
                .checked_add(1)
                .ok_or(RelayError::CursorGap {
                    expected: u64::MAX,
                    actual: event.cursor,
                })?;
            if event.cursor != expected_cursor {
                return Err(RelayError::CursorGap {
                    expected: expected_cursor,
                    actual: event.cursor,
                });
            }
            if event.schema_version != OMS_EVENT_SCHEMA_VERSION {
                return Err(RelayError::UnsupportedSchema {
                    cursor: event.cursor,
                    expected: OMS_EVENT_SCHEMA_VERSION,
                    actual: event.schema_version,
                });
            }
            if event.account_id != self.account_id {
                return Err(RelayError::ForeignAccount {
                    cursor: event.cursor,
                });
            }
            if event.event_id.trim().is_empty() {
                return Err(RelayError::EmptyEventIdentity {
                    cursor: event.cursor,
                });
            }
            if !event_ids.insert(event.event_id.as_str()) {
                return Err(RelayError::DuplicateEventIdentity {
                    cursor: event.cursor,
                });
            }
        }

        if batch.next_cursor != expected_cursor {
            return Err(RelayError::BatchCursorMismatch {
                expected: expected_cursor,
                actual: batch.next_cursor,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
