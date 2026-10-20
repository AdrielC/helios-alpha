#![cfg(feature = "native")]
mod common;
use common::*;
use helio_decision::*;
use proptest::prelude::*;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

struct TestClock(AtomicU64);
impl Clock for TestClock {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}
struct Transport {
    calls: Arc<AtomicUsize>,
    delay: Duration,
    error: Option<DecisionError>,
}
impl DecisionTransport for Transport {
    fn identity(&self) -> Value {
        json!({"transport":"test"})
    }
    fn send(&self, _: Value) -> DecisionFuture<'_, Result<Value, DecisionError>> {
        Box::pin(async {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if !self.delay.is_zero() {
                tokio::time::sleep(self.delay).await;
            }
            match self.error {
                Some(error) => Err(error),
                None => Ok(predicate_response(Protocol::OpenAi, 0.8)),
            }
        })
    }
}
fn service(
    journal: Arc<dyn DecisionJournal>,
    clock: Arc<dyn Clock>,
    transport: Arc<dyn DecisionTransport>,
) -> DecisionService {
    DecisionService::new(
        Arc::new(adapter(Protocol::OpenAi)),
        transport,
        journal,
        clock,
        1,
        10_000,
        Duration::from_secs(1),
    )
    .unwrap()
}
fn transport(calls: Arc<AtomicUsize>) -> Arc<Transport> {
    Arc::new(Transport {
        calls,
        delay: Duration::ZERO,
        error: None,
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn event_sequences_preserve_identity_across_restarts(events in prop::collection::vec((0u8..8, any::<bool>()), 1..40)) {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("decisions.sqlite3");
        let calls = Arc::new(AtomicUsize::new(0));
        let clock = Arc::new(TestClock(AtomicU64::new(100)));
        let mut accepted: BTreeMap<u8, (bool, DecisionReceipt)> = BTreeMap::new();
        for (id, revision) in events {
            let mut r = request();
            r.request_id = format!("event-{id}");
            r.input = DecisionInput::State { value: json!({"revision":revision}) };
            // Reopen on every step: the test model is independent of in-memory caches.
            let journal = Arc::new(SqliteJournal::open(&path, 100).unwrap());
            let worker = service(journal, clock.clone(), transport(calls.clone()));
            let result = runtime.block_on(worker.execute(r));
            match accepted.get(&id) {
                None => {
                    let delivery = result.unwrap();
                    prop_assert!(!delivery.replayed);
                    accepted.insert(id, (revision, delivery.receipt));
                }
                Some((original, receipt)) if *original == revision => {
                    let delivery = result.unwrap();
                    prop_assert!(delivery.replayed);
                    prop_assert_eq!(&delivery.receipt, receipt);
                }
                Some(_) => prop_assert_eq!(result.unwrap_err(), DecisionError::IdentityConflict),
            }
            prop_assert_eq!(calls.load(Ordering::SeqCst), accepted.len());
        }
    }

    #[test]
    fn corrupted_journal_cannot_replay_or_resend(
        mutation in 0u8..12,
        invalid_probability in 1.01f64..1e6,
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("decisions.sqlite3");
        let calls = Arc::new(AtomicUsize::new(0));
        let clock = Arc::new(TestClock(AtomicU64::new(100)));
        let first = {
            let worker = service(Arc::new(SqliteJournal::open(&path, 10).unwrap()),
                clock.clone(), transport(calls.clone()));
            runtime.block_on(worker.execute(request())).unwrap().receipt
        };
        let mut stored = serde_json::to_value(&first).unwrap();
        match mutation {
            0 => stored["schema_version"] = json!(SCHEMA_VERSION + 1),
            1 => stored["request_id"] = json!("different-request"),
            2 => stored["fingerprint"] = json!("different-fingerprint"),
            3 => stored["effective_at_ms"] = json!(request().effective_at_ms + 1),
            4 => stored["available_at_ms"] = json!(request().available_at_ms + 1),
            5 => stored["received_at_ms"] = json!(request().available_at_ms - 1),
            6 => stored["received_at_ms"] = json!(request().deadline_ms),
            7 => stored["outcome"]["Ok"]["answers"]["verified"]["probability"] = json!(invalid_probability),
            8 => stored["outcome"]["Ok"]["answers"] = json!({}),
            9 => stored["outcome"]["Ok"]["model"] = json!(" "),
            10 | 11 => {}
            _ => unreachable!(),
        }
        {
            let db = rusqlite::Connection::open(&path).unwrap();
            if mutation == 10 {
                let mut changed_request = request();
                changed_request.input = DecisionInput::State { value: json!("different evidence") };
                db.execute("UPDATE decisions SET request_json=?1",
                    [serde_json::to_string(&changed_request).unwrap()]).unwrap();
            } else {
                let encoded = if mutation == 11 { "{broken".to_owned() } else { stored.to_string() };
                db.execute("UPDATE decisions SET receipt_json=?1", [encoded]).unwrap();
            }
        }
        let restarted = service(Arc::new(SqliteJournal::open(&path, 10).unwrap()),
            clock, transport(calls.clone()));
        prop_assert_eq!(runtime.block_on(restarted.execute(request())).unwrap_err(), DecisionError::Journal);
        prop_assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}

struct UncheckedAdapter;
impl DecisionAdapter for UncheckedAdapter {
    fn identity(&self) -> Value {
        json!("unchecked-test-adapter")
    }
    fn encode(&self, request: &DecisionRequest) -> Result<Value, DecisionError> {
        adapter(Protocol::OpenAi).encode(request)
    }
    fn decode(&self, _: &DecisionRequest, _: Value) -> Result<DecisionResponse, DecisionError> {
        Ok(DecisionResponse {
            model: "test".into(),
            answers: BTreeMap::from([("verified".into(), Answer::Predicate { probability: -0.5 })]),
            usage: Value::Null,
        })
    }
}

#[tokio::test]
async fn custom_adapter_cannot_bypass_normalized_answer_validation() {
    let dir = tempfile::tempdir().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let worker = DecisionService::new(
        Arc::new(UncheckedAdapter),
        transport(calls.clone()),
        Arc::new(SqliteJournal::open(dir.path().join("j.db"), 10).unwrap()),
        Arc::new(TestClock(AtomicU64::new(100))),
        1,
        10_000,
        Duration::from_secs(1),
    )
    .unwrap();
    let first = worker.execute(request()).await.unwrap();
    assert_eq!(first.receipt.outcome, Err(DecisionError::InvalidResponse));
    let replay = worker.execute(request()).await.unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.receipt, first.receipt);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn replay_retains_original_arrival_after_deadline_and_new_model_conflicts() {
    let dir = tempfile::tempdir().unwrap();
    let journal = Arc::new(SqliteJournal::open(dir.path().join("j.db"), 10).unwrap());
    let calls = Arc::new(AtomicUsize::new(0));
    let clock = Arc::new(TestClock(AtomicU64::new(100)));
    let worker = service(journal.clone(), clock.clone(), transport(calls.clone()));
    let first = worker.execute(request()).await.unwrap();
    clock.0.store(50_000, Ordering::SeqCst);
    let replay = worker.execute(request()).await.unwrap();
    assert!(replay.replayed);
    assert_eq!(first.receipt, replay.receipt);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let mut changed = adapter(Protocol::OpenAi);
    changed.model = "different-model".into();
    let worker = DecisionService::new(
        Arc::new(changed),
        transport(calls.clone()),
        journal,
        clock,
        1,
        10_000,
        Duration::from_secs(1),
    )
    .unwrap();
    assert_eq!(
        worker.execute(request()).await.unwrap_err(),
        DecisionError::IdentityConflict
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

struct FailedFinish(Arc<SqliteJournal>);
impl DecisionJournal for FailedFinish {
    fn claim(&self, r: &DecisionRequest, fp: &str, now: u64) -> Result<Claim, DecisionError> {
        self.0.claim(r, fp, now)
    }
    fn finish(&self, _: &DecisionReceipt) -> Result<(), DecisionError> {
        Err(DecisionError::Journal)
    }
}
struct FailedClaim;
impl DecisionJournal for FailedClaim {
    fn claim(&self, _: &DecisionRequest, _: &str, _: u64) -> Result<Claim, DecisionError> {
        Err(DecisionError::Journal)
    }
    fn finish(&self, _: &DecisionReceipt) -> Result<(), DecisionError> {
        panic!("must not finish");
    }
}

#[tokio::test]
async fn failure_to_persist_never_emits_success_or_resends_unknown_outcome() {
    let dir = tempfile::tempdir().unwrap();
    let journal = Arc::new(SqliteJournal::open(dir.path().join("j.db"), 10).unwrap());
    let clock = Arc::new(TestClock(AtomicU64::new(100)));
    let calls = Arc::new(AtomicUsize::new(0));
    let worker = service(
        Arc::new(FailedClaim),
        clock.clone(),
        transport(calls.clone()),
    );
    assert_eq!(
        worker.execute(request()).await.unwrap_err(),
        DecisionError::Journal
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let worker = service(
        Arc::new(FailedFinish(journal.clone())),
        clock.clone(),
        transport(calls.clone()),
    );
    assert_eq!(
        worker.execute(request()).await.unwrap_err(),
        DecisionError::Journal
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let restarted = service(journal, clock, transport(calls.clone()));
    assert_eq!(
        restarted.execute(request()).await.unwrap_err(),
        DecisionError::Pending
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn virtual_time_expires_slow_io_and_persists_failure() {
    let dir = tempfile::tempdir().unwrap();
    let journal = Arc::new(SqliteJournal::open(dir.path().join("j.db"), 10).unwrap());
    let calls = Arc::new(AtomicUsize::new(0));
    let clock = Arc::new(TestClock(AtomicU64::new(100)));
    let slow = Arc::new(Transport {
        calls: calls.clone(),
        delay: Duration::from_secs(100),
        error: None,
    });
    let worker = service(journal, clock, slow);
    let result = worker.execute(request()).await.unwrap();
    assert_eq!(result.receipt.outcome, Err(DecisionError::Expired));
    assert!(result.receipt.latency_ms >= 1000);
    assert!(worker.execute(request()).await.unwrap().replayed);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn admission_and_capacity_reject_without_provider_calls() {
    let dir = tempfile::tempdir().unwrap();
    let journal = Arc::new(SqliteJournal::open(dir.path().join("j.db"), 1).unwrap());
    let calls = Arc::new(AtomicUsize::new(0));
    let clock = Arc::new(TestClock(AtomicU64::new(99)));
    let worker = service(journal, clock.clone(), transport(calls.clone()));
    assert_eq!(
        worker.execute(request()).await.unwrap_err(),
        DecisionError::FutureEvidence
    );
    clock.0.store(10_000, Ordering::SeqCst);
    assert_eq!(
        worker.execute(request()).await.unwrap_err(),
        DecisionError::Expired
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    clock.0.store(100, Ordering::SeqCst);
    worker.execute(request()).await.unwrap();
    let mut second = request();
    second.request_id = "second".into();
    assert_eq!(
        worker.execute(second).await.unwrap_err(),
        DecisionError::Busy
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

struct Gated {
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
    calls: AtomicUsize,
}
impl DecisionTransport for Gated {
    fn identity(&self) -> Value {
        json!("gated")
    }
    fn send(&self, _: Value) -> DecisionFuture<'_, Result<Value, DecisionError>> {
        Box::pin(async {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.entered.notify_one();
            self.release.notified().await;
            Ok(predicate_response(Protocol::OpenAi, 0.8))
        })
    }
}

#[tokio::test]
async fn independent_workers_cannot_send_the_same_reserved_identity() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("j.db");
    let clock = Arc::new(TestClock(AtomicU64::new(100)));
    let gated = Arc::new(Gated {
        entered: tokio::sync::Notify::new(),
        release: tokio::sync::Notify::new(),
        calls: AtomicUsize::new(0),
    });
    let a = Arc::new(service(
        Arc::new(SqliteJournal::open(&path, 10).unwrap()),
        clock.clone(),
        gated.clone(),
    ));
    let b = service(
        Arc::new(SqliteJournal::open(&path, 10).unwrap()),
        clock,
        gated.clone(),
    );
    let worker = a.clone();
    let pending = tokio::spawn(async move { worker.execute(request()).await });
    gated.entered.notified().await;
    assert_eq!(a.execute(request()).await.unwrap_err(), DecisionError::Busy);
    assert_eq!(
        b.execute(request()).await.unwrap_err(),
        DecisionError::Pending
    );
    assert_eq!(gated.calls.load(Ordering::SeqCst), 1);
    gated.release.notify_one();
    pending.await.unwrap().unwrap();
    assert!(b.execute(request()).await.unwrap().replayed);
}

#[tokio::test]
async fn authentication_and_rate_errors_are_durable_without_automatic_retries() {
    for error in [
        DecisionError::Authentication,
        DecisionError::RateLimited,
        DecisionError::Unavailable,
    ] {
        let dir = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let worker = service(
            Arc::new(SqliteJournal::open(dir.path().join("j.db"), 10).unwrap()),
            Arc::new(TestClock(AtomicU64::new(100))),
            Arc::new(Transport {
                calls: calls.clone(),
                delay: Duration::ZERO,
                error: Some(error),
            }),
        );
        assert_eq!(
            worker.execute(request()).await.unwrap().receipt.outcome,
            Err(error)
        );
        assert!(worker.execute(request()).await.unwrap().replayed);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
