//! A durable diagnostic JetStream consumer. Not an order executor.
use std::io::Write;
use std::time::Duration;

use async_nats::jetstream::{self, consumer};
use futures_util::StreamExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "nats://127.0.0.1:4222".into());
    let mut options = async_nats::ConnectOptions::new();
    if let Ok(token) = std::env::var("HELIOS_NATS_TOKEN") {
        options = options.token(token);
    }
    let connection = options.connect(url).await?;
    let js = jetstream::new(connection);
    let stream = js.get_stream("HELIOS_SIGNALS_V1").await?;
    let consumer = stream
        .get_or_create_consumer(
            "signal-diagnostics-v1",
            consumer::pull::Config {
                durable_name: Some("signal-diagnostics-v1".into()),
                ack_policy: consumer::AckPolicy::Explicit,
                ack_wait: Duration::from_secs(30),
                max_ack_pending: 1,
                filter_subject: "helios.signal.>".into(),
                ..Default::default()
            },
        )
        .await?;
    let mut messages = consumer.messages().await?;
    while let Some(message) = messages.next().await {
        let message = message?;
        // Malformed data blocks acknowledgement and does not get echoed to logs.
        let value: serde_json::Value = serde_json::from_slice(&message.payload)?;
        println!("{}\t{}", message.subject, serde_json::to_string(&value)?);
        std::io::stdout().flush()?;
        message.double_ack().await?;
    }
    Ok(())
}
