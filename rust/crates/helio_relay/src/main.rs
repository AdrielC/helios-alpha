use std::env;
use std::time::Duration;

use helio_relay::golem::{
    GolemEndpoint, GolemOmsEventSource, GolemProjectionCursor, GolemRelaySettings,
};
use helio_relay::nats::{JetStreamPublisher, JetStreamSettings, NatsConnectionSettings};
use helio_relay::{OmsEventRelay, RelayError};
use thiserror::Error;
use tracing::{info, warn};

#[derive(Debug, Error)]
enum StartupError {
    #[error("missing required environment variable {0}")]
    MissingEnvironment(&'static str),
    #[error("invalid environment variable {name}: {detail}")]
    InvalidEnvironment { name: &'static str, detail: String },
    #[error(transparent)]
    Relay(#[from] RelayError),
    #[error("port connection failed: {0}")]
    Port(String),
}

fn required(name: &'static str) -> Result<String, StartupError> {
    env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or(StartupError::MissingEnvironment(name))
}

fn parse_usize(name: &'static str, default: usize) -> Result<usize, StartupError> {
    match env::var(name) {
        Ok(value) => value.parse().map_err(|error: std::num::ParseIntError| {
            StartupError::InvalidEnvironment {
                name,
                detail: error.to_string(),
            }
        }),
        Err(_) => Ok(default),
    }
}

fn parse_i64(name: &'static str, default: i64) -> Result<i64, StartupError> {
    match env::var(name) {
        Ok(value) => value.parse().map_err(|error: std::num::ParseIntError| {
            StartupError::InvalidEnvironment {
                name,
                detail: error.to_string(),
            }
        }),
        Err(_) => Ok(default),
    }
}

fn enabled(name: &'static str) -> Result<bool, StartupError> {
    match env::var(name) {
        Err(_) => Ok(false),
        Ok(value) if value == "1" => Ok(true),
        Ok(value) if value == "0" => Ok(false),
        Ok(_) => Err(StartupError::InvalidEnvironment {
            name,
            detail: "expected 0 or 1".into(),
        }),
    }
}

fn golem_settings() -> Result<GolemRelaySettings, StartupError> {
    let app_name = env::var("HELIOS_GOLEM_APP").unwrap_or_else(|_| "helios-alpha".into());
    let mode = env::var("HELIOS_GOLEM_MODE").unwrap_or_else(|_| "local".into());
    let environment_name = match env::var("HELIOS_GOLEM_ENVIRONMENT") {
        Ok(value) if !value.trim().is_empty() => value,
        _ if mode == "local" => "local".into(),
        _ => return Err(StartupError::MissingEnvironment("HELIOS_GOLEM_ENVIRONMENT")),
    };
    let endpoint = match mode.as_str() {
        "local" => GolemEndpoint::Local,
        "cloud" => GolemEndpoint::Cloud {
            token: required("GOLEM_TOKEN")?,
        },
        "custom" => GolemEndpoint::Custom {
            url: required("HELIOS_GOLEM_URL")?,
            token: required("GOLEM_TOKEN")?,
        },
        other => {
            return Err(StartupError::InvalidEnvironment {
                name: "HELIOS_GOLEM_MODE",
                detail: format!("unsupported mode {other}"),
            })
        }
    };
    Ok(GolemRelaySettings {
        endpoint,
        app_name,
        environment_name,
    })
}

#[tokio::main]
async fn main() -> Result<(), StartupError> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "helio_oms_relayd=info".into()),
        )
        .init();

    let account_id = required("HELIOS_ACCOUNT_ID")?;
    let projection_id =
        env::var("HELIOS_RELAY_PROJECTION_ID").unwrap_or_else(|_| "nats-oms-events-v1".into());
    let batch_size = parse_usize("HELIOS_RELAY_BATCH_SIZE", 256)?;
    let idle_poll_ms = parse_usize("HELIOS_RELAY_IDLE_POLL_MS", 100)?;
    let retry_ms = parse_usize("HELIOS_RELAY_RETRY_MS", 1_000)?;
    if idle_poll_ms == 0 || retry_ms == 0 {
        return Err(StartupError::InvalidEnvironment {
            name: "HELIOS_RELAY_IDLE_POLL_MS/HELIOS_RELAY_RETRY_MS",
            detail: "durations must be positive".into(),
        });
    }

    let golem = golem_settings()?;
    let source = GolemOmsEventSource::connect(&golem, &account_id)
        .await
        .map_err(|error| StartupError::Port(error.to_string()))?;
    let cursor = GolemProjectionCursor::connect(&golem, &account_id, &projection_id)
        .await
        .map_err(|error| StartupError::Port(error.to_string()))?;
    let connection = NatsConnectionSettings {
        url: env::var("NATS_URL").unwrap_or_else(|_| "nats://127.0.0.1:4222".into()),
        token: env::var("NATS_TOKEN").ok(),
    };
    let publisher = JetStreamPublisher::connect(&JetStreamSettings {
        connection,
        stream_name: env::var("HELIOS_NATS_STREAM").unwrap_or_else(|_| "HELIOS_OMS_V1".into()),
        subjects: vec!["helios.oms.v1.>".into()],
        max_bytes: parse_i64("HELIOS_NATS_MAX_BYTES", 10 * 1024 * 1024 * 1024)?,
        max_messages: parse_i64("HELIOS_NATS_MAX_MESSAGES", 10_000_000)?,
        max_age: Duration::from_secs(parse_usize("HELIOS_NATS_MAX_AGE_SECONDS", 604_800)? as u64),
        duplicate_window: Duration::from_secs(parse_usize(
            "HELIOS_NATS_DUPLICATE_WINDOW_SECONDS",
            600,
        )? as u64),
        replicas: parse_usize("HELIOS_NATS_REPLICAS", 3)?,
        allow_create: enabled("HELIOS_NATS_ALLOW_STREAM_CREATE")?,
    })
    .await
    .map_err(|error| StartupError::Port(error.to_string()))?;
    let relay = OmsEventRelay::try_new(
        account_id.clone(),
        projection_id.clone(),
        batch_size,
        source,
        publisher,
        cursor,
    )?;

    info!(%account_id, %projection_id, batch_size, "OMS relay admitted");
    loop {
        tokio::select! {
            signal = tokio::signal::ctrl_c() => {
                signal.map_err(|error| StartupError::Port(error.to_string()))?;
                info!("OMS relay shutdown requested");
                break;
            }
            receipt = relay.run_once() => {
                match receipt {
                    Ok(receipt) => {
                        if receipt.published > 0 {
                            info!(start_cursor = receipt.start_cursor, end_cursor = receipt.end_cursor, published = receipt.published, duplicate_acks = receipt.duplicate_acks, "OMS events durably relayed");
                        } else {
                            tokio::time::sleep(Duration::from_millis(idle_poll_ms as u64)).await;
                        }
                    }
                    Err(RelayError::Port(error)) => {
                        warn!(error = %error, "relay port unavailable; cursor remains at last acknowledged event");
                        tokio::time::sleep(Duration::from_millis(retry_ms as u64)).await;
                    }
                    Err(error) => return Err(error.into()),
                }
            }
        }
    }
    Ok(())
}
