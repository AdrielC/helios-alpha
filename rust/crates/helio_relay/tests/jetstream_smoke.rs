#![cfg(feature = "native-nats")]

use std::time::Duration;

use helio_oms::{OmsEvent, OmsEventEnvelope};
use helio_relay::nats::{JetStreamPublisher, JetStreamSettings, NatsConnectionSettings};
use helio_relay::AcknowledgedEventPublisher;

#[tokio::test]
#[ignore = "requires an isolated NATS JetStream server at HELIOS_NATS_TEST_URL"]
async fn real_jetstream_acknowledges_and_deduplicates_stable_event_identity() {
    let url = std::env::var("HELIOS_NATS_TEST_URL")
        .expect("HELIOS_NATS_TEST_URL must identify the isolated test server");
    let stream_name = format!("HELIOS_OMS_TEST_{}", std::process::id());
    let publisher = JetStreamPublisher::connect(&JetStreamSettings {
        connection: NatsConnectionSettings { url, token: None },
        stream_name,
        subjects: vec!["helios.oms.v1.>".into()],
        max_bytes: 16 * 1024 * 1024,
        max_messages: 10_000,
        max_age: Duration::from_secs(3_600),
        duplicate_window: Duration::from_secs(600),
        replicas: 1,
        allow_create: true,
    })
    .await
    .expect("JetStream stream must be provisioned with the exact durability policy");
    let event = OmsEventEnvelope {
        schema_version: 1,
        cursor: 1,
        event_id: "oms:v1:test-account:test-order:1".into(),
        account_id: "test-account".into(),
        client_order_id: "test-order".into(),
        aggregate_version: 1,
        committed_at_ns: 1,
        event: OmsEvent::Canceled { at_ns: 1 },
    };
    let payload = serde_json::to_vec(&event).unwrap();

    let first = publisher.publish_and_ack(&event, &payload).await.unwrap();
    let replay = publisher.publish_and_ack(&event, &payload).await.unwrap();

    assert!(!first.duplicate);
    assert!(replay.duplicate);
    assert_eq!(replay.stream_sequence, first.stream_sequence);
}
