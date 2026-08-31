use std::sync::{Arc, Mutex};

use helio_oms::{OmsEvent, OmsEventEnvelope};

use super::*;

fn event(cursor: u64) -> OmsEventEnvelope {
    OmsEventEnvelope {
        schema_version: 1,
        cursor,
        event_id: format!("event-{cursor}"),
        account_id: "paper-1".into(),
        client_order_id: "order-1".into(),
        aggregate_version: cursor,
        committed_at_ns: cursor,
        event: OmsEvent::Canceled { at_ns: cursor },
    }
}

#[derive(Clone)]
struct FakeSource {
    batch: OmsEventBatch,
    requests: Arc<Mutex<Vec<(u64, usize)>>>,
}

#[async_trait]
impl DurableOmsEventSource for FakeSource {
    async fn events_after(
        &self,
        cursor: u64,
        limit: usize,
    ) -> Result<OmsEventBatch, RelayPortError> {
        self.requests.lock().unwrap().push((cursor, limit));
        Ok(self.batch.clone())
    }
}

#[derive(Clone, Default)]
struct FakePublisher {
    events: PublishedEvents,
    fail: Arc<Mutex<bool>>,
    duplicate: bool,
}

type PublishedEvents = Arc<Mutex<Vec<(String, Vec<u8>)>>>;

#[async_trait]
impl AcknowledgedEventPublisher for FakePublisher {
    async fn publish_and_ack(
        &self,
        event: &OmsEventEnvelope,
        payload: &[u8],
    ) -> Result<PublishAck, RelayPortError> {
        self.events
            .lock()
            .unwrap()
            .push((event.event_id.clone(), payload.to_vec()));
        if *self.fail.lock().unwrap() {
            return Err(RelayPortError::Publisher("injected".into()));
        }
        Ok(PublishAck {
            stream_sequence: event.cursor,
            duplicate: self.duplicate,
        })
    }
}

#[derive(Clone)]
struct FakeCursor {
    status: Arc<Mutex<ProjectionCursorStatus>>,
    advances: Arc<Mutex<Vec<(u64, u64, String)>>>,
    fail_advance: Arc<Mutex<bool>>,
}

impl FakeCursor {
    fn new() -> Self {
        Self {
            status: Arc::new(Mutex::new(ProjectionCursorStatus {
                account_id: "paper-1".into(),
                projection_id: "jetstream".into(),
                cursor: 0,
                last_event_id: None,
            })),
            advances: Arc::default(),
            fail_advance: Arc::new(Mutex::new(false)),
        }
    }
}

#[async_trait]
impl DurableProjectionCursor for FakeCursor {
    async fn status(&self) -> Result<ProjectionCursorStatus, RelayPortError> {
        Ok(self.status.lock().unwrap().clone())
    }

    async fn advance(
        &self,
        expected_cursor: u64,
        next_cursor: u64,
        event_id: &str,
    ) -> Result<CursorAdvanceReceipt, RelayPortError> {
        self.advances
            .lock()
            .unwrap()
            .push((expected_cursor, next_cursor, event_id.to_owned()));
        if *self.fail_advance.lock().unwrap() {
            return Err(RelayPortError::Cursor("injected".into()));
        }
        let mut status = self.status.lock().unwrap();
        status.cursor = next_cursor;
        status.last_event_id = Some(event_id.to_owned());
        Ok(CursorAdvanceReceipt {
            cursor: next_cursor,
            event_id: event_id.to_owned(),
            replayed: false,
        })
    }
}

fn relay(
    batch: OmsEventBatch,
    publisher: FakePublisher,
    cursor: FakeCursor,
) -> OmsEventRelay<FakeSource, FakePublisher, FakeCursor> {
    OmsEventRelay::try_new(
        "paper-1",
        "jetstream",
        16,
        FakeSource {
            batch,
            requests: Arc::default(),
        },
        publisher,
        cursor,
    )
    .unwrap()
}

#[tokio::test]
async fn acknowledges_then_advances_every_contiguous_event() {
    let publisher = FakePublisher::default();
    let cursor = FakeCursor::new();
    let relay = relay(
        OmsEventBatch {
            next_cursor: 2,
            events: vec![event(1), event(2)],
        },
        publisher.clone(),
        cursor.clone(),
    );

    let receipt = relay.run_once().await.unwrap();

    assert_eq!(receipt.start_cursor, 0);
    assert_eq!(receipt.end_cursor, 2);
    assert_eq!(receipt.published, 2);
    assert_eq!(cursor.status().await.unwrap().cursor, 2);
    assert_eq!(publisher.events.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn publish_failure_never_advances_cursor() {
    let publisher = FakePublisher::default();
    *publisher.fail.lock().unwrap() = true;
    let cursor = FakeCursor::new();
    let relay = relay(
        OmsEventBatch {
            next_cursor: 1,
            events: vec![event(1)],
        },
        publisher,
        cursor.clone(),
    );

    assert!(matches!(relay.run_once().await, Err(RelayError::Port(_))));
    assert_eq!(cursor.status().await.unwrap().cursor, 0);
    assert!(cursor.advances.lock().unwrap().is_empty());
}

#[tokio::test]
async fn crash_gap_republishes_same_message_identity() {
    let publisher = FakePublisher::default();
    let cursor = FakeCursor::new();
    *cursor.fail_advance.lock().unwrap() = true;
    let batch = OmsEventBatch {
        next_cursor: 1,
        events: vec![event(1)],
    };
    let first = relay(batch.clone(), publisher.clone(), cursor.clone());
    assert!(first.run_once().await.is_err());
    assert_eq!(cursor.status().await.unwrap().cursor, 0);

    *cursor.fail_advance.lock().unwrap() = false;
    let retry = relay(batch, publisher.clone(), cursor.clone());
    retry.run_once().await.unwrap();

    let identities: Vec<_> = publisher
        .events
        .lock()
        .unwrap()
        .iter()
        .map(|(identity, _)| identity.clone())
        .collect();
    assert_eq!(identities, vec!["event-1", "event-1"]);
    assert_eq!(cursor.status().await.unwrap().cursor, 1);
}

#[tokio::test]
async fn validates_entire_batch_before_any_publish() {
    let publisher = FakePublisher::default();
    let cursor = FakeCursor::new();
    let mut foreign = event(2);
    foreign.account_id = "other-account".into();
    let relay = relay(
        OmsEventBatch {
            next_cursor: 2,
            events: vec![event(1), foreign],
        },
        publisher.clone(),
        cursor,
    );

    assert!(matches!(
        relay.run_once().await,
        Err(RelayError::ForeignAccount { cursor: 2 })
    ));
    assert!(publisher.events.lock().unwrap().is_empty());
}

#[tokio::test]
async fn rejects_cursor_gaps_and_inconsistent_batch_cursor() {
    let publisher = FakePublisher::default();
    let cursor = FakeCursor::new();
    let gap = relay(
        OmsEventBatch {
            next_cursor: 2,
            events: vec![event(2)],
        },
        publisher.clone(),
        cursor.clone(),
    );
    assert!(matches!(
        gap.run_once().await,
        Err(RelayError::CursorGap { .. })
    ));

    let inconsistent = relay(
        OmsEventBatch {
            next_cursor: 9,
            events: vec![event(1)],
        },
        publisher,
        cursor,
    );
    assert!(matches!(
        inconsistent.run_once().await,
        Err(RelayError::BatchCursorMismatch { .. })
    ));
}
