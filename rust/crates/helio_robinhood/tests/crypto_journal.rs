use std::collections::VecDeque;

use chrono::{DateTime, SecondsFormat};
use helio_crypto::{
    CryptoAccountCoverage, CryptoBrokerJournal, CryptoOrderIntent, CryptoOrderPhase,
    CryptoOrderSide, CryptoPreparationContext,
};
use helio_execution::{BrokerDecimal, BrokerError};
use helio_robinhood::{
    ClockError, CryptoJournalBinding, HttpMethod, HttpRequest, HttpResponse, RequestSigner,
    RobinhoodCryptoBroker, RobinhoodCryptoConfig, RobinhoodTransport, SignedHeaders, SigningError,
    TimeInForce, TransportError, UnixClock,
};
use proptest::prelude::*;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const NOW: u64 = 1_800_000_000_123;
const API_ACCOUNT: &str = "syntheticApiAccount7";
const CLIENT: &str = "11299b2b-61e3-43e7-b9f7-dee77210bb29";
const BROKER_ID: &str = "497f6eca-6276-4993-bfeb-53cbbbba6f08";

struct FixedClock(u64);
impl UnixClock for FixedClock {
    fn unix_seconds(&mut self) -> Result<u64, ClockError> {
        Ok(self.0 / 1000)
    }
    fn unix_millis(&mut self) -> Result<u64, ClockError> {
        Ok(self.0)
    }
}

struct FakeSigner;
impl RequestSigner for FakeSigner {
    fn sign(
        &self,
        timestamp: u64,
        _: &str,
        _: HttpMethod,
        _: &[u8],
    ) -> Result<SignedHeaders, SigningError> {
        Ok(SignedHeaders {
            api_key: "synthetic-key".into(),
            signature: "synthetic-signature".into(),
            timestamp: timestamp.to_string(),
        })
    }
}

#[derive(Default)]
struct FakeTransport {
    requests: Vec<HttpRequest>,
    replies: VecDeque<Result<HttpResponse, TransportError>>,
    allow_cancel: bool,
}
impl FakeTransport {
    fn page(&mut self, results: Vec<Value>, next: Option<&str>) {
        self.raw(serde_json::to_vec(&json!({"results": results, "next": next})).unwrap());
    }
    fn raw(&mut self, body: Vec<u8>) {
        self.replies
            .push_back(Ok(HttpResponse { status: 200, body }));
    }
}
impl RobinhoodTransport for FakeTransport {
    fn execute(&mut self, request: HttpRequest) -> Result<HttpResponse, TransportError> {
        if request.method != HttpMethod::Get {
            assert!(self.allow_cancel, "read-only recovery must never write");
            assert_eq!(request.method, HttpMethod::Post);
            assert_eq!(
                request.path_and_query,
                format!("/api/v2/crypto/trading/orders/{BROKER_ID}/cancel/")
            );
        }
        assert!(request.body.is_empty());
        self.requests.push(request);
        self.replies.pop_front().expect("unexpected extra request")
    }
}
type Broker = RobinhoodCryptoBroker<FakeTransport, FixedClock, FakeSigner>;

fn broker(transport: FakeTransport, at: u64) -> Broker {
    RobinhoodCryptoBroker::try_new(
        RobinhoodCryptoConfig::try_new(API_ACCOUNT).unwrap(),
        transport,
        FixedClock(at),
        FakeSigner,
    )
    .unwrap()
}
fn decimal(s: &str) -> BrokerDecimal {
    BrokerDecimal::try_new(s).unwrap()
}
fn timestamp(ms: u64) -> String {
    DateTime::from_timestamp_millis(ms as i64)
        .unwrap()
        .to_rfc3339_opts(SecondsFormat::Millis, true)
}
fn intent() -> CryptoOrderIntent {
    CryptoOrderIntent {
        account_key: "a".repeat(64),
        client_order_id: CLIENT.into(),
        symbol: "BTC-USD".into(),
        side: CryptoOrderSide::Buy,
        quantity: decimal("0.000123456789123456"),
        limit_price: decimal("100000.123456789012345678"),
        reserved_cash_micros: 20_000_000,
        policy_revision: 1,
        strategy_fingerprint: "2".repeat(64),
        input_digest: "3".repeat(64),
        risk_reservation_id: "4".repeat(64),
        created_ms: NOW - 2000,
        expires_ms: NOW + 10_000,
    }
}
fn binding() -> CryptoJournalBinding {
    CryptoJournalBinding::try_new(API_ACCOUNT, intent().account_key).unwrap()
}
fn order() -> Value {
    json!({
        "id": BROKER_ID,
        "account_number": API_ACCOUNT,
        "client_order_id": CLIENT,
        "symbol": "BTC-USD", "side": "buy", "type": "limit",
        "state": "partially_filled",
        "limit_order_config": {
            "asset_quantity": intent().quantity.as_str(),
            "limit_price": intent().limit_price.as_str(),
            "time_in_force": "gtc",
        },
        "fee_charged": "0.000000000000000123",
        "filled_asset_quantity": "0.000000000000000007",
        "executions": [{
            "effective_price": "99999.123456789012345678",
            "quantity": "0.000000000000000007",
            "timestamp": timestamp(NOW - 10),
        }],
        "created_at": timestamp(NOW - 1000),
        "updated_at": timestamp(NOW - 5),
    })
}
fn read(
    value: Value,
    planned: &CryptoOrderIntent,
    at: u64,
) -> Result<helio_crypto::CryptoOrderObservation, BrokerError> {
    let mut transport = FakeTransport::default();
    transport.page(vec![value], None);
    broker(transport, at)
        .read_crypto_order(&binding(), planned)?
        .ok_or(BrokerError::Unavailable)
}
fn dispatched(planned: &CryptoOrderIntent) -> CryptoBrokerJournal {
    let mut journal = CryptoBrokerJournal::new(planned.account_key.clone()).unwrap();
    journal
        .prepare(planned.clone(), planned.created_ms)
        .unwrap();
    journal
        .mark_dispatch_started(CLIENT, planned.created_ms + 1)
        .unwrap();
    journal
}

#[test]
fn durable_cancel_recovers_exact_partial_fills_before_canceling_original_order() {
    let planned = intent();
    let mut journal = dispatched(&planned);
    journal.request_cancel(CLIENT, NOW - 20).unwrap();
    let record = journal.record(CLIENT).unwrap().clone();
    let mut transport = FakeTransport {
        allow_cancel: true,
        ..Default::default()
    };
    transport.page(vec![order()], None);
    let mut canceled = order();
    canceled["state"] = "canceled".into();
    canceled["updated_at"] = timestamp(NOW).into();
    transport.raw(serde_json::to_vec(&canceled).unwrap());
    let mut adapter = broker(transport, NOW);
    let recovered = adapter
        .recover_and_cancel_crypto_order(&binding(), &record)
        .unwrap()
        .unwrap();
    assert_eq!(recovered.filled_quantity.as_str(), "0.000000000000000007");
    assert_eq!(recovered.fee_usd.as_str(), "0.000000000000000123");
    journal.observe(recovered, NOW).unwrap();
    assert_eq!(
        journal.record(CLIENT).unwrap().phase,
        CryptoOrderPhase::Canceled
    );
    assert_eq!(
        journal.reservations().unwrap().cash_micros,
        planned.reserved_cash_micros
    );
    assert_eq!(adapter.transport().requests.len(), 2);
    assert_eq!(adapter.transport().requests[0].method, HttpMethod::Get);
    assert_eq!(adapter.transport().requests[1].method, HttpMethod::Post);
}

#[test]
fn missing_terminal_and_unrequested_cancellations_never_post() {
    for (cancel, present, terminal) in [
        (false, true, false),
        (true, false, false),
        (true, true, true),
    ] {
        let mut journal = dispatched(&intent());
        if cancel {
            journal.request_cancel(CLIENT, NOW - 20).unwrap();
        }
        let mut value = order();
        if terminal {
            value["state"] = "canceled".into();
        }
        let mut transport = FakeTransport::default();
        transport.page(if present { vec![value] } else { vec![] }, None);
        let mut adapter = broker(transport, NOW);
        let result = adapter
            .recover_and_cancel_crypto_order(&binding(), journal.record(CLIENT).unwrap())
            .unwrap();
        assert_eq!(result.is_some(), present);
        assert_eq!(adapter.transport().requests.len(), 1);
    }
}

#[test]
fn lost_cancel_reply_retains_reservation_and_a_late_fill_wins_the_race() {
    let mut journal = dispatched(&intent());
    journal
        .observe(read(order(), &intent(), NOW).unwrap(), NOW)
        .unwrap();
    journal.request_cancel(CLIENT, NOW + 1).unwrap();
    let saved = journal.save_snapshot().unwrap();
    let mut transport = FakeTransport {
        allow_cancel: true,
        ..Default::default()
    };
    transport.page(vec![order()], None);
    transport
        .replies
        .push_back(Err(TransportError::OutcomeUnknown));
    let mut adapter = broker(transport, NOW + 2);
    assert_eq!(
        adapter.recover_and_cancel_crypto_order(&binding(), journal.record(CLIENT).unwrap()),
        Err(BrokerError::AmbiguousOutcome)
    );
    assert_eq!(journal.save_snapshot().unwrap(), saved);
    journal = CryptoBrokerJournal::restore_snapshot(&intent().account_key, &saved).unwrap();
    let mut filled = order();
    filled["state"] = "filled".into();
    filled["filled_asset_quantity"] = intent().quantity.as_str().into();
    filled["fee_charged"] = "0.25".into();
    filled["updated_at"] = timestamp(NOW + 3).into();
    filled["executions"].as_array_mut().unwrap().push(json!({
        "effective_price":"99999.123456789012345678",
        "quantity":"0.000123456789123449",
        "timestamp":timestamp(NOW + 3),
    }));
    let mut transport = FakeTransport::default();
    transport.page(vec![filled], None);
    let mut adapter = broker(transport, NOW + 4);
    let recovered = adapter
        .recover_and_cancel_crypto_order(&binding(), journal.record(CLIENT).unwrap())
        .unwrap()
        .unwrap();
    journal.observe(recovered, NOW + 4).unwrap();
    assert_eq!(
        journal.record(CLIENT).unwrap().phase,
        CryptoOrderPhase::Filled
    );
    assert_eq!(
        journal.reservations().unwrap().cash_micros,
        intent().reserved_cash_micros
    );
    assert_eq!(adapter.transport().requests.len(), 1);
}

#[test]
fn unusable_cancel_responses_and_changed_identity_remain_ambiguous() {
    let mut journal = dispatched(&intent());
    journal.request_cancel(CLIENT, NOW - 20).unwrap();
    for variant in 0..7 {
        let mut canceled = order();
        canceled["state"] = "canceled".into();
        canceled["updated_at"] = timestamp(NOW).into();
        match variant {
            0 => canceled["id"] = "12121212-1212-4212-8212-121212121212".into(),
            1 => canceled["account_number"] = "anotherApiAccount".into(),
            2 => canceled["client_order_id"] = "12121212-1212-4212-8212-121212121212".into(),
            3 => canceled["fee_charged"] = "0".into(),
            4 => canceled["executions"] = json!([]),
            _ => {}
        }
        let mut transport = FakeTransport {
            allow_cancel: true,
            ..Default::default()
        };
        transport.page(vec![order()], None);
        transport.replies.push_back(Ok(HttpResponse {
            status: if variant == 5 { 429 } else { 200 },
            body: if variant == 6 {
                vec![]
            } else {
                serde_json::to_vec(&canceled).unwrap()
            },
        }));
        let mut adapter = broker(transport, NOW);
        assert_eq!(
            adapter.recover_and_cancel_crypto_order(&binding(), journal.record(CLIENT).unwrap()),
            Err(BrokerError::AmbiguousOutcome),
            "variant {variant}"
        );
    }
}

#[test]
fn invalid_recovery_record_or_regressed_get_cannot_cancel() {
    let mut journal = dispatched(&intent());
    journal
        .observe(read(order(), &intent(), NOW).unwrap(), NOW)
        .unwrap();
    journal.request_cancel(CLIENT, NOW + 1).unwrap();
    for variant in 0..4 {
        let mut record = journal.record(CLIENT).unwrap().clone();
        match variant {
            0 => record.intent.account_key = "b".repeat(64),
            1 => record.intent.limit_price = decimal("100001"),
            2 => record.cancel_requested_ms = Some(NOW + 10),
            _ => record.dispatch_started_ms = None,
        }
        let mut adapter = broker(FakeTransport::default(), NOW + 2);
        assert!(adapter
            .recover_and_cancel_crypto_order(&binding(), &record)
            .is_err());
        assert!(adapter.transport().requests.is_empty());
    }
    let mut regressed = order();
    regressed["id"] = "12121212-1212-4212-8212-121212121212".into();
    let mut transport = FakeTransport::default();
    transport.page(vec![regressed], None);
    let mut adapter = broker(transport, NOW + 2);
    assert!(adapter
        .recover_and_cancel_crypto_order(&binding(), journal.record(CLIENT).unwrap())
        .is_err());
    assert_eq!(adapter.transport().requests.len(), 1);
}

fn preparation_context() -> CryptoPreparationContext {
    let mut context: CryptoPreparationContext = serde_json::from_value(json!({
        "policy": {
            "schema_version": 1, "revision": 1, "sha256": "", "updated_ns": (NOW - 3000) * 1_000_000,
            "limits": {
                "capital_micros": 500_000_000, "order_micros": 20_000_000,
                "daily_loss_micros": 25_000_000, "drawdown_micros": 100_000_000,
                "symbol_exposure_micros": 300_000_000, "max_daily_orders": 10,
                "max_spread_bps": 20, "max_quote_age_ms": 1000,
                "symbols": ["BTC-USD", "ETH-USD"], "leverage": false,
            },
        },
        "account": {
            "account_key": "a".repeat(64), "symbol": "BTC-USD", "snapshot_digest": "9".repeat(64),
            "observed_ms": NOW - 10, "day_utc": NOW / 86_400_000,
            "buying_power_micros": 500_000_000, "equity_micros": 500_000_000,
            "day_open_equity_micros": 500_000_000, "peak_equity_micros": 500_000_000,
            "gross_exposure_micros": 0, "symbol_exposure_micros": 0,
            "available_quantity": "0.01", "daily_order_count": 0, "unresolved_orders": false,
            "paused": false, "loss_stop_latched": false, "fee_reserve_bps": 37,
            "settlement_quantum_micros": 10_000,
        },
        "quote": {
            "account_key": "a".repeat(64), "symbol": "BTC-USD",
            "bid": "99999", "ask": "100000", "observed_ms": NOW - 10,
        },
        "rules": {
            "symbol": "BTC-USD", "observed_ms": NOW - 10,
            "asset_increment": "0.000000000000000001", "quote_increment": "0.000000000000000001",
            "max_order_quantity": "2", "minimum_order_micros": 0,
            "status": "tradable", "api_tradable": true,
        },
        "signal": {
            "input_digest": "3".repeat(64), "available_ms": NOW - 3000, "expires_ms": NOW + 10_000,
        },
    })).unwrap();
    context.policy.sha256 = hex::encode(Sha256::digest(
        serde_json::to_vec(&context.policy.limits).unwrap(),
    ));
    context
}

#[test]
fn exact_draft_is_stable_unsigned_and_recovers_to_the_same_journal_intent() {
    let adapter = broker(FakeTransport::default(), NOW);
    let draft = adapter
        .prepare_crypto_limit_order(&binding(), intent(), &preparation_context(), NOW)
        .unwrap();
    let again = adapter
        .prepare_crypto_limit_order(&binding(), intent(), &preparation_context(), NOW + 1)
        .unwrap();
    assert_eq!(draft.body(), again.body());
    assert_eq!(draft.body_sha256(), again.body_sha256());
    assert!(adapter.transport().requests.is_empty());
    let body: Value = serde_json::from_slice(draft.body()).unwrap();
    assert_eq!(
        body["limit_order_config"]["asset_quantity"],
        intent().quantity.as_str()
    );
    assert_eq!(
        body["limit_order_config"]["limit_price"],
        intent().limit_price.as_str()
    );
    assert_eq!(body["limit_order_config"]["time_in_force"], "gtc");
    assert_eq!(
        serde_json::to_value(draft.preparation()).unwrap()["execution_authority"],
        false
    );
    assert!(!format!("{draft:?}").contains(intent().quantity.as_str()));
    let mut response = order();
    response["limit_order_config"] = body["limit_order_config"].clone();
    let recovered = read(response, draft.preparation().intent(), NOW).unwrap();
    let mut journal = dispatched(draft.preparation().intent());
    journal.observe(recovered, NOW).unwrap();
    assert_eq!(journal.record(CLIENT).unwrap().intent, intent());
}

#[test]
fn draft_requires_current_fees_policy_deadline_and_fixed_duration() {
    let adapter = broker(FakeTransport::default(), NOW);
    let mut changed = preparation_context();
    changed.account.fee_reserve_bps = 10_000;
    assert!(adapter
        .prepare_crypto_limit_order(&binding(), intent(), &changed, NOW)
        .is_err());
    changed = preparation_context();
    changed.policy.revision += 1;
    assert!(adapter
        .prepare_crypto_limit_order(&binding(), intent(), &changed, NOW)
        .is_err());
    changed = preparation_context();
    changed.signal.expires_ms = NOW;
    assert!(adapter
        .prepare_crypto_limit_order(&binding(), intent(), &changed, NOW)
        .is_err());
    assert!(adapter.transport().requests.is_empty());
    let mut config = RobinhoodCryptoConfig::try_new(API_ACCOUNT).unwrap();
    config.time_in_force = TimeInForce::GoodForDay;
    let adapter = RobinhoodCryptoBroker::try_new(
        config,
        FakeTransport::default(),
        FixedClock(NOW),
        FakeSigner,
    )
    .unwrap();
    assert!(adapter
        .prepare_crypto_limit_order(&binding(), intent(), &preparation_context(), NOW)
        .is_err());
    assert!(adapter.transport().requests.is_empty());
}

#[test]
fn changed_draft_cannot_replace_a_frozen_journal_intent() {
    let adapter = broker(FakeTransport::default(), NOW);
    let first = adapter
        .prepare_crypto_limit_order(&binding(), intent(), &preparation_context(), NOW)
        .unwrap();
    let mut journal = dispatched(first.preparation().intent());
    let before = journal.save_snapshot().unwrap();
    let mut changed = intent();
    changed.quantity = decimal("0.0001");
    let second = adapter
        .prepare_crypto_limit_order(&binding(), changed, &preparation_context(), NOW)
        .unwrap();
    assert_ne!(first.body_sha256(), second.body_sha256());
    assert!(journal
        .prepare(second.preparation().intent().clone(), NOW)
        .is_err());
    assert_eq!(journal.save_snapshot().unwrap(), before);
}

#[test]
fn exact_api_report_survives_journal_restore_and_duplicate_delivery() {
    let planned = intent();
    let report = read(order(), &planned, NOW).unwrap();
    assert_eq!(report.filled_quantity.as_str(), "0.000000000000000007");
    assert_eq!(report.fee_usd.as_str(), "0.000000000000000123");
    assert_eq!(report.ordered_quantity, planned.quantity);
    assert_eq!(report.limit_price, planned.limit_price);
    assert_eq!(report.received_ms, NOW);
    let mut journal = dispatched(&planned);
    let receipt = journal.observe(report.clone(), NOW).unwrap();
    assert_eq!(receipt.record.phase, CryptoOrderPhase::PartiallyFilled);
    assert!(!receipt.execution_authority);
    let bytes = journal.save_snapshot().unwrap();
    let mut restored = CryptoBrokerJournal::restore_snapshot(&planned.account_key, &bytes).unwrap();
    assert!(!restored.observe(report, NOW).unwrap().changed);
    assert_eq!(restored.reservations().unwrap().cash_micros, 20_000_000);
}

#[test]
fn explicit_account_binding_and_intent_are_checked_before_transport() {
    for invalid in [
        CryptoJournalBinding::try_new("anotherApiAccount", intent().account_key).unwrap(),
        CryptoJournalBinding::try_new(API_ACCOUNT, "b".repeat(64)).unwrap(),
    ] {
        let mut adapter = broker(FakeTransport::default(), NOW);
        assert!(adapter.read_crypto_order(&invalid, &intent()).is_err());
        assert!(adapter.transport().requests.is_empty());
    }
    let mut planned = intent();
    planned.client_order_id = "00000000-0000-0000-0000-000000000000".into();
    let mut adapter = broker(FakeTransport::default(), NOW);
    assert!(adapter.read_crypto_order(&binding(), &planned).is_err());
    assert!(adapter.transport().requests.is_empty());
    assert!(CryptoJournalBinding::try_new("bad&account", "a".repeat(64)).is_err());
    assert!(CryptoJournalBinding::try_new(API_ACCOUNT, "A".repeat(64)).is_err());
    assert!(!format!("{:?}", binding()).contains(API_ACCOUNT));
}

#[test]
fn missing_fee_and_order_configuration_never_become_zero_or_intent_defaults() {
    for path in [
        "/fee_charged",
        "/limit_order_config",
        "/limit_order_config/asset_quantity",
        "/limit_order_config/limit_price",
    ] {
        let mut missing = order();
        let (parent, name) = path.rsplit_once('/').unwrap();
        missing
            .pointer_mut(parent)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove(name);
        assert!(read(missing, &intent(), NOW).is_err(), "{path}");
        let mut null = order();
        *null.pointer_mut(path).unwrap() = Value::Null;
        assert!(read(null, &intent(), NOW).is_err(), "null {path}");
    }
}

#[test]
fn conflicting_identity_amounts_states_and_timestamps_are_rejected() {
    for (path, replacement) in [
        ("/account_number", json!("anotherAccount")),
        ("/id", json!("not-a-uuid")),
        ("/symbol", json!("ETH-USD")),
        ("/side", json!("sell")),
        ("/type", json!("market")),
        ("/limit_order_config/asset_quantity", json!("0.000123")),
        ("/limit_order_config/limit_price", json!("99999")),
        ("/limit_order_config/time_in_force", json!("gfd")),
        ("/filled_asset_quantity", json!("0.000000000000000008")),
        ("/executions/0/effective_price", json!("100001")),
        ("/state", json!("filled")),
        ("/fee_charged", json!("-0.1")),
        ("/fee_charged", json!("0.0000000000000000001")),
        ("/created_at", json!(timestamp(intent().created_ms - 1))),
        ("/updated_at", json!(timestamp(NOW + 1))),
        ("/executions/0/timestamp", json!(timestamp(NOW))),
    ] {
        let mut value = order();
        *value.pointer_mut(path).unwrap() = replacement;
        assert!(read(value, &intent(), NOW).is_err(), "{path}");
    }
}

#[test]
fn sell_reports_enforce_the_opposite_limit_direction() {
    let mut planned = intent();
    planned.side = CryptoOrderSide::Sell;
    planned.reserved_cash_micros = 0;
    let mut value = order();
    value["side"] = json!("sell");
    assert!(read(value.clone(), &planned, NOW).is_err());
    value["executions"][0]["effective_price"] = json!("100001");
    assert!(read(value, &planned, NOW).is_ok());
}

#[test]
fn ambiguous_and_missing_reads_keep_the_original_dispatched_reservation() {
    let planned = intent();
    let mut journal = dispatched(&planned);
    let original = journal.save_snapshot().unwrap();
    let mut transport = FakeTransport::default();
    transport
        .replies
        .push_back(Err(TransportError::OutcomeUnknown));
    transport.page(vec![], None);
    transport.page(vec![order()], None);
    let mut adapter = broker(transport, NOW + 120_000); // expired intent is still recoverable
    assert!(adapter.read_crypto_order(&binding(), &planned).is_err());
    assert!(adapter
        .read_crypto_order(&binding(), &planned)
        .unwrap()
        .is_none());
    assert_eq!(journal.save_snapshot().unwrap(), original);
    let recovered = adapter
        .read_crypto_order(&binding(), &planned)
        .unwrap()
        .unwrap();
    journal.observe(recovered, NOW + 120_000).unwrap();
    assert_eq!(adapter.transport().requests.len(), 3);
    assert_eq!(journal.reservations().unwrap().cash_micros, 20_000_000);
}

#[test]
fn unrelated_quote_amount_order_does_not_break_exact_quantity_lookup() {
    let mut unrelated = order();
    unrelated["client_order_id"] = json!("11299b2b-61e3-43e7-b9f7-dee77210bb28");
    unrelated["limit_order_config"] = json!({"quote_amount":"20","limit_price":"100000"});
    unrelated.as_object_mut().unwrap().remove("fee_charged");
    let mut transport = FakeTransport::default();
    transport.page(vec![unrelated, order()], None);
    assert!(broker(transport, NOW)
        .read_crypto_order(&binding(), &intent())
        .unwrap()
        .is_some());
}

#[test]
fn pagination_checks_scope_cycles_duplicates_and_incomplete_scans() {
    let next = "/api/v2/crypto/trading/orders/?cursor=second";
    let mut transport = FakeTransport::default();
    transport.page(vec![], Some(next));
    transport.page(vec![order()], None);
    let mut adapter = broker(transport, NOW);
    assert!(adapter
        .read_crypto_order(&binding(), &intent())
        .unwrap()
        .is_some());
    assert_eq!(
        adapter.transport().requests[1].path_and_query,
        format!("/api/v2/crypto/trading/orders/?account_number={API_ACCOUNT}&cursor=second")
    );

    for bad in [
        "https://elsewhere.invalid/api/v2/crypto/trading/orders/?cursor=x",
        "/api/v2/crypto/trading/orders/?account_number=other&cursor=x",
        "/api/v2/crypto/trading/orders/?cursor=x&state=open",
    ] {
        let mut transport = FakeTransport::default();
        transport.page(vec![], Some(bad));
        let mut adapter = broker(transport, NOW);
        assert!(adapter.read_crypto_order(&binding(), &intent()).is_err());
        assert_eq!(adapter.transport().requests.len(), 1);
    }
    let mut transport = FakeTransport::default();
    transport.page(vec![], Some(next));
    transport.page(vec![], Some(next));
    let mut adapter = broker(transport, NOW);
    assert!(adapter.read_crypto_order(&binding(), &intent()).is_err());
    assert_eq!(adapter.transport().requests.len(), 2);

    let mut transport = FakeTransport::default();
    transport.page(vec![order(), order()], None);
    assert!(broker(transport, NOW)
        .read_crypto_order(&binding(), &intent())
        .is_err());

    let mut transport = FakeTransport::default();
    transport.page(vec![], Some(next));
    let mut config = RobinhoodCryptoConfig::try_new(API_ACCOUNT).unwrap();
    config.max_reconciliation_pages = 1;
    let mut adapter =
        RobinhoodCryptoBroker::try_new(config, transport, FixedClock(NOW), FakeSigner).unwrap();
    assert!(adapter.read_crypto_order(&binding(), &intent()).is_err());
}

#[test]
fn malformed_or_oversized_pages_are_not_empty_account_results() {
    for body in [
        br#"{}"#.to_vec(),
        br#"{"results":[]}"#.to_vec(),
        br#"{"next":null}"#.to_vec(),
        b" ".repeat(1_048_577),
    ] {
        let mut transport = FakeTransport::default();
        transport.raw(body);
        assert!(broker(transport, NOW)
            .read_crypto_order(&binding(), &intent())
            .is_err());
    }
}

#[test]
fn late_fee_amendment_invalidates_coverage_without_inventing_a_second_fill() {
    let planned = intent();
    let mut value = order();
    value["state"] = json!("canceled");
    let mut journal = dispatched(&planned);
    let initial = read(value.clone(), &planned, NOW).unwrap();
    let receipt = journal.observe(initial.clone(), NOW).unwrap();
    journal
        .cover_account(
            CLIENT,
            CryptoAccountCoverage {
                account_key: planned.account_key.clone(),
                observation_digest: receipt.record.observation_digest.unwrap(),
                account_snapshot_digest: "9".repeat(64),
                snapshot_observed_ms: NOW,
            },
            NOW,
        )
        .unwrap();
    assert_eq!(journal.reservations().unwrap().cash_micros, 0);
    value["fee_charged"] = json!("0.000000000000000124");
    value["updated_at"] = json!(timestamp(NOW + 1));
    let amended = read(value, &planned, NOW + 1).unwrap();
    assert_eq!(amended.executions, initial.executions);
    let receipt = journal.observe(amended, NOW + 1).unwrap();
    assert!(receipt.record.account_coverage.is_none());
    assert_eq!(journal.reservations().unwrap().cash_micros, 20_000_000);
}

proptest! {
    #[test]
    fn numeric_json_preserves_sub_micro_quantities_prices_and_fees(
        fill_units in 1_u64..1_000_000,
        fee_units in 0_u64..1_000_000,
    ) {
        let quantity = format!("0.{fill_units:018}");
        let fee = format!("0.{fee_units:018}");
        let mut value = order();
        value["filled_asset_quantity"] = json!(quantity);
        value["executions"][0]["quantity"] = json!(quantity);
        value["fee_charged"] = json!(fee);
        let raw = serde_json::to_string(&json!({"results":[value],"next":null})).unwrap()
            .replace(&format!("\"{quantity}\""), &format!("{fill_units}e-18"))
            .replace(&format!("\"{fee}\""), &format!("{fee_units}e-18"))
            .replace("\"99999.123456789012345678\"", "99999.123456789012345678");
        let mut transport = FakeTransport::default();
        transport.raw(raw.into_bytes());
        let observed = broker(transport, NOW).read_crypto_order(&binding(), &intent()).unwrap().unwrap();
        prop_assert_eq!(&observed.filled_quantity, &decimal(&quantity));
        prop_assert_eq!(&observed.fee_usd, &decimal(&fee));
        prop_assert_eq!(observed.executions[0].price.as_str(), "99999.123456789012345678");
        let mut journal = dispatched(&intent());
        prop_assert!(journal.observe(observed, NOW).is_ok());
    }
}
