use std::str::FromStr;
use std::time::Duration;

use async_nats::jetstream::stream::{
    Config as StreamConfig, DiscardPolicy, RetentionPolicy, StorageType,
};
use async_nats::{jetstream, ConnectOptions, HeaderMap, HeaderValue};
use async_trait::async_trait;
use helio_oms::OmsEventEnvelope;

use crate::{AcknowledgedEventPublisher, PublishAck, RelayPortError};

pub const NATS_MESSAGE_ID_HEADER: &str = "Nats-Msg-Id";

#[derive(Clone, PartialEq, Eq)]
pub struct NatsConnectionSettings {
    pub url: String,
    pub token: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JetStreamSettings {
    pub connection: NatsConnectionSettings,
    pub stream_name: String,
    pub subjects: Vec<String>,
    pub max_bytes: i64,
    pub max_messages: i64,
    pub max_age: Duration,
    pub duplicate_window: Duration,
    pub replicas: usize,
    /// Development/bootstrap only. Production publishers should have no stream-management grant.
    pub allow_create: bool,
}

impl JetStreamSettings {
    pub fn development(connection: NatsConnectionSettings) -> Self {
        Self {
            connection,
            stream_name: "HELIOS_OMS_V1".into(),
            subjects: vec!["helios.oms.v1.>".into()],
            max_bytes: 10 * 1024 * 1024 * 1024,
            max_messages: 10_000_000,
            max_age: Duration::from_secs(7 * 24 * 60 * 60),
            duplicate_window: Duration::from_secs(10 * 60),
            replicas: 1,
            allow_create: true,
        }
    }

    fn stream_config(&self) -> Result<StreamConfig, RelayPortError> {
        self.connection.validate()?;
        if self.stream_name.trim().is_empty()
            || self.subjects.is_empty()
            || self
                .subjects
                .iter()
                .any(|subject| subject.trim().is_empty())
            || self.max_bytes <= 0
            || self.max_messages <= 0
            || self.max_age.is_zero()
            || self.duplicate_window.is_zero()
            || !(1..=5).contains(&self.replicas)
        {
            return Err(RelayPortError::Publisher(
                "invalid bounded JetStream configuration".into(),
            ));
        }
        Ok(StreamConfig {
            name: self.stream_name.clone(),
            description: Some("Helios committed OMS events v1".into()),
            subjects: self.subjects.clone(),
            max_bytes: self.max_bytes,
            max_messages: self.max_messages,
            max_age: self.max_age,
            duplicate_window: self.duplicate_window,
            num_replicas: self.replicas,
            storage: StorageType::File,
            retention: RetentionPolicy::Limits,
            discard: DiscardPolicy::Old,
            deny_delete: true,
            deny_purge: true,
            ..Default::default()
        })
    }
}

impl std::fmt::Debug for NatsConnectionSettings {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NatsConnectionSettings")
            .field("url", &self.url)
            .field("token", &self.token.as_ref().map(|_| "[redacted]"))
            .finish()
    }
}

impl NatsConnectionSettings {
    pub fn validate(&self) -> Result<(), RelayPortError> {
        if self.url.trim().is_empty() {
            return Err(RelayPortError::Publisher(
                "NATS URL must not be empty".into(),
            ));
        }
        if self
            .token
            .as_ref()
            .is_some_and(|token| token.trim().is_empty())
        {
            return Err(RelayPortError::Publisher(
                "NATS token must not be empty when configured".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct JetStreamPublisher {
    context: jetstream::Context,
    stream_name: String,
}

impl JetStreamPublisher {
    pub async fn connect(settings: &JetStreamSettings) -> Result<Self, RelayPortError> {
        let expected = settings.stream_config()?;
        let options = match &settings.connection.token {
            Some(token) => ConnectOptions::new().token(token.clone()),
            None => ConnectOptions::new(),
        };
        let client = options
            .connect(settings.connection.url.clone())
            .await
            .map_err(|error| RelayPortError::Publisher(error.to_string()))?;
        let context = jetstream::new(client);
        let stream = if settings.allow_create {
            context
                .get_or_create_stream(expected.clone())
                .await
                .map_err(|error| RelayPortError::Publisher(error.to_string()))?
        } else {
            context
                .get_stream(&settings.stream_name)
                .await
                .map_err(|error| RelayPortError::Publisher(error.to_string()))?
        };
        validate_stream(&expected, &stream.cached_info().config)?;
        Ok(Self {
            context,
            stream_name: settings.stream_name.clone(),
        })
    }

    pub fn from_context(context: jetstream::Context, stream_name: impl Into<String>) -> Self {
        Self {
            context,
            stream_name: stream_name.into(),
        }
    }
}

fn validate_stream(expected: &StreamConfig, actual: &StreamConfig) -> Result<(), RelayPortError> {
    let matches = actual.name == expected.name
        && actual.subjects == expected.subjects
        && actual.max_bytes == expected.max_bytes
        && actual.max_messages == expected.max_messages
        && actual.max_age == expected.max_age
        && actual.duplicate_window == expected.duplicate_window
        && actual.num_replicas == expected.num_replicas
        && actual.storage == expected.storage
        && actual.retention == expected.retention
        && actual.discard == expected.discard
        && actual.deny_delete == expected.deny_delete
        && actual.deny_purge == expected.deny_purge
        && !actual.no_ack;
    if matches {
        Ok(())
    } else {
        Err(RelayPortError::Publisher(format!(
            "JetStream stream {} does not match the required bounded durability policy",
            expected.name
        )))
    }
}

#[async_trait]
impl AcknowledgedEventPublisher for JetStreamPublisher {
    async fn publish_and_ack(
        &self,
        event: &OmsEventEnvelope,
        payload: &[u8],
    ) -> Result<PublishAck, RelayPortError> {
        let mut headers = HeaderMap::new();
        let message_id = HeaderValue::from_str(event.nats_message_id())
            .map_err(|error| RelayPortError::Publisher(error.to_string()))?;
        headers.insert(NATS_MESSAGE_ID_HEADER, message_id);

        let pending_ack = self
            .context
            .publish_with_headers(event.nats_subject(), headers, payload.to_vec().into())
            .await
            .map_err(|error| RelayPortError::Publisher(error.to_string()))?;
        let ack = pending_ack
            .await
            .map_err(|error| RelayPortError::Publisher(error.to_string()))?;
        if ack.stream != self.stream_name {
            return Err(RelayPortError::Publisher(
                "JetStream acknowledged the event from an unexpected stream".into(),
            ));
        }
        Ok(PublishAck {
            stream_sequence: ack.sequence,
            duplicate: ack.duplicate,
        })
    }
}
