use std::collections::{BTreeMap, VecDeque};

use chrono::DateTime;
use helio_crypto::{
    CryptoAccountingSeed, CryptoAllocationBalance, CryptoAllocationQuote,
    CryptoAllocationValuationInput, CryptoBrokerJournal, CryptoOrderIntent, CryptoOrderSide,
    CryptoPreparationContext, ExactCryptoQuote,
};
use helio_execution::BrokerDecimal;
use helio_robinhood::*;
use proptest::prelude::*;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const NOW: u64 = 1_800_000_000_000;
const API_ACCOUNT: &str = "syntheticMarketAccount";

struct Clock(VecDeque<u64>);
impl UnixClock for Clock {
    fn unix_seconds(&mut self) -> Result<u64, ClockError> {
        Ok(NOW / 1000)
    }
    fn unix_millis(&mut self) -> Result<u64, ClockError> {
        Ok(self.0.pop_front().unwrap_or(NOW))
    }
}
struct Signer;
impl RequestSigner for Signer {
    fn sign(
        &self,
        timestamp: u64,
        _: &str,
        _: HttpMethod,
        _: &[u8],
    ) -> Result<SignedHeaders, SigningError> {
        Ok(SignedHeaders {
            api_key: "synthetic".into(),
            signature: "synthetic".into(),
            timestamp: timestamp.to_string(),
        })
    }
}
#[derive(Default)]
struct Transport {
    requests: Vec<HttpRequest>,
    replies: VecDeque<HttpResponse>,
}
impl RobinhoodTransport for Transport {
    fn execute(&mut self, request: HttpRequest) -> Result<HttpResponse, TransportError> {
        assert_eq!(request.method, HttpMethod::Get);
        assert!(request.body.is_empty());
        self.requests.push(request);
        self.replies.pop_front().ok_or(TransportError::Unavailable)
    }
}
type Broker = RobinhoodCryptoBroker<Transport, Clock, Signer>;
fn broker(pages: Vec<Value>, times: Vec<u64>) -> Broker {
    RobinhoodCryptoBroker::try_new(
        RobinhoodCryptoConfig::try_new(API_ACCOUNT).unwrap(),
        Transport {
            replies: pages
                .into_iter()
                .map(|v| HttpResponse {
                    status: 200,
                    body: serde_json::to_vec(&v).unwrap(),
                })
                .collect(),
            ..Default::default()
        },
        Clock(times.into()),
        Signer,
    )
    .unwrap()
}
fn d(value: &str) -> BrokerDecimal {
    BrokerDecimal::try_new(value).unwrap()
}
fn binding() -> CryptoJournalBinding {
    CryptoJournalBinding::try_new(API_ACCOUNT, "a".repeat(64)).unwrap()
}
fn stamp(ms: u64) -> String {
    DateTime::from_timestamp_millis(ms as i64)
        .unwrap()
        .to_rfc3339()
}
fn pair() -> Value {
    json!({"symbol":"BTC-USD","asset_code":"BTC","quote_code":"USD",
        "asset_increment":"0.00000001","quote_increment":"0.01",
        "max_order_size":"10","min_order_amount":"1.0000001","status":"tradable","is_api_tradable":true})
}
fn page(rows: Vec<Value>) -> Value {
    json!({"results":rows,"next":null})
}
fn estimates(quantity: &str) -> Value {
    let rows = ["bid", "ask"].map(|side| {
        json!({
            "symbol":"BTC-USD","quantity":quantity,"side":side,"bid":99999,"ask":100000,
            "fee_ratio":0.0095,"timestamp":stamp(NOW),
        })
    });
    json!({"results": rows})
}
fn account() -> Value {
    page(vec![
        json!({"account_number":API_ACCOUNT,"status":"active","buying_power":"500",
        "buying_power_currency":"USD","is_api_tradable":true,"fee_tier_status":{"fee_ratio":0.0095}}),
    ])
}
fn holdings() -> Value {
    page(vec![
        json!({"account_number":API_ACCOUNT,"asset_code":"BTC",
        "total_quantity":"0.002","quantity_available_for_trading":"0.0005"}),
    ])
}
fn allocation() -> CryptoAllocationBalance {
    CryptoBrokerJournal::new("a".repeat(64))
        .unwrap()
        .initialize_accounting(
            CryptoAccountingSeed {
                schema_version: 1,
                account_key: "a".repeat(64),
                cash_micros: 500_000_000,
                settlement_quantum_micros: 10_000,
                quantities: BTreeMap::from([("BTC-USD".into(), d("0.001"))]),
                source_digest: "b".repeat(64),
                observed_ms: NOW - 100,
            },
            NOW - 10,
        )
        .unwrap()
        .balance
}
fn intent() -> CryptoOrderIntent {
    CryptoOrderIntent {
        account_key: "a".repeat(64),
        client_order_id: "11299b2b-61e3-43e7-b9f7-dee77210bb29".into(),
        symbol: "BTC-USD".into(),
        side: CryptoOrderSide::Buy,
        quantity: d("0.0001"),
        limit_price: d("100000"),
        reserved_cash_micros: 11_000_000,
        policy_revision: 1,
        strategy_fingerprint: "2".repeat(64),
        input_digest: "3".repeat(64),
        risk_reservation_id: "4".repeat(64),
        created_ms: NOW - 1000,
        expires_ms: NOW + 10_000,
    }
}
fn policy_hash(context: &mut CryptoPreparationContext) {
    context.policy.sha256 = hex::encode(Sha256::digest(
        serde_json::to_vec(&context.policy.limits).unwrap(),
    ));
}
fn context(allocation: &CryptoAllocationBalance) -> CryptoPreparationContext {
    let mut ctx: CryptoPreparationContext=serde_json::from_value(json!({
        "policy":{"schema_version":1,"revision":1,"sha256":"","updated_ns":(NOW-3000)*1_000_000,
            "limits":{"capital_micros":600_000_000,"order_micros":60_000_000,
                "daily_loss_micros":60_000_000,"drawdown_micros":180_000_000,
                "symbol_exposure_micros":360_000_000,"max_daily_orders":6,"max_spread_bps":20,
                "max_quote_age_ms":1000,"symbols":["BTC-USD","ETH-USD"],"leverage":false}},
        "account":{"account_key":"a".repeat(64),"symbol":"BTC-USD",
            "snapshot_digest":allocation.ledger_digest,"observed_ms":NOW,"day_utc":NOW/86_400_000,
            "buying_power_micros":600_000_000,"equity_micros":600_000_000,
            "day_open_equity_micros":600_000_000,"peak_equity_micros":600_000_000,
            "gross_exposure_micros":100_000_000,"symbol_exposure_micros":100_000_000,
            "available_quantity":"0.001","daily_order_count":0,"unresolved_orders":false,
            "paused":false,"loss_stop_latched":false,"fee_reserve_bps":100,
            "settlement_quantum_micros":10_000},
        "quote":{"account_key":"a".repeat(64),"symbol":"BTC-USD","bid":"1","ask":"1","observed_ms":NOW},
        "rules":{"symbol":"BTC-USD","observed_ms":NOW,"asset_increment":"1","quote_increment":"1",
            "max_order_quantity":"10","minimum_order_micros":0,"status":"tradable","api_tradable":true},
        "signal":{"input_digest":"3".repeat(64),"available_ms":NOW-1000,"expires_ms":NOW+10_000},
    })).unwrap();
    policy_hash(&mut ctx);
    ctx
}
fn reads<'a>(
    a: &'a CryptoAccountAvailability,
    m: &'a CryptoMarketAvailability,
    l: &'a CryptoAllocationBalance,
) -> CryptoPreparationReads<'a> {
    CryptoPreparationReads {
        allocation: l,
        account: a,
        market: m,
    }
}
fn ready(
    quantity: &str,
) -> (
    Broker,
    CryptoAccountAvailability,
    CryptoMarketAvailability,
    CryptoAllocationBalance,
) {
    let mut b = broker(
        vec![
            account(),
            holdings(),
            page(vec![pair()]),
            estimates(quantity),
        ],
        vec![],
    );
    let a = b.read_crypto_account(&binding()).unwrap();
    let m = b
        .read_crypto_market(&binding(), "BTC-USD", &d(quantity))
        .unwrap();
    (b, a, m, allocation())
}

#[test]
fn complete_quantity_specific_read_preserves_exact_rules_prices_and_timestamps() {
    let mut p = pair();
    p["asset_increment"] = json!("0.000000000000000001");
    let mut prices = estimates("0.000000000000000007");
    prices["results"][1]["fee_ratio"] = json!("0.009500000000000001");
    prices["results"][0]["bid"] = json!("99998.123456789123456789");
    let mut b = broker(vec![page(vec![p]), prices], vec![]);
    let r = b
        .read_crypto_market(&binding(), "BTC-USD", &d("0.000000000000000007"))
        .unwrap();
    assert_eq!(r.data().quantity, d("0.000000000000000007"));
    assert_eq!(r.data().rules.minimum_order_micros, 1_000_001);
    assert_eq!(r.data().quote.bid, d("99998.123456789123456789"));
    assert_eq!(r.data().reported_fee_bps, 96);
    assert_eq!(r.data().quote.observed_ms, NOW);
    assert!(!r.data().execution_authority);
    let paths: Vec<_> = b
        .transport()
        .requests
        .iter()
        .map(|r| r.path_and_query.as_str())
        .collect();
    assert_eq!(paths,vec![
        "/api/v2/crypto/trading/trading_pairs/?symbol=BTC-USD",
        "/api/v2/crypto/trading/estimated_price/?symbol=BTC-USD&side=both&quantity=0.000000000000000007",
    ]);
    let wire = serde_json::to_value(&r).unwrap();
    assert_eq!(
        r.snapshot_digest(),
        hex::encode(Sha256::digest(
            serde_json::to_vec(&wire["snapshot"]).unwrap()
        ))
    );
    assert!(!format!("{r:?}").contains(API_ACCOUNT));
}

#[test]
fn incompatible_or_incomplete_market_responses_never_become_usable_quotes() {
    for mode in 0..16 {
        let mut p = page(vec![pair()]);
        let mut e = estimates("0.0001");
        match mode {
            0 => {
                p.as_object_mut().unwrap().remove("next");
            }
            1 => p["results"] = json!([]),
            2 => p["results"][0]["symbol"] = json!("ETH-USD"),
            3 => p["results"][0]["asset_code"] = json!("ETH"),
            4 => p["results"][0]["quote_code"] = json!("EUR"),
            5 => p["results"][0]["asset_increment"] = json!("0"),
            6 => p["results"][0]["max_order_size"] = json!("0.00001"),
            7 => p["results"][0]["status"] = json!("unexpected"),
            8 => p["results"][0]["is_api_tradable"] = json!(false),
            9 => e["results"][0]["quantity"] = json!("0.0002"),
            10 => e["results"][0]["side"] = json!("ask"),
            11 => e["results"][0]["symbol"] = json!("ETH-USD"),
            12 => e["results"][0]["bid"] = json!("0"),
            13 => e["results"][0]["ask"] = json!("1"),
            14 => {
                e["results"][0].as_object_mut().unwrap().remove("fee_ratio");
            }
            15 => p["results"][0]["max_order_size"] = json!("0.0001"),
            _ => unreachable!(),
        }
        let mut b = broker(vec![p, e], vec![]);
        assert!(
            b.read_crypto_market(&binding(), "BTC-USD", &d("0.0001"))
                .is_err(),
            "mode {mode}"
        );
    }
}

#[test]
fn symbol_pagination_cannot_change_endpoint_scope_or_hide_incomplete_results() {
    for next in [
        "https://evil.example/api/v2/crypto/trading/trading_pairs/?cursor=x",
        "/api/v2/crypto/trading/orders/?cursor=x",
        "/api/v2/crypto/trading/trading_pairs/?symbol=ETH-USD&cursor=x",
        "/api/v2/crypto/trading/trading_pairs/?symbol=BTC-USD&symbol=BTC-USD&cursor=x",
        "/api/v2/crypto/trading/trading_pairs/?account_number=other&cursor=x",
    ] {
        let mut b = broker(vec![json!({"results":[pair()],"next":next})], vec![]);
        assert!(b
            .read_crypto_market(&binding(), "BTC-USD", &d("0.0001"))
            .is_err());
        assert_eq!(b.transport().requests.len(), 1);
    }
    let mut b = broker(
        vec![
            json!({"results":[],"next":"/api/v2/crypto/trading/trading_pairs/?cursor=a%26symbol%3DETH-USD"}),
            page(vec![pair()]),
            estimates("0.0001"),
        ],
        vec![],
    );
    b.read_crypto_market(&binding(), "BTC-USD", &d("0.0001"))
        .unwrap();
    assert_eq!(
        b.transport().requests[1].path_and_query,
        "/api/v2/crypto/trading/trading_pairs/?symbol=BTC-USD&cursor=a%26symbol%3DETH-USD"
    );
}

#[test]
fn both_estimate_timestamps_must_be_current_and_the_read_must_finish_on_time() {
    for (first, second, times) in [
        (NOW - 5000, NOW, vec![]),
        (NOW - 1, NOW + 1, vec![]),
        (NOW, NOW, vec![NOW, NOW + 5000]),
        (NOW, NOW, vec![NOW, NOW - 1]),
    ] {
        let mut prices = estimates("0.0001");
        prices["results"][0]["timestamp"] = json!(stamp(first));
        prices["results"][1]["timestamp"] = json!(stamp(second));
        let mut b = broker(vec![page(vec![pair()]), prices], times);
        assert!(b
            .read_crypto_market(&binding(), "BTC-USD", &d("0.0001"))
            .is_err());
    }
}

#[test]
fn invalid_requests_and_binding_never_reach_the_transport() {
    let mut b = broker(vec![], vec![]);
    assert!(b
        .read_crypto_market(&binding(), "BTC-USD&side=ask", &d("0.0001"))
        .is_err());
    assert!(b
        .read_crypto_market(&binding(), "BTC-USD", &d("0"))
        .is_err());
    let other = CryptoJournalBinding::try_new("differentAccount", "a".repeat(64)).unwrap();
    assert!(b
        .read_crypto_market(&other, "BTC-USD", &d("0.0001"))
        .is_err());
    assert!(b.transport().requests.is_empty());
}

#[test]
fn preparation_uses_broker_rules_and_caps_validity_without_changing_the_frozen_intent() {
    let (b, a, m, l) = ready("0.0001");
    let mut ctx = context(&l);
    ctx.policy.limits.max_quote_age_ms = 60_000;
    policy_hash(&mut ctx);
    let i = intent();
    let draft = b
        .prepare_crypto_limit_order_from_reads(&binding(), i.clone(), &ctx, reads(&a, &m, &l), NOW)
        .unwrap();
    assert_eq!(draft.preparation().expires_ms(), NOW + 5000);
    assert_eq!(draft.preparation().intent(), &i);
    assert_eq!(draft.preparation().cash_reserve_micros(), 11_000_000);
    let body: Value = serde_json::from_slice(draft.body()).unwrap();
    assert_eq!(body["limit_order_config"]["asset_quantity"], "0.0001");
    assert_eq!(body["limit_order_config"]["limit_price"], "100000");
    assert_eq!(b.transport().requests.len(), 4); // Preparation itself performs no HTTP or writes.
    assert!(b
        .prepare_crypto_limit_order_from_reads(&binding(), i, &ctx, reads(&a, &m, &l), NOW + 5000)
        .is_err());
}

#[test]
fn reservation_aware_preparation_is_bound_to_one_immutable_unsent_order() {
    let (b, a, m, _) = ready("0.0001");
    let mut journal = CryptoBrokerJournal::new("a".repeat(64)).unwrap();
    journal
        .initialize_accounting(
            CryptoAccountingSeed {
                schema_version: 1,
                account_key: "a".repeat(64),
                cash_micros: 100_000_000,
                quantities: BTreeMap::new(),
                settlement_quantum_micros: 10_000,
                source_digest: "b".repeat(64),
                observed_ms: NOW - 5000,
            },
            NOW - 5000,
        )
        .unwrap();
    let allocation = journal.allocation_balance().unwrap().unwrap();
    let ctx = context(&allocation);
    let mut mark = CryptoAllocationValuationInput {
        schema_version: 1,
        account_key: "a".repeat(64),
        expected_balance_digest: allocation.snapshot_digest().unwrap(),
        policy: ctx.policy.clone(),
        observed_ms: NOW,
        source_digest: "c".repeat(64),
        fee_reserve_bps: 100,
        paused: false,
        quotes: vec![],
    };
    journal.mark_allocation(mark.clone(), NOW).unwrap();
    journal.prepare(intent(), NOW).unwrap();
    let prepared = journal
        .prepared_allocation(&intent().client_order_id, NOW)
        .unwrap();
    mark.expected_balance_digest = prepared.balance_digest.clone();
    journal
        .mark_prepared_allocation(prepared.scope.clone(), mark, NOW)
        .unwrap();
    let valuation = journal
        .prepared_valued_account(&prepared.scope, NOW)
        .unwrap();
    b.prepare_crypto_limit_order_from_valuation(
        &binding(),
        intent(),
        &valuation,
        &ctx.signal,
        reads(&a, &m, &prepared.balance),
        NOW,
    )
    .unwrap();
    for case in 0..4 {
        let mut changed = intent();
        match case {
            0 => changed.client_order_id = "33333333-3333-4333-8333-333333333333".into(),
            1 => changed.limit_price = d("99999"),
            2 => changed.expires_ms += 1,
            3 => changed.risk_reservation_id = "9".repeat(64),
            _ => unreachable!(),
        }
        assert!(
            b.prepare_crypto_limit_order_from_valuation(
                &binding(),
                changed,
                &valuation,
                &ctx.signal,
                reads(&a, &m, &prepared.balance),
                NOW,
            )
            .is_err(),
            "case {case}"
        );
    }
    let reserved = journal.allocation_balance().unwrap().unwrap();
    assert_eq!(reserved.available_cash_micros, 89_000_000);
    assert!(reserved.unresolved_orders);
    assert!(b
        .prepare_crypto_limit_order_from_valuation(
            &binding(),
            intent(),
            &valuation,
            &ctx.signal,
            reads(&a, &m, &reserved),
            NOW,
        )
        .is_err());
    journal
        .mark_dispatch_started(&intent().client_order_id, NOW)
        .unwrap();
    assert!(journal
        .prepared_allocation(&intent().client_order_id, NOW)
        .is_err());
    assert_eq!(b.transport().requests.len(), 4);
}

#[test]
fn durable_valuation_limits_preparation_to_oldest_holding_quote_and_exact_ledger() {
    let (b, a, m, _) = ready("0.0001");
    let mut journal = CryptoBrokerJournal::new("a".repeat(64)).unwrap();
    journal
        .initialize_accounting(
            CryptoAccountingSeed {
                schema_version: 1,
                account_key: "a".repeat(64),
                cash_micros: 400_000_000,
                quantities: BTreeMap::from([("BTC-USD".into(), d("0.001"))]),
                settlement_quantum_micros: 10_000,
                source_digest: "b".repeat(64),
                observed_ms: NOW - 5000,
            },
            NOW - 5000,
        )
        .unwrap();
    let allocation = journal.allocation_balance().unwrap().unwrap();
    let mut ctx = context(&allocation);
    ctx.policy.limits.max_quote_age_ms = 10_000;
    policy_hash(&mut ctx);
    journal
        .mark_allocation(
            CryptoAllocationValuationInput {
                schema_version: 1,
                account_key: "a".repeat(64),
                expected_balance_digest: allocation.snapshot_digest().unwrap(),
                policy: ctx.policy.clone(),
                observed_ms: NOW,
                source_digest: "c".repeat(64),
                fee_reserve_bps: 100,
                paused: false,
                quotes: vec![CryptoAllocationQuote {
                    quote: ExactCryptoQuote {
                        account_key: "a".repeat(64),
                        symbol: "BTC-USD".into(),
                        bid: d("99999"),
                        ask: d("100000"),
                        observed_ms: NOW - 4000,
                    },
                    quantity: d("0.001"),
                    source_digest: "d".repeat(64),
                }],
            },
            NOW,
        )
        .unwrap();
    let valuation = journal.valued_account("BTC-USD", NOW).unwrap();
    let draft = b
        .prepare_crypto_limit_order_from_valuation(
            &binding(),
            intent(),
            &valuation,
            &ctx.signal,
            reads(&a, &m, &allocation),
            NOW,
        )
        .unwrap();
    assert_eq!(draft.preparation().expires_ms(), NOW + 1000);
    assert!(b
        .prepare_crypto_limit_order_from_valuation(
            &binding(),
            intent(),
            &valuation,
            &ctx.signal,
            reads(&a, &m, &allocation),
            NOW + 1000,
        )
        .is_err());
    let mut changed = allocation.clone();
    changed.available_cash_micros -= 1;
    assert!(b
        .prepare_crypto_limit_order_from_valuation(
            &binding(),
            intent(),
            &valuation,
            &ctx.signal,
            reads(&a, &m, &changed),
            NOW,
        )
        .is_err());
    assert_eq!(b.transport().requests.len(), 4);
}

#[test]
fn older_broker_timestamps_shorten_preparation_even_when_the_http_read_is_new() {
    let mut prices = estimates("0.0001");
    for row in prices["results"].as_array_mut().unwrap() {
        row["timestamp"] = json!(stamp(NOW - 4000));
    }
    let mut b = broker(
        vec![account(), holdings(), page(vec![pair()]), prices],
        vec![],
    );
    let a = b.read_crypto_account(&binding()).unwrap();
    let m = b
        .read_crypto_market(&binding(), "BTC-USD", &d("0.0001"))
        .unwrap();
    let l = allocation();
    let mut ctx = context(&l);
    ctx.policy.limits.max_quote_age_ms = 60_000;
    policy_hash(&mut ctx);
    let draft = b
        .prepare_crypto_limit_order_from_reads(&binding(), intent(), &ctx, reads(&a, &m, &l), NOW)
        .unwrap();
    assert_eq!(draft.preparation().expires_ms(), NOW + 1000);
    assert!(b
        .prepare_crypto_limit_order_from_reads(
            &binding(),
            intent(),
            &ctx,
            reads(&a, &m, &l),
            NOW + 1000,
        )
        .is_err());
}

#[test]
fn lower_broker_cash_and_locked_holdings_restrict_preparation() {
    let mut low_cash = account();
    low_cash["results"][0]["buying_power"] = json!("10");
    let mut b = broker(
        vec![
            low_cash,
            holdings(),
            page(vec![pair()]),
            estimates("0.0001"),
        ],
        vec![],
    );
    let a = b.read_crypto_account(&binding()).unwrap();
    let m = b
        .read_crypto_market(&binding(), "BTC-USD", &d("0.0001"))
        .unwrap();
    let l = allocation();
    assert!(b
        .prepare_crypto_limit_order_from_reads(
            &binding(),
            intent(),
            &context(&l),
            reads(&a, &m, &l),
            NOW
        )
        .is_err());
    let (b, a, m, l) = ready("0.0006");
    let mut i = intent();
    i.side = CryptoOrderSide::Sell;
    i.reserved_cash_micros = 0;
    i.quantity = d("0.0006");
    assert!(b
        .prepare_crypto_limit_order_from_reads(&binding(), i, &context(&l), reads(&a, &m, &l), NOW)
        .is_err());
}

#[test]
fn fee_policy_signal_and_durable_risk_rejections_survive_composition() {
    let (b, a, m, l) = ready("0.0001");
    for mode in 0..11 {
        let mut ctx = context(&l);
        match mode {
            0 => ctx.account.fee_reserve_bps = 94,
            1 => ctx.account.snapshot_digest = "f".repeat(64),
            2 => ctx.account.observed_ms = NOW - 11,
            3 => ctx.account.paused = true,
            4 => ctx.account.loss_stop_latched = true,
            5 => ctx.account.daily_order_count = 6,
            6 => ctx.account.gross_exposure_micros = 600_000_000,
            7 => ctx.account.equity_micros = 500_000_000,
            8 => ctx.account.unresolved_orders = true,
            9 => ctx.signal.expires_ms = NOW,
            10 => ctx.policy.revision = 2,
            _ => unreachable!(),
        }
        assert!(
            b.prepare_crypto_limit_order_from_reads(
                &binding(),
                intent(),
                &ctx,
                reads(&a, &m, &l),
                NOW
            )
            .is_err(),
            "mode {mode}"
        );
    }
}

#[test]
fn account_fingerprint_and_exact_quantity_cannot_be_rebound_after_a_read() {
    let (b, a, m, l) = ready("0.0001");
    let different = RobinhoodCryptoBroker::try_new(
        RobinhoodCryptoConfig::try_new("differentAccount").unwrap(),
        Transport::default(),
        Clock(vec![].into()),
        Signer,
    )
    .unwrap();
    let binding = CryptoJournalBinding::try_new("differentAccount", "a".repeat(64)).unwrap();
    assert!(different
        .prepare_crypto_limit_order_from_reads(
            &binding,
            intent(),
            &context(&l),
            reads(&a, &m, &l),
            NOW
        )
        .is_err());
    let mut i = intent();
    i.quantity = d("0.0002");
    assert!(b
        .prepare_crypto_limit_order_from_reads(
            &self::binding(),
            i,
            &context(&l),
            reads(&a, &m, &l),
            NOW
        )
        .is_err());
}

proptest! {
    #[test]
    fn receipt_deadlines_never_outlive_broker_evidence(max_age in 100u64..=60_000, elapsed in 0u64..5001) {
        let (b,a,m,l)=ready("0.0001");
        let mut ctx=context(&l);
        ctx.policy.limits.max_quote_age_ms=max_age;
        policy_hash(&mut ctx);
        let result=b.prepare_crypto_limit_order_from_reads(&binding(),intent(),&ctx,reads(&a,&m,&l),NOW+elapsed);
        if let Ok(draft)=result {
            prop_assert!(elapsed<5000 && elapsed<=max_age);
            prop_assert!(draft.preparation().expires_ms()<=NOW+5000);
            prop_assert!(draft.preparation().expires_ms()<=NOW+max_age);
            prop_assert_eq!(draft.preparation().intent().expires_ms,NOW+10_000);
        } else if elapsed<max_age.min(5000) {
            prop_assert!(false,"fresh valid inputs should prepare");
        }
    }
}
