use std::fmt;
use std::sync::OnceLock;

use async_trait::async_trait;
use golem_client::bridge::GolemServer;
use oms_account_agent_client as oms_wire;
use projection_cursor_agent_client as cursor_wire;
use sha2::{Digest, Sha256};

use crate::{
    CursorAdvanceReceipt, DurableOmsEventSource, DurableProjectionCursor, OmsEventBatch,
    ProjectionCursorStatus, RelayPortError, MAX_RELAY_BATCH_SIZE,
};

#[derive(Clone, PartialEq, Eq)]
pub enum GolemEndpoint {
    Local,
    Cloud { token: String },
    Custom { url: String, token: String },
}

impl fmt::Debug for GolemEndpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Local => formatter.write_str("Local"),
            Self::Cloud { .. } => formatter.write_str("Cloud { token: [redacted] }"),
            Self::Custom { url, .. } => formatter
                .debug_struct("Custom")
                .field("url", url)
                .field("token", &"[redacted]")
                .finish(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GolemRelaySettings {
    pub endpoint: GolemEndpoint,
    pub app_name: String,
    pub environment_name: String,
}

impl GolemRelaySettings {
    pub fn local(app_name: impl Into<String>, environment_name: impl Into<String>) -> Self {
        Self {
            endpoint: GolemEndpoint::Local,
            app_name: app_name.into(),
            environment_name: environment_name.into(),
        }
    }

    fn validate(&self) -> Result<(), RelayPortError> {
        if self.app_name.trim().is_empty() || self.environment_name.trim().is_empty() {
            return Err(RelayPortError::Source(
                "Golem app and environment names must not be empty".into(),
            ));
        }
        match &self.endpoint {
            GolemEndpoint::Local => Ok(()),
            GolemEndpoint::Cloud { token } if !token.trim().is_empty() => Ok(()),
            GolemEndpoint::Custom { url, token }
                if !url.trim().is_empty() && !token.trim().is_empty() =>
            {
                reqwest::Url::parse(url)
                    .map(|_| ())
                    .map_err(|_| RelayPortError::Source("invalid Golem custom URL".into()))
            }
            _ => Err(RelayPortError::Source(
                "Golem credentials must not be empty".into(),
            )),
        }
    }

    fn server(&self) -> Result<GolemServer, RelayPortError> {
        match &self.endpoint {
            GolemEndpoint::Local => Ok(GolemServer::Local),
            GolemEndpoint::Cloud { token } => Ok(GolemServer::Cloud {
                token: token.clone(),
            }),
            GolemEndpoint::Custom { url, token } => Ok(GolemServer::Custom {
                url: reqwest::Url::parse(url)
                    .map_err(|_| RelayPortError::Source("invalid Golem custom URL".into()))?,
                token: token.clone(),
            }),
        }
    }

    fn fingerprint(&self) -> String {
        let (kind, url, token) = match &self.endpoint {
            GolemEndpoint::Local => ("local", "", ""),
            GolemEndpoint::Cloud { token } => ("cloud", "", token.as_str()),
            GolemEndpoint::Custom { url, token } => ("custom", url.as_str(), token.as_str()),
        };
        let token_hash = Sha256::digest(token.as_bytes());
        format!(
            "{kind}\u{1f}{url}\u{1f}{}\u{1f}{}\u{1f}{token_hash:x}",
            self.app_name, self.environment_name
        )
    }
}

static CLIENT_CONFIGURATION: OnceLock<String> = OnceLock::new();

fn configure_clients(settings: &GolemRelaySettings) -> Result<(), RelayPortError> {
    settings.validate()?;
    let fingerprint = settings.fingerprint();
    if let Some(existing) = CLIENT_CONFIGURATION.get() {
        return if existing == &fingerprint {
            Ok(())
        } else {
            Err(RelayPortError::Source(
                "Golem clients were already configured for another endpoint or credential".into(),
            ))
        };
    }
    oms_wire::configure(
        settings.server()?,
        &settings.app_name,
        &settings.environment_name,
    );
    cursor_wire::configure(
        settings.server()?,
        &settings.app_name,
        &settings.environment_name,
    );
    CLIENT_CONFIGURATION
        .set(fingerprint)
        .map_err(|_| RelayPortError::Source("Golem client configuration raced".into()))
}

pub struct GolemOmsEventSource {
    agent: oms_wire::OmsAccountAgent,
}

impl GolemOmsEventSource {
    pub async fn connect(
        settings: &GolemRelaySettings,
        account_id: &str,
    ) -> Result<Self, RelayPortError> {
        if account_id.trim().is_empty() {
            return Err(RelayPortError::Source(
                "Golem OMS account identity must not be empty".into(),
            ));
        }
        configure_clients(settings)?;
        let agent = oms_wire::OmsAccountAgent::get(account_id.to_owned())
            .await
            .map_err(|error| RelayPortError::Source(error.to_string()))?;
        Ok(Self { agent })
    }
}

#[async_trait]
impl DurableOmsEventSource for GolemOmsEventSource {
    async fn events_after(
        &self,
        cursor: u64,
        limit: usize,
    ) -> Result<OmsEventBatch, RelayPortError> {
        if limit == 0 || limit > MAX_RELAY_BATCH_SIZE {
            return Err(RelayPortError::Source(
                "invalid OMS event batch limit".into(),
            ));
        }
        let limit = u32::try_from(limit)
            .map_err(|_| RelayPortError::Source("OMS event batch limit overflow".into()))?;
        let batch = self
            .agent
            .events_after(cursor, limit)
            .await
            .map_err(|error| RelayPortError::Source(error.to_string()))?
            .map_err(|error| {
                RelayPortError::Source(format!("Golem OMS rejected query: {error:?}"))
            })?;
        let events = batch
            .events_json
            .into_iter()
            .map(|json| {
                serde_json::from_str(&json).map_err(|error| {
                    RelayPortError::Source(format!("invalid OMS event JSON: {error}"))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(OmsEventBatch {
            next_cursor: batch.next_cursor,
            events,
        })
    }
}

pub struct GolemProjectionCursor {
    agent: cursor_wire::ProjectionCursorAgent,
}

impl GolemProjectionCursor {
    pub async fn connect(
        settings: &GolemRelaySettings,
        account_id: &str,
        projection_id: &str,
    ) -> Result<Self, RelayPortError> {
        configure_clients(settings)?;
        let agent = cursor_wire::ProjectionCursorAgent::get(
            account_id.to_owned(),
            projection_id.to_owned(),
        )
        .await
        .map_err(|error| RelayPortError::Cursor(error.to_string()))?;
        Ok(Self { agent })
    }
}

#[async_trait]
impl DurableProjectionCursor for GolemProjectionCursor {
    async fn status(&self) -> Result<ProjectionCursorStatus, RelayPortError> {
        let status = self
            .agent
            .status()
            .await
            .map_err(|error| RelayPortError::Cursor(error.to_string()))?
            .map_err(|error| {
                RelayPortError::Cursor(format!("Golem cursor rejected status: {error:?}"))
            })?;
        Ok(ProjectionCursorStatus {
            account_id: status.account_id,
            projection_id: status.projection_id,
            cursor: status.cursor,
            last_event_id: status.last_event_id,
        })
    }

    async fn advance(
        &self,
        expected_cursor: u64,
        next_cursor: u64,
        event_id: &str,
    ) -> Result<CursorAdvanceReceipt, RelayPortError> {
        let receipt = self
            .agent
            .advance(cursor_wire::AdvanceProjectionCursorInput {
                expected_cursor,
                next_cursor,
                event_id: event_id.to_owned(),
            })
            .await
            .map_err(|error| RelayPortError::Cursor(error.to_string()))?
            .map_err(|error| {
                RelayPortError::Cursor(format!("Golem cursor rejected advance: {error:?}"))
            })?;
        Ok(CursorAdvanceReceipt {
            cursor: receipt.cursor,
            event_id: receipt.event_id,
            replayed: receipt.replayed,
        })
    }
}
