use std::collections::{BTreeMap, VecDeque};

use helio_crypto::{CryptoAccountingSeed, CryptoBrokerJournal};
use helio_execution::BrokerDecimal;
use helio_robinhood::*;
use proptest::prelude::*;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const ACCOUNT: &str = "syntheticApiAccount7";
const NOW: u64 = 1_800_000_000_000;

struct Clock {
    start: u64,
    end: u64,
    reads: usize,
}
impl UnixClock for Clock {
    fn unix_seconds(&mut self) -> Result<u64, ClockError> {
        Ok(self.start / 1000)
    }
    fn unix_millis(&mut self) -> Result<u64, ClockError> {
        self.reads += 1;
        Ok(if self.reads == 1 {
            self.start
        } else {
            self.end
        })
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
            api_key: "fake".into(),
            signature: "fake".into(),
            timestamp: timestamp.to_string(),
        })
    }
}
#[derive(Default)]
struct Transport {
    replies: VecDeque<HttpResponse>,
    requests: Vec<HttpRequest>,
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
fn broker(pages: Vec<Value>) -> Broker {
    let transport = Transport {
        replies: pages
            .into_iter()
            .map(|p| HttpResponse {
                status: 200,
                body: serde_json::to_vec(&p).unwrap(),
            })
            .collect(),
        ..Default::default()
    };
    RobinhoodCryptoBroker::try_new(
        RobinhoodCryptoConfig::try_new(ACCOUNT).unwrap(),
        transport,
        Clock {
            start: NOW,
            end: NOW + 20,
            reads: 0,
        },
        Signer,
    )
    .unwrap()
}
fn binding() -> CryptoJournalBinding {
    CryptoJournalBinding::try_new(ACCOUNT, "a".repeat(64)).unwrap()
}
fn decimal(s: &str) -> BrokerDecimal {
    BrokerDecimal::try_new(s).unwrap()
}
fn account() -> Value {
    json!({"account_number":ACCOUNT,"status":"active","buying_power":"450.123456789",
        "buying_power_currency":"USD","is_api_tradable":true,"fee_tier_status":{"fee_ratio":0.0095}})
}
fn holding(code: &str, total: &str, available: &str) -> Value {
    json!({"account_number":ACCOUNT,"asset_code":code,"total_quantity":total,"quantity_available_for_trading":available})
}
fn page(rows: Vec<Value>) -> Value {
    json!({"results":rows,"next":null})
}
fn allocation() -> helio_crypto::CryptoAllocationBalance {
    let mut journal = CryptoBrokerJournal::new("a".repeat(64)).unwrap();
    journal
        .initialize_accounting(
            CryptoAccountingSeed {
                schema_version: 1,
                account_key: "a".repeat(64),
                cash_micros: 600_000_000,
                settlement_quantum_micros: 10_000,
                quantities: BTreeMap::from([("BTC-USD".into(), decimal("0.5"))]),
                source_digest: "b".repeat(64),
                observed_ms: NOW - 10,
            },
            NOW - 5,
        )
        .unwrap()
        .balance
}

#[test]
fn exact_account_availability_caps_cash_and_locked_assets_without_adopting_other_holdings() {
    let mut broker = broker(vec![
        page(vec![account()]),
        page(vec![holding("BTC", "2", "0.2"), holding("ETH", "10", "10")]),
    ]);
    let read = broker.read_crypto_account(&binding()).unwrap();
    assert_eq!(read.data().buying_power_usd, decimal("450.123456789"));
    assert_eq!(read.data().reported_fee_bps, Some(95));
    let caps = read.cap_allocation(&allocation(), NOW + 30, 5000).unwrap();
    assert_eq!(caps.new_buying_power_micros, 450_123_456);
    assert_eq!(
        caps.sellable_quantities,
        BTreeMap::from([("BTC-USD".into(), decimal("0.2"))])
    );
    assert!(!caps.execution_authority);
    assert_eq!(broker.transport().requests.len(), 2);
    assert_eq!(
        broker.transport().requests[1].path_and_query,
        format!("/api/v2/crypto/trading/holdings/?account_number={ACCOUNT}")
    );
    assert!(!format!("{read:?}").contains(ACCOUNT));
    assert!(!serde_json::to_string(&read).unwrap().contains(ACCOUNT));
    let value = serde_json::to_value(&read).unwrap();
    assert_eq!(
        read.snapshot_digest(),
        hex::encode(Sha256::digest(
            serde_json::to_vec(&value["snapshot"]).unwrap()
        ))
    );
}

#[test]
fn missing_fees_account_restrictions_and_existing_uncertainty_stop_new_buying_power() {
    for mode in 0..5 {
        let mut a = account();
        match mode {
            0 => {
                a.as_object_mut().unwrap().remove("fee_tier_status");
            }
            1 => a["status"] = json!("sell_only"),
            2 => a["status"] = json!("deactivated"),
            3 => a["is_api_tradable"] = json!(false),
            _ => {}
        };
        let mut b = broker(vec![page(vec![a]), page(vec![holding("BTC", "2", "1")])]);
        let read = b.read_crypto_account(&binding()).unwrap();
        let mut own = allocation();
        if mode == 4 {
            own.unresolved_orders = true;
        }
        let caps = read.cap_allocation(&own, NOW + 30, 5000).unwrap();
        assert_eq!(caps.new_buying_power_micros, 0);
        if mode == 0 {
            assert_eq!(caps.reported_fee_bps, None);
        }
        if mode == 1 {
            assert_eq!(caps.sellable_quantities["BTC-USD"], decimal("0.5"));
        }
        if mode >= 2 {
            assert!(caps.sellable_quantities.is_empty());
        }
    }
    let mut a = account();
    a["fee_tier_status"]["fee_ratio"] = json!(0);
    let read = broker(vec![page(vec![a]), page(vec![])])
        .read_crypto_account(&binding())
        .unwrap();
    assert_eq!(read.data().reported_fee_bps, Some(0));
}

#[test]
fn incomplete_cross_account_duplicate_and_inconsistent_results_never_become_empty_balances() {
    for mode in 0..10 {
        let mut accounts = page(vec![account()]);
        let mut holdings = page(vec![holding("BTC", "2", "1")]);
        match mode {
            0 => {
                accounts.as_object_mut().unwrap().remove("next");
            }
            1 => accounts["results"] = json!([]),
            2 => accounts["results"] = json!([account(), account()]),
            3 => accounts["results"][0]["buying_power_currency"] = json!("EUR"),
            4 => holdings["results"][0]["account_number"] = json!("anotherAccount"),
            5 => holdings["results"] = json!([holding("BTC", "2", "1"), holding("BTC", "2", "1")]),
            6 => holdings["results"][0]["quantity_available_for_trading"] = json!("3"),
            7 => {
                holdings.as_object_mut().unwrap().remove("results");
            }
            8 => accounts["results"][0]["fee_tier_status"]["fee_ratio"] = json!("1.00001"),
            _ => holdings["results"][0]["total_quantity"] = json!("0.0000000000000000001"),
        };
        assert!(
            broker(vec![accounts, holdings])
                .read_crypto_account(&binding())
                .is_err(),
            "case {mode}"
        );
    }
    let mut b = broker(vec![]);
    let wrong = CryptoJournalBinding::try_new("differentAccount", "a".repeat(64)).unwrap();
    assert!(b.read_crypto_account(&wrong).is_err());
    assert!(b.transport().requests.is_empty());
}

#[test]
fn pagination_keeps_account_scope_and_rejects_path_filters_and_cycles() {
    let mut accounts = page(vec![]);
    accounts["next"] = json!("/api/v2/crypto/trading/accounts/?cursor=next&limit=20");
    let mut holdings = page(vec![holding("BTC", "2", "1")]);
    holdings["next"] = json!("/api/v2/crypto/trading/holdings/?cursor=x%26account_number%3Devil");
    let mut b = broker(vec![
        accounts,
        page(vec![account()]),
        holdings,
        page(vec![holding("ETH", "3", "2")]),
    ]);
    assert_eq!(
        b.read_crypto_account(&binding())
            .unwrap()
            .data()
            .holdings
            .len(),
        2
    );
    assert_eq!(b.transport().requests[3].path_and_query,
        format!("/api/v2/crypto/trading/holdings/?account_number={ACCOUNT}&cursor=x%26account_number%3Devil"));
    for next in [
        "https://evil.example/api/v2/crypto/trading/accounts/?cursor=x",
        "/api/v2/crypto/trading/holdings/?cursor=x",
        "/api/v2/crypto/trading/accounts/?cursor=x&account_number=another",
        "/api/v2/crypto/trading/accounts/?cursor=x&asset_code=BTC",
        "/api/v2/crypto/trading/accounts/?cursor=x&cursor=y",
        "/api/v2/crypto/trading/accounts/?cursor=x#fragment",
    ] {
        let mut p = page(vec![account()]);
        p["next"] = json!(next);
        let mut b = broker(vec![p]);
        assert!(b.read_crypto_account(&binding()).is_err());
        assert_eq!(b.transport().requests.len(), 1);
    }
    let mut p = page(vec![]);
    p["next"] = json!("/api/v2/crypto/trading/accounts/?cursor=x");
    let mut b = broker(vec![p.clone(), p]);
    assert!(b.read_crypto_account(&binding()).is_err());
    assert_eq!(b.transport().requests.len(), 2);
}

#[test]
fn pagination_completion_is_required_and_is_hard_bounded() {
    let pages = (0..4)
        .map(|i| {
            json!({"results":if i==0 {vec![account()]} else {vec![]},
        "next":format!("/api/v2/crypto/trading/accounts/?cursor={i}")})
        })
        .collect();
    let mut b = broker(pages);
    assert!(b.read_crypto_account(&binding()).is_err());
    assert_eq!(b.transport().requests.len(), 4);
}

#[test]
fn stale_future_and_mismatched_allocations_cannot_be_used_for_new_preparation() {
    let read = broker(vec![page(vec![account()]), page(vec![])])
        .read_crypto_account(&binding())
        .unwrap();
    assert!(read
        .cap_allocation(&allocation(), NOW + 5000, 5000)
        .is_err());
    assert!(read.cap_allocation(&allocation(), NOW + 10, 5000).is_err());
    let mut wrong = allocation();
    wrong.account_key = "c".repeat(64);
    assert!(read.cap_allocation(&wrong, NOW + 30, 5000).is_err());
    for end in [NOW - 1, NOW + 5001] {
        let transport = Transport {
            replies: vec![page(vec![account()]), page(vec![])]
                .into_iter()
                .map(|p| HttpResponse {
                    status: 200,
                    body: serde_json::to_vec(&p).unwrap(),
                })
                .collect(),
            ..Default::default()
        };
        let mut b = RobinhoodCryptoBroker::try_new(
            RobinhoodCryptoConfig::try_new(ACCOUNT).unwrap(),
            transport,
            Clock {
                start: NOW,
                end,
                reads: 0,
            },
            Signer,
        )
        .unwrap();
        assert!(b.read_crypto_account(&binding()).is_err());
    }
}

proptest! {
    #[test]
    fn caps_never_create_cash_or_units_and_fractional_fees_round_up(
        own_cash in 0_u64..600_000_001, broker_cash in 0_u64..600_000_001,
        broker_units in 0_u64..500_000_001, fee_units in 0_u64..1_000_000_001,
    ) {
        let mut a=account();a["buying_power"]=json!(format!("{}.{:06}",broker_cash/1_000_000,broker_cash%1_000_000));
        a["fee_tier_status"]["fee_ratio"]=json!(format!("0.{fee_units:018}"));
        let available=format!("0.{broker_units:018}");
        let read=broker(vec![page(vec![a]),page(vec![holding("BTC","1",&available)])]).read_crypto_account(&binding()).unwrap();
        let mut own=allocation();own.cash_micros=own_cash;own.available_cash_micros=own_cash;
        let caps=read.cap_allocation(&own,NOW+30,5000).unwrap();
        prop_assert_eq!(caps.new_buying_power_micros,own_cash.min(broker_cash));
        if broker_units>0 { prop_assert_eq!(&caps.sellable_quantities["BTC-USD"],&decimal(&available)); }
        else { prop_assert!(caps.sellable_quantities.is_empty()); }
        prop_assert_eq!(caps.reported_fee_bps,Some(if fee_units==0 {0} else {1}));
    }
}
