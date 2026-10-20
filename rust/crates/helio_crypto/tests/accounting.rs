use std::collections::BTreeMap;

use helio_crypto::{
    CryptoAccountingSeed, CryptoBrokerJournal, CryptoJournalError as Error, CryptoOrderFill,
    CryptoOrderIntent, CryptoOrderObservation, CryptoOrderSide,
};
use helio_execution::{BrokerDecimal, BrokerOrderState};
use proptest::prelude::*;

fn d(value: &str) -> BrokerDecimal {
    BrokerDecimal::try_new(value).unwrap()
}

fn seed(cash: u64) -> CryptoAccountingSeed {
    CryptoAccountingSeed {
        schema_version: 1,
        account_key: "1".repeat(64),
        cash_micros: cash,
        settlement_quantum_micros: 1,
        quantities: BTreeMap::new(),
        source_digest: "2".repeat(64),
        observed_ms: 100,
    }
}

fn seeded(cash: u64) -> CryptoBrokerJournal {
    let mut journal = CryptoBrokerJournal::new(seed(cash).account_key).unwrap();
    journal.initialize_accounting(seed(cash), 500).unwrap();
    journal
}

fn intent(number: u8, side: CryptoOrderSide, at: u64) -> CryptoOrderIntent {
    CryptoOrderIntent {
        account_key: seed(0).account_key,
        client_order_id: format!("11111111-1111-4111-8111-{number:012}"),
        symbol: "BTC-USD".into(),
        side,
        quantity: d("0.0001"),
        limit_price: d("100000"),
        reserved_cash_micros: if side == CryptoOrderSide::Buy {
            11_000_000
        } else {
            0
        },
        policy_revision: 1,
        strategy_fingerprint: "3".repeat(64),
        input_digest: "4".repeat(64),
        risk_reservation_id: format!("{number:064x}"),
        created_ms: at,
        expires_ms: at + 10_000,
    }
}

fn observed(planned: &CryptoOrderIntent, price: &str, fee: &str) -> CryptoOrderObservation {
    let at = planned.created_ms;
    CryptoOrderObservation {
        account_key: planned.account_key.clone(),
        client_order_id: planned.client_order_id.clone(),
        broker_order_id: format!("22222222-2222-4222-8222-{:012}", at),
        symbol: planned.symbol.clone(),
        side: planned.side,
        ordered_quantity: planned.quantity.clone(),
        limit_price: planned.limit_price.clone(),
        state: BrokerOrderState::Filled,
        filled_quantity: planned.quantity.clone(),
        fee_usd: d(fee),
        executions: vec![CryptoOrderFill {
            execution_id: "actual-fill".into(),
            quantity: planned.quantity.clone(),
            price: d(price),
            occurred_ms: at + 2,
        }],
        updated_ms: at + 3,
        received_ms: at + 4,
    }
}

fn dispatch(journal: &mut CryptoBrokerJournal, planned: &CryptoOrderIntent) {
    journal
        .prepare(planned.clone(), planned.created_ms)
        .unwrap();
    journal
        .mark_dispatch_started(&planned.client_order_id, planned.created_ms + 1)
        .unwrap();
}

fn reconcile(journal: &mut CryptoBrokerJournal, id: &str, at: u64) {
    let digest = journal
        .record(id)
        .unwrap()
        .observation_digest
        .clone()
        .unwrap();
    let receipt = journal.reconcile_terminal(id, &digest, at).unwrap();
    assert!(!receipt.execution_authority);
}

fn restored(journal: &CryptoBrokerJournal) -> CryptoBrokerJournal {
    CryptoBrokerJournal::restore_snapshot(&seed(0).account_key, &journal.save_snapshot().unwrap())
        .unwrap()
}

#[test]
fn legacy_snapshots_are_unchanged_and_seeding_is_explicit_immutable_and_empty_only() {
    let mut journal = CryptoBrokerJournal::new(seed(0).account_key).unwrap();
    let legacy = journal.save_snapshot().unwrap();
    assert!(!String::from_utf8(legacy.clone())
        .unwrap()
        .contains("accounting"));
    assert_eq!(restored(&journal).save_snapshot().unwrap(), legacy);
    assert_eq!(journal.allocation_balance().unwrap(), None);
    assert!(
        journal
            .initialize_accounting(seed(600_000_000), 500)
            .unwrap()
            .changed
    );
    let bytes = journal.save_snapshot().unwrap();
    assert!(
        !journal
            .initialize_accounting(seed(600_000_000), 600)
            .unwrap()
            .changed
    );
    assert_eq!(journal.save_snapshot().unwrap(), bytes);
    assert_eq!(
        journal.initialize_accounting(seed(601_000_000), 600),
        Err(Error::IdentityConflict)
    );
    let mut late = CryptoBrokerJournal::new(seed(0).account_key).unwrap();
    late.prepare(intent(1, CryptoOrderSide::Buy, 1000), 1000)
        .unwrap();
    assert_eq!(
        late.initialize_accounting(seed(600_000_000), 2000),
        Err(Error::InvalidAccounting)
    );
}

#[test]
fn buy_and_sell_book_fills_and_fees_once_without_adopting_external_assets() {
    let mut journal = seeded(600_000_000);
    assert_eq!(
        journal.prepare(intent(2, CryptoOrderSide::Sell, 1000), 1000),
        Err(Error::InsufficientAllocation)
    );
    let buy = intent(1, CryptoOrderSide::Buy, 1000);
    dispatch(&mut journal, &buy);
    assert_eq!(
        journal
            .allocation_balance()
            .unwrap()
            .unwrap()
            .available_cash_micros,
        589_000_000
    );
    journal
        .observe(observed(&buy, "99000", "0.095"), 1004)
        .unwrap();
    // Neither a terminal read nor a caller-supplied coverage digest can free unbooked cash.
    let record = journal.record(&buy.client_order_id).unwrap();
    let arbitrary = helio_crypto::CryptoAccountCoverage {
        account_key: seed(0).account_key,
        observation_digest: record.observation_digest.clone().unwrap(),
        account_snapshot_digest: "f".repeat(64),
        snapshot_observed_ms: 1005,
    };
    assert_eq!(
        journal.cover_account(&buy.client_order_id, arbitrary, 1005),
        Err(Error::InvalidCoverage)
    );
    reconcile(&mut journal, &buy.client_order_id, 1005);
    let balance = journal.allocation_balance().unwrap().unwrap();
    assert_eq!(balance.cash_micros, 590_005_000);
    assert_eq!(balance.available_cash_micros, balance.cash_micros);
    assert_eq!(balance.quantities["BTC-USD"], d("0.0001"));
    assert!(!balance.unresolved_orders);
    let bytes = journal.save_snapshot().unwrap();
    reconcile(&mut journal, &buy.client_order_id, 1006);
    assert_eq!(journal.save_snapshot().unwrap(), bytes);
    journal = restored(&journal);
    let sell = intent(2, CryptoOrderSide::Sell, 2000);
    dispatch(&mut journal, &sell);
    assert_eq!(
        journal
            .allocation_balance()
            .unwrap()
            .unwrap()
            .available_quantities["BTC-USD"],
        d("0")
    );
    journal
        .observe(observed(&sell, "101000", "0.01"), 2004)
        .unwrap();
    reconcile(&mut journal, &sell.client_order_id, 2005);
    let balance = restored(&journal).allocation_balance().unwrap().unwrap();
    assert_eq!(balance.cash_micros, 600_095_000);
    assert!(balance.quantities.is_empty());
    assert_eq!(balance.accounted_orders, 2);
}

#[test]
fn amendments_replace_prior_totals_and_stale_acknowledgements_are_atomic_failures() {
    let mut journal = seeded(600_000_000);
    let planned = intent(1, CryptoOrderSide::Buy, 1000);
    dispatch(&mut journal, &planned);
    let mut fill = observed(&planned, "99000", "0.095");
    journal.observe(fill.clone(), 1004).unwrap();
    reconcile(&mut journal, &planned.client_order_id, 1005);
    let stale = journal
        .record(&planned.client_order_id)
        .unwrap()
        .observation_digest
        .clone()
        .unwrap();
    fill.fee_usd = d("0.105");
    fill.updated_ms = 1006;
    fill.received_ms = 1007;
    journal.observe(fill, 1007).unwrap();
    assert!(journal
        .record(&planned.client_order_id)
        .unwrap()
        .reservation_held());
    assert_eq!(
        journal.prepare(intent(2, CryptoOrderSide::Buy, 1008), 1008),
        Err(Error::InvalidAccounting)
    );
    let before = journal.save_snapshot().unwrap();
    assert_eq!(
        journal.reconcile_terminal(&planned.client_order_id, &stale, 1008),
        Err(Error::InvalidCoverage)
    );
    assert_eq!(journal.save_snapshot().unwrap(), before);
    journal = restored(&journal);
    reconcile(&mut journal, &planned.client_order_id, 1008);
    let balance = journal.allocation_balance().unwrap().unwrap();
    assert_eq!(balance.cash_micros, 589_995_000);
    assert_eq!(balance.quantities["BTC-USD"], d("0.0001"));
    assert_eq!(balance.accounted_orders, 1);
}

#[test]
fn tiny_canceled_partial_fills_keep_exact_assets_and_round_costs_conservatively() {
    let mut journal = seeded(600_000_000);
    let planned = intent(1, CryptoOrderSide::Buy, 1000);
    dispatch(&mut journal, &planned);
    let mut fill = observed(&planned, "99999.123456789", "0.000000000000000123");
    fill.state = BrokerOrderState::Canceled;
    fill.filled_quantity = d("0.000000000000000007");
    fill.executions[0].quantity = fill.filled_quantity.clone();
    journal.observe(fill, 1004).unwrap();
    reconcile(&mut journal, &planned.client_order_id, 1005);
    let balance = restored(&journal).allocation_balance().unwrap().unwrap();
    assert_eq!(balance.cash_micros, 599_999_998);
    assert_eq!(balance.quantities["BTC-USD"], d("0.000000000000000007"));
}

#[test]
fn broker_currency_quantum_never_overstates_cash_and_overflow_keeps_the_reservation() {
    let mut journal = CryptoBrokerJournal::new(seed(0).account_key).unwrap();
    let mut opening = seed(600_000_000);
    opening.settlement_quantum_micros = 10_000;
    journal.initialize_accounting(opening, 500).unwrap();
    let mut planned = intent(1, CryptoOrderSide::Buy, 1000);
    planned.quantity = d("0.00010000001");
    dispatch(&mut journal, &planned);
    journal
        .observe(observed(&planned, "99000", "0.095"), 1004)
        .unwrap();
    reconcile(&mut journal, &planned.client_order_id, 1005);
    assert_eq!(
        journal.allocation_balance().unwrap().unwrap().cash_micros,
        589_990_000
    );

    let mut journal = CryptoBrokerJournal::new(seed(0).account_key).unwrap();
    let mut opening = seed(600_000_000);
    opening.settlement_quantum_micros = u64::MAX;
    journal.initialize_accounting(opening, 500).unwrap();
    dispatch(&mut journal, &planned);
    journal
        .observe(observed(&planned, "99000", "0.095"), 1004)
        .unwrap();
    let digest = journal
        .record(&planned.client_order_id)
        .unwrap()
        .observation_digest
        .clone()
        .unwrap();
    let before = journal.save_snapshot().unwrap();
    assert_eq!(
        journal.reconcile_terminal(&planned.client_order_id, &digest, 1005),
        Err(Error::Capacity)
    );
    assert_eq!(journal.save_snapshot().unwrap(), before);
    assert_eq!(journal.reservations().unwrap().cash_micros, 11_000_000);
}

#[test]
fn invalid_seed_and_nonterminal_observations_cannot_activate_or_release_accounting() {
    let empty = CryptoBrokerJournal::new(seed(0).account_key).unwrap();
    for mutation in 0..4 {
        let mut journal = empty.clone();
        let mut bad = seed(600_000_000);
        match mutation {
            0 => bad.account_key = "9".repeat(64),
            1 => bad.settlement_quantum_micros = 0,
            2 => bad.observed_ms = 501,
            _ => {
                bad.quantities.insert("DOGE-USD".into(), d("1"));
            }
        }
        assert!(journal.initialize_accounting(bad, 500).is_err());
        assert_eq!(
            journal.save_snapshot().unwrap(),
            empty.save_snapshot().unwrap()
        );
    }
    let mut journal = seeded(600_000_000);
    let planned = intent(1, CryptoOrderSide::Buy, 1000);
    dispatch(&mut journal, &planned);
    let mut open = observed(&planned, "99000", "0");
    open.state = BrokerOrderState::Open;
    open.filled_quantity = d("0");
    open.executions.clear();
    journal.observe(open, 1004).unwrap();
    let digest = journal
        .record(&planned.client_order_id)
        .unwrap()
        .observation_digest
        .clone()
        .unwrap();
    assert_eq!(
        journal.reconcile_terminal(&planned.client_order_id, &digest, 1005),
        Err(Error::InvalidCoverage)
    );
    assert_eq!(journal.reservations().unwrap().cash_micros, 11_000_000);
}

#[test]
fn an_actual_deficit_is_preserved_and_prevents_further_allocation() {
    let mut journal = seeded(11_000_000);
    let planned = intent(1, CryptoOrderSide::Buy, 1000);
    dispatch(&mut journal, &planned);
    assert_eq!(
        journal.prepare(intent(2, CryptoOrderSide::Buy, 1002), 1002),
        Err(Error::InsufficientAllocation)
    );
    journal
        .observe(observed(&planned, "99000", "100"), 1004)
        .unwrap();
    reconcile(&mut journal, &planned.client_order_id, 1005);
    let balance = journal.allocation_balance().unwrap().unwrap();
    assert_eq!(balance.cash_micros, 0);
    assert_eq!(balance.cash_deficit_micros, 98_900_000);
    assert_eq!(
        journal.prepare(intent(2, CryptoOrderSide::Buy, 2000), 2000),
        Err(Error::InsufficientAllocation)
    );
    assert_eq!(
        restored(&journal).allocation_balance().unwrap().unwrap(),
        balance
    );
    // Selling owned assets remains possible; accounting does not force an uncovered short.
    journal
        .prepare(intent(2, CryptoOrderSide::Sell, 2000), 2000)
        .unwrap();
    journal
        .mark_dispatch_started(
            &intent(2, CryptoOrderSide::Sell, 2000).client_order_id,
            2001,
        )
        .unwrap();
}

#[test]
fn a_late_fee_amendment_invalidates_funding_for_an_already_prepared_buy() {
    let mut journal = seeded(40_000_000);
    let first = intent(1, CryptoOrderSide::Buy, 1000);
    dispatch(&mut journal, &first);
    let mut fill = observed(&first, "99000", "0.095");
    journal.observe(fill.clone(), 1004).unwrap();
    reconcile(&mut journal, &first.client_order_id, 1005);
    let second = intent(2, CryptoOrderSide::Buy, 2000);
    journal.prepare(second.clone(), 2000).unwrap();
    fill.fee_usd = d("25");
    fill.updated_ms = 2001;
    fill.received_ms = 2002;
    journal.observe(fill, 2002).unwrap();
    reconcile(&mut journal, &first.client_order_id, 2003);
    let before = journal.save_snapshot().unwrap();
    assert_eq!(
        journal.mark_dispatch_started(&second.client_order_id, 2004),
        Err(Error::InsufficientAllocation),
    );
    assert_eq!(journal.save_snapshot().unwrap(), before);
    assert_eq!(
        journal
            .record(&second.client_order_id)
            .unwrap()
            .dispatch_started_ms,
        None
    );
    // The original deadline can still release this provably unsent reservation.
    journal
        .mark_dispatch_started(&second.client_order_id, second.expires_ms)
        .unwrap();
    assert!(!journal
        .record(&second.client_order_id)
        .unwrap()
        .reservation_held());
}
#[test]
fn restored_coverage_must_have_matching_valid_accounting() {
    let mut journal = seeded(600_000_000);
    let planned = intent(1, CryptoOrderSide::Buy, 1000);
    dispatch(&mut journal, &planned);
    journal
        .observe(observed(&planned, "99000", "0.095"), 1004)
        .unwrap();
    reconcile(&mut journal, &planned.client_order_id, 1005);
    let original: serde_json::Value =
        serde_json::from_slice(&journal.save_snapshot().unwrap()).unwrap();
    for mutation in 0..3 {
        let mut broken = original.clone();
        let entries = &mut broken["accounting"]["terminal_observations"];
        match mutation {
            0 => *entries = serde_json::json!({}),
            1 => entries[&planned.client_order_id]["fee_usd"] = serde_json::json!("0"),
            _ => entries[&planned.client_order_id]["state"] = serde_json::json!("open"),
        }
        assert!(CryptoBrokerJournal::restore_snapshot(
            &seed(0).account_key,
            &serde_json::to_vec(&broken).unwrap()
        )
        .is_err());
    }
}

proptest! {
    #[test]
    fn recovery_never_duplicates_assets_or_understates_tiny_costs(
        quantity_units in 1_u64..1_000_000_000,
        price in 1_u64..100_000,
        fee_units in 0_u64..1_000_000_000,
        repeats in 1_usize..20,
    ) {
        let mut journal = seeded(600_000_000);
        let mut planned = intent(1, CryptoOrderSide::Buy, 1000);
        planned.quantity = d(&format!("0.{quantity_units:018}"));
        dispatch(&mut journal, &planned);
        let fill = observed(&planned, &price.to_string(), &format!("0.{fee_units:018}"));
        journal.observe(fill, 1004).unwrap();
        for _ in 0..repeats {
            reconcile(&mut journal, &planned.client_order_id, 1005);
            journal = restored(&journal);
        }
        let balance = journal.allocation_balance().unwrap().unwrap();
        let spent = (u128::from(quantity_units) * u128::from(price) * 1_000_000)
            .div_ceil(1_000_000_000_000_000_000)
            + u128::from(fee_units).div_ceil(1_000_000_000_000);
        prop_assert_eq!(u128::from(600_000_000 - balance.cash_micros), spent);
        prop_assert_eq!(&balance.quantities["BTC-USD"], &planned.quantity);
        prop_assert_eq!(balance.accounted_orders, 1);
    }
}
