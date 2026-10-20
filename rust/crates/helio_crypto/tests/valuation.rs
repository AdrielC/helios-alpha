use std::collections::BTreeMap;

use helio_crypto::*;
use helio_execution::{BrokerDecimal, BrokerOrderState};
use proptest::prelude::*;
use sha2::{Digest, Sha256};

const DAY: u64 = 86_400_000;
const NOW: u64 = 42 * DAY + 3_600_000;

fn d(s: &str) -> BrokerDecimal {
    BrokerDecimal::try_new(s).unwrap()
}

fn hash<T: serde::Serialize>(value: &T) -> String {
    hex::encode(Sha256::digest(serde_json::to_vec(value).unwrap()))
}

fn policy() -> PolicyRevision {
    let limits = CryptoRiskLimits {
        capital_micros: 500_000_000,
        order_micros: 20_000_000,
        daily_loss_micros: 10_000_000,
        drawdown_micros: 15_000_000,
        symbol_exposure_micros: 250_000_000,
        max_daily_orders: 7,
        max_spread_bps: 20,
        max_quote_age_ms: 5_000,
        symbols: vec!["BTC-USD".into(), "ETH-USD".into()],
        leverage: false,
    };
    PolicyRevision {
        schema_version: 1,
        revision: 1,
        sha256: hash(&limits),
        limits,
        updated_ns: 0,
    }
}

fn seeded(cash: u64, quantity: Option<&str>, quantum: u64) -> CryptoBrokerJournal {
    let mut j = CryptoBrokerJournal::new("a".repeat(64)).unwrap();
    j.initialize_accounting(
        CryptoAccountingSeed {
            schema_version: 1,
            account_key: "a".repeat(64),
            cash_micros: cash,
            settlement_quantum_micros: quantum,
            quantities: quantity
                .map(|q| BTreeMap::from([("BTC-USD".into(), d(q))]))
                .unwrap_or_default(),
            source_digest: "b".repeat(64),
            observed_ms: NOW - 100,
        },
        NOW,
    )
    .unwrap();
    j
}

fn input(j: &CryptoBrokerJournal, at: u64, price: &str) -> CryptoAllocationValuationInput {
    let state = j.allocation_risk_state(at).unwrap();
    let b = state.balance.unwrap();
    CryptoAllocationValuationInput {
        schema_version: 1,
        account_key: "a".repeat(64),
        expected_balance_digest: state.balance_digest.unwrap(),
        policy: policy(),
        observed_ms: at,
        source_digest: "c".repeat(64),
        fee_reserve_bps: 100,
        paused: false,
        quotes: b
            .quantities
            .iter()
            .chain(&b.quantity_deficits)
            .map(|(symbol, quantity)| CryptoAllocationQuote {
                quote: ExactCryptoQuote {
                    account_key: "a".repeat(64),
                    symbol: symbol.clone(),
                    bid: d(price),
                    ask: d(price),
                    observed_ms: at,
                },
                quantity: quantity.clone(),
                source_digest: "d".repeat(64),
            })
            .collect(),
    }
}

fn mark(j: &mut CryptoBrokerJournal, at: u64, price: &str) -> CryptoAllocationRiskView {
    let i = input(j, at, price);
    j.mark_allocation(i, at).unwrap().risk
}

fn restart(j: &CryptoBrokerJournal) -> CryptoBrokerJournal {
    CryptoBrokerJournal::restore_snapshot(&"a".repeat(64), &j.save_snapshot().unwrap()).unwrap()
}

fn intent(at: u64) -> CryptoOrderIntent {
    CryptoOrderIntent {
        account_key: "a".repeat(64),
        client_order_id: "11111111-1111-4111-8111-111111111111".into(),
        symbol: "BTC-USD".into(),
        side: CryptoOrderSide::Buy,
        quantity: d("0.001"),
        limit_price: d("100"),
        reserved_cash_micros: 1_000_000,
        policy_revision: 1,
        strategy_fingerprint: "1".repeat(64),
        input_digest: "2".repeat(64),
        risk_reservation_id: "3".repeat(64),
        created_ms: at,
        expires_ms: at + 1_000,
    }
}

fn failed(j: &mut CryptoBrokerJournal, order: &CryptoOrderIntent) {
    let at = order.created_ms;
    j.mark_dispatch_started(&order.client_order_id, at + 1)
        .unwrap();
    let r = j
        .observe(
            CryptoOrderObservation {
                account_key: order.account_key.clone(),
                client_order_id: order.client_order_id.clone(),
                broker_order_id: "22222222-2222-4222-8222-222222222222".into(),
                symbol: order.symbol.clone(),
                side: order.side,
                ordered_quantity: order.quantity.clone(),
                limit_price: order.limit_price.clone(),
                state: BrokerOrderState::Failed,
                filled_quantity: d("0"),
                fee_usd: d("0"),
                executions: vec![],
                updated_ms: at + 2,
                received_ms: at + 2,
            },
            at + 2,
        )
        .unwrap();
    j.reconcile_terminal(
        &order.client_order_id,
        r.record.observation_digest.as_ref().unwrap(),
        at + 3,
    )
    .unwrap();
}

#[test]
fn valuation_uses_liquidation_fees_and_ask_exposure_without_refreshing_balance() {
    let mut j = seeded(100_000_000, Some("0.001"), 10_000);
    let before = j.allocation_balance().unwrap().unwrap();
    let mut i = input(&j, NOW + 100, "100000");
    i.quotes[0].quote.ask = d("101000");
    let receipt = j.mark_allocation(i.clone(), NOW + 100).unwrap();
    assert!(!receipt.execution_authority);
    assert!(receipt.changed && receipt.risk.current);
    assert_eq!(receipt.risk.equity_micros, 199_000_000);
    assert_eq!(receipt.risk.gross_exposure_micros, 101_000_000);
    assert_eq!(j.allocation_balance().unwrap().unwrap(), before);
    let saved = j.save_snapshot().unwrap();
    assert!(!j.mark_allocation(i.clone(), NOW + 200).unwrap().changed);
    assert_eq!(saved, j.save_snapshot().unwrap());
    assert!(j.mark_allocation(i, NOW + 5_100).is_err());
    assert!(
        !j.allocation_risk_state(NOW + 5_100)
            .unwrap()
            .risk
            .unwrap()
            .current
    );
    assert!(j.valued_account("BTC-USD", NOW + 5_100).is_err());
}

#[test]
fn marks_and_snapshots_preserve_latches_through_recovery_and_policy_relaxation() {
    let mut j = seeded(100_000_000, Some("0.001"), 1);
    let initial = mark(&mut j, NOW, "100000");
    let loss = mark(&mut j, NOW + 100, "80000");
    assert!(loss.day_loss_latched && loss.drawdown_latched);
    assert_eq!(initial.day_open_equity_micros, loss.day_open_equity_micros);
    j = restart(&j);
    let mut i = input(&j, NOW + 200, "110000");
    i.policy.revision = 2;
    i.policy.limits.daily_loss_micros *= 5;
    i.policy.limits.drawdown_micros *= 5;
    i.policy.sha256 = hash(&i.policy.limits);
    let recovered = j.mark_allocation(i, NOW + 200).unwrap().risk;
    assert!(recovered.day_loss_latched && recovered.drawdown_latched);
    assert_eq!(recovered.peak_equity_micros, 208_900_000);
    let account = restart(&j).valued_account("BTC-USD", NOW + 200).unwrap();
    assert!(account.account.loss_stop_latched);
    assert!(!account.execution_authority);
}

#[test]
fn overnight_gap_is_a_loss_and_daily_reset_does_not_clear_drawdown() {
    let mut j = seeded(100_000_000, Some("0.001"), 1);
    let close = 43 * DAY - 100;
    mark(&mut j, close, "100000");
    j = restart(&j);
    let open = mark(&mut j, 43 * DAY + 100, "80000");
    assert_eq!(open.day_utc, 43);
    assert_eq!(open.day_open_equity_micros, 199_000_000);
    assert!(open.day_loss_latched && open.drawdown_latched);
    let next = mark(&mut j, 44 * DAY + 100, "80000");
    assert!(!next.day_loss_latched && next.drawdown_latched);
    assert_eq!(next.day_open_equity_micros, open.equity_micros);
}

#[test]
fn repeated_source_across_midnight_advances_day_without_renewing_freshness() {
    let mut j = seeded(100_000_000, Some("0.001"), 1);
    let at = 43 * DAY - 100;
    let i = input(&j, at, "100000");
    let first = j.mark_allocation(i.clone(), at).unwrap();
    let next = j.mark_allocation(i, at + 200).unwrap();
    assert!(next.changed && next.risk.current);
    assert_eq!(next.risk.day_utc, 43);
    assert_eq!(next.risk.valid_until_ms, first.risk.valid_until_ms);
}

#[test]
fn failed_dispatch_counts_but_prepared_or_expired_unsent_orders_do_not() {
    let mut j = seeded(100_000_000, None, 1);
    mark(&mut j, NOW, "100");
    let order = intent(NOW + 100);
    j.prepare(order.clone(), order.created_ms).unwrap();
    assert!(
        !j.allocation_risk_state(NOW + 100)
            .unwrap()
            .risk
            .unwrap()
            .current
    );
    assert!(j
        .mark_allocation(input(&j, NOW + 100, "100"), NOW + 100)
        .is_err());
    failed(&mut j, &order);
    let r = mark(&mut j, NOW + 110, "100");
    assert_eq!(r.daily_order_count, 1);
    assert_eq!(
        restart(&j)
            .valued_account("BTC-USD", NOW + 111)
            .unwrap()
            .account
            .daily_order_count,
        1
    );
    let new_day = mark(&mut j, 43 * DAY, "100");
    assert_eq!(new_day.daily_order_count, 0);

    let mut unsent = seeded(100_000_000, None, 1);
    mark(&mut unsent, NOW, "100");
    unsent.prepare(order.clone(), order.created_ms).unwrap();
    unsent.plan_recovery(order.expires_ms).unwrap();
    let r = mark(&mut unsent, order.expires_ms, "100");
    assert_eq!(r.daily_order_count, 0);
}

#[test]
fn old_traded_journal_cannot_silently_start_new_loss_history() {
    let mut j = seeded(100_000_000, None, 1);
    let order = intent(NOW + 100);
    j.prepare(order.clone(), order.created_ms).unwrap();
    failed(&mut j, &order);
    assert!(j
        .mark_allocation(input(&j, NOW + 110, "100"), NOW + 110)
        .is_err());
    assert!(j.allocation_risk_state(NOW + 110).unwrap().risk.is_none());
}

#[test]
fn invalid_sources_policy_quantity_and_missing_quotes_are_atomic() {
    let j = seeded(100_000_000, Some("0.001"), 1);
    let original = j.save_snapshot().unwrap();
    for case in 0..15 {
        let mut j = j.clone();
        let mut i = input(&j, NOW, "100000");
        match case {
            0 => i.expected_balance_digest = "f".repeat(64),
            1 => i.account_key = "e".repeat(64),
            2 => i.quotes.clear(),
            3 => i.quotes.push(i.quotes[0].clone()),
            4 => i.quotes[0].quantity = d("0.0001"),
            5 => i.quotes[0].quote.account_key = "e".repeat(64),
            6 => i.quotes[0].quote.observed_ms = NOW + 1,
            7 => i.quotes[0].quote.observed_ms = NOW - 5_000,
            8 => i.quotes[0].quote.bid = d("0"),
            9 => i.quotes[0].quote.ask = d("99999"),
            10 => i.observed_ms = NOW + 1,
            11 => i.fee_reserve_bps = 10_001,
            12 => i.policy.sha256 = "0".repeat(64),
            13 => i.policy.updated_ns = (NOW + 1) * 1_000_000,
            14 => i.quotes[0].source_digest = "invalid".into(),
            _ => unreachable!(),
        }
        assert!(j.mark_allocation(i, NOW).is_err(), "case {case}");
        assert_eq!(j.save_snapshot().unwrap(), original, "case {case}");
    }
}

#[test]
fn old_prices_or_policy_cannot_replace_newer_history() {
    let mut j = seeded(100_000_000, Some("0.001"), 1);
    let mut i = input(&j, NOW + 100, "100000");
    i.policy.revision = 2;
    j.mark_allocation(i.clone(), NOW + 100).unwrap();
    let before = j.save_snapshot().unwrap();
    i.policy.revision = 1;
    assert!(j.mark_allocation(i, NOW + 200).is_err());
    let mut i = input(&j, NOW + 200, "100000");
    i.policy.revision = 2;
    i.quotes[0].quote.observed_ms = NOW + 99;
    assert!(j.mark_allocation(i, NOW + 200).is_err());
    assert_eq!(j.save_snapshot().unwrap(), before);
}

#[test]
fn historical_marks_restore_after_allocation_changes_but_cannot_authorize_a_snapshot() {
    let mut j = seeded(100_000_000, None, 1);
    mark(&mut j, NOW, "100");
    let order = intent(NOW + 100);
    j.prepare(order.clone(), order.created_ms).unwrap();
    j = restart(&j);
    assert!(j.valued_account("BTC-USD", NOW + 100).is_err());
    failed(&mut j, &order);
    j = restart(&j);
    assert!(
        !j.allocation_risk_state(NOW + 110)
            .unwrap()
            .risk
            .unwrap()
            .current
    );
    mark(&mut j, NOW + 110, "100");
    assert!(restart(&j).valued_account("BTC-USD", NOW + 111).is_ok());
}

#[test]
fn snapshots_validate_derived_valuation_and_preserve_legacy_serialization() {
    let mut j = seeded(100_000_000, Some("0.001"), 1);
    let legacy = j.save_snapshot().unwrap();
    assert!(!String::from_utf8(legacy.clone())
        .unwrap()
        .contains("risk_history"));
    assert_eq!(restart(&j).save_snapshot().unwrap(), legacy);
    mark(&mut j, NOW, "100000");
    mark(&mut j, NOW + 100, "80000");
    let value: serde_json::Value = serde_json::from_slice(&j.save_snapshot().unwrap()).unwrap();
    for case in 0..8 {
        let mut v = value.clone();
        let h = &mut v["risk_history"];
        match case {
            0 => h["valuation"]["equity_micros"] = 1.into(),
            1 => h["peak_equity_micros"] = 1.into(),
            2 => h["day_loss_latched"] = false.into(),
            3 => h["drawdown_latched"] = false.into(),
            4 => h["daily_order_count"] = 1.into(),
            5 => h["basis"]["available_cash_micros"] = 0.into(),
            6 => h["day_utc"] = 0.into(),
            7 => h["valuation"]["valid_until_ms"] = u64::MAX.into(),
            _ => unreachable!(),
        }
        assert!(
            CryptoBrokerJournal::restore_snapshot(
                &"a".repeat(64),
                &serde_json::to_vec(&v).unwrap()
            )
            .is_err(),
            "case {case}"
        );
    }
}

#[test]
fn cash_only_state_has_no_fabricated_positions_and_pause_is_retained() {
    let j = CryptoBrokerJournal::new("a".repeat(64)).unwrap();
    let r = j.allocation_risk_state(NOW).unwrap();
    assert!(r.balance.is_none() && r.risk.is_none() && r.balance_digest.is_none());
    let mut j = seeded(100_000_000, None, 1);
    let mut i = input(&j, NOW, "100000");
    i.paused = true;
    j.mark_allocation(i, NOW).unwrap();
    let snapshot = j.valued_account("ETH-USD", NOW).unwrap();
    assert_eq!(snapshot.account.available_quantity, d("0"));
    assert_eq!(snapshot.account.gross_exposure_micros, 0);
    assert!(snapshot.account.paused);
    assert!(j.valued_account("OTHER-USD", NOW).is_err());
}

#[test]
fn every_owned_asset_is_valued_even_when_removed_from_new_buy_policy() {
    let mut j = CryptoBrokerJournal::new("a".repeat(64)).unwrap();
    j.initialize_accounting(
        CryptoAccountingSeed {
            schema_version: 1,
            account_key: "a".repeat(64),
            cash_micros: 100_000_000,
            settlement_quantum_micros: 1,
            quantities: BTreeMap::from([
                ("BTC-USD".into(), d("0.001")),
                ("ETH-USD".into(), d("0.05")),
            ]),
            source_digest: "b".repeat(64),
            observed_ms: NOW,
        },
        NOW,
    )
    .unwrap();
    let mut i = input(&j, NOW, "100000");
    i.policy.limits.symbols = vec!["BTC-USD".into()];
    i.policy.sha256 = hash(&i.policy.limits);
    let eth = i
        .quotes
        .iter_mut()
        .find(|q| q.quote.symbol == "ETH-USD")
        .unwrap();
    eth.quote.bid = d("2000");
    eth.quote.ask = d("2000");
    let mut missing = i.clone();
    missing.quotes.pop();
    assert!(j.mark_allocation(missing, NOW).is_err());
    let r = j.mark_allocation(i, NOW).unwrap().risk;
    assert_eq!(r.equity_micros, 298_000_000);
    assert_eq!(r.gross_exposure_micros, 200_000_000);
    assert_eq!(r.symbol_exposure_micros.len(), 2);
}

#[test]
fn unexpected_fee_deficit_is_valued_and_stops_new_risk() {
    let mut j = seeded(1_000_000, None, 1);
    mark(&mut j, NOW, "100");
    let order = intent(NOW + 100);
    j.prepare(order.clone(), order.created_ms).unwrap();
    j.mark_dispatch_started(&order.client_order_id, NOW + 101)
        .unwrap();
    let r = j
        .observe(
            CryptoOrderObservation {
                account_key: order.account_key.clone(),
                client_order_id: order.client_order_id.clone(),
                broker_order_id: "22222222-2222-4222-8222-222222222222".into(),
                symbol: order.symbol.clone(),
                side: order.side,
                ordered_quantity: order.quantity.clone(),
                limit_price: order.limit_price.clone(),
                state: BrokerOrderState::Filled,
                filled_quantity: order.quantity.clone(),
                fee_usd: d("2"),
                executions: vec![CryptoOrderFill {
                    execution_id: "unexpected-fee-fill".into(),
                    quantity: order.quantity.clone(),
                    price: d("100"),
                    occurred_ms: NOW + 102,
                }],
                updated_ms: NOW + 102,
                received_ms: NOW + 102,
            },
            NOW + 102,
        )
        .unwrap();
    j.reconcile_terminal(
        &order.client_order_id,
        r.record.observation_digest.as_ref().unwrap(),
        NOW + 103,
    )
    .unwrap();
    let risk = mark(&mut j, NOW + 104, "100");
    assert_eq!(risk.equity_micros, 0);
    assert_eq!(risk.equity_deficit_micros, 1_001_000);
    assert!(risk.deficit_latched);
    let snapshot = restart(&j).valued_account("BTC-USD", NOW + 105).unwrap();
    assert!(snapshot.account.loss_stop_latched);
    assert_eq!(snapshot.account.buying_power_micros, 0);
    assert_eq!(snapshot.account.available_quantity, d("0.001"));
}

proptest! {
    #[test]
    fn rounding_never_inflates_equity_and_peak_survives_restart(
        price in 1_u64..1_000_000, fee in 0_u32..=10_000, quantum in 1_u64..10_001
    ) {
        let mut j = seeded(10_000_000, Some("0.001"), quantum);
        let mut i = input(&j, NOW, &price.to_string());
        i.fee_reserve_bps = fee;
        let risk = j.mark_allocation(i, NOW).unwrap().risk;
        let ideal_micros = 10_000_000 + price * 1_000 - (price * 1_000 * u64::from(fee)).div_ceil(10_000);
        prop_assert!(risk.equity_micros <= ideal_micros);
        j = restart(&j);
        let mut i = input(&j, NOW + 1, &(price / 2 + 1).to_string());
        i.fee_reserve_bps = fee;
        let second = j.mark_allocation(i, NOW + 1).unwrap().risk;
        prop_assert!(second.peak_equity_micros >= risk.peak_equity_micros);
        prop_assert_eq!(second.day_open_equity_micros, risk.day_open_equity_micros);
        prop_assert!(!risk.day_loss_latched || second.day_loss_latched);
        prop_assert!(!risk.drawdown_latched || second.drawdown_latched);
    }
}

fn prepared_mark(
    j: &mut CryptoBrokerJournal,
    order: &CryptoOrderIntent,
    at: u64,
    price: &str,
) -> CryptoValuedAccount {
    let prepared = j.prepared_allocation(&order.client_order_id, at).unwrap();
    let mut i = input(j, at, price);
    i.expected_balance_digest = prepared.balance_digest;
    let receipt = j
        .mark_prepared_allocation(prepared.scope.clone(), i, at)
        .unwrap();
    assert!(receipt.risk.current);
    j.prepared_valued_account(&prepared.scope, at).unwrap()
}

#[test]
fn prepared_buy_can_recheck_its_reservation_without_releasing_it() {
    let mut j = seeded(100_000_000, None, 1);
    mark(&mut j, NOW, "100");
    let order = intent(NOW + 100);
    j.prepare(order.clone(), order.created_ms).unwrap();
    let reserved = j.allocation_balance().unwrap().unwrap();
    assert_eq!(reserved.available_cash_micros, 99_000_000);
    assert!(reserved.unresolved_orders);
    let before = j.save_snapshot().unwrap();
    let view = j
        .prepared_allocation(&order.client_order_id, NOW + 100)
        .unwrap();
    assert_eq!(view.balance.available_cash_micros, 100_000_000);
    assert!(!view.balance.unresolved_orders && !view.execution_authority);
    assert_eq!(before, j.save_snapshot().unwrap());
    let valued = prepared_mark(&mut j, &order, NOW + 101, "100");
    assert_eq!(valued.account.buying_power_micros, 100_000_000);
    assert_eq!(valued.prepared_order, Some(view.scope.clone()));
    assert_eq!(valued.valid_until_ms, order.expires_ms);
    assert_eq!(j.allocation_balance().unwrap().unwrap(), reserved);
    assert!(
        !j.allocation_risk_state(NOW + 101)
            .unwrap()
            .risk
            .unwrap()
            .current
    );
    assert!(j.valued_account("BTC-USD", NOW + 101).is_err());
    j = restart(&j);
    assert!(j.prepared_valued_account(&view.scope, NOW + 102).is_ok());
    assert!(j
        .prepared_valued_account(&view.scope, order.expires_ms)
        .is_err());
    assert!(j
        .prepared_allocation(&order.client_order_id, order.expires_ms)
        .is_err());
}

#[test]
fn prepared_sell_rechecks_actual_owned_units_and_cannot_erase_loss_latches() {
    let mut j = seeded(100_000_000, Some("0.001"), 1);
    mark(&mut j, NOW, "100000");
    mark(&mut j, NOW + 1, "80000");
    let mut order = intent(NOW + 100);
    order.side = CryptoOrderSide::Sell;
    order.reserved_cash_micros = 0;
    j.prepare(order.clone(), order.created_ms).unwrap();
    let before = j.allocation_balance().unwrap().unwrap();
    assert_eq!(before.available_quantities["BTC-USD"], d("0"));
    let valued = prepared_mark(&mut j, &order, NOW + 101, "110000");
    assert_eq!(valued.account.available_quantity, d("0.001"));
    assert!(valued.account.loss_stop_latched);
    assert_eq!(valued.account.day_open_equity_micros, 199_000_000);
    assert_eq!(valued.account.peak_equity_micros, 208_900_000);
    assert_eq!(valued.account.daily_order_count, 0);
    assert_eq!(j.allocation_balance().unwrap().unwrap(), before);
}

#[test]
fn another_unresolved_order_prevents_order_specific_valuation() {
    let mut j = seeded(100_000_000, None, 1);
    mark(&mut j, NOW, "100");
    let order = intent(NOW + 100);
    j.prepare(order.clone(), order.created_ms).unwrap();
    let view = j
        .prepared_allocation(&order.client_order_id, NOW + 100)
        .unwrap();
    let mut second = intent(NOW + 101);
    second.client_order_id = "33333333-3333-4333-8333-333333333333".into();
    second.risk_reservation_id = "5".repeat(64);
    j.prepare(second, NOW + 101).unwrap();
    assert!(j
        .prepared_allocation(&order.client_order_id, NOW + 101)
        .is_err());
    assert!(j.prepared_valued_account(&view.scope, NOW + 101).is_err());
    let mut i = input(&j, NOW + 101, "100");
    i.expected_balance_digest = view.balance_digest;
    let before = j.save_snapshot().unwrap();
    assert!(j
        .mark_prepared_allocation(view.scope, i, NOW + 101)
        .is_err());
    assert_eq!(before, j.save_snapshot().unwrap());
}

#[test]
fn dispatch_and_recovery_invalidate_scoped_snapshots_but_preserve_history() {
    let mut j = seeded(100_000_000, None, 1);
    mark(&mut j, NOW, "100");
    let order = intent(NOW + 100);
    j.prepare(order.clone(), order.created_ms).unwrap();
    let valued = prepared_mark(&mut j, &order, NOW + 100, "100");
    let scope = valued.prepared_order.unwrap();
    failed(&mut j, &order);
    j = restart(&j);
    assert!(j
        .prepared_allocation(&order.client_order_id, NOW + 104)
        .is_err());
    assert!(j.prepared_valued_account(&scope, NOW + 104).is_err());
    let marked = mark(&mut j, NOW + 105, "100");
    assert_eq!(marked.daily_order_count, 1);
    assert!(j
        .valued_account("BTC-USD", NOW + 105)
        .unwrap()
        .prepared_order
        .is_none());
}

#[test]
fn scope_and_allocation_cas_reject_changed_id_revision_intent_or_balance() {
    let mut j = seeded(100_000_000, None, 1);
    mark(&mut j, NOW, "100");
    let order = intent(NOW + 100);
    j.prepare(order.clone(), order.created_ms).unwrap();
    let view = j
        .prepared_allocation(&order.client_order_id, NOW + 100)
        .unwrap();
    let mut i = input(&j, NOW + 100, "100");
    i.expected_balance_digest = view.balance_digest.clone();
    let before = j.save_snapshot().unwrap();
    for case in 0..4 {
        let mut scope = view.scope.clone();
        let mut input = i.clone();
        match case {
            0 => scope.client_order_id = "33333333-3333-4333-8333-333333333333".into(),
            1 => scope.revision += 1,
            2 => scope.intent_digest = "f".repeat(64),
            3 => input.expected_balance_digest = "e".repeat(64),
            _ => unreachable!(),
        }
        assert!(j.mark_prepared_allocation(scope, input, NOW + 100).is_err());
        assert_eq!(before, j.save_snapshot().unwrap());
    }
}

#[test]
fn scoped_valuation_cannot_initialize_or_reset_missing_history() {
    let mut j = seeded(100_000_000, None, 1);
    let order = intent(NOW + 100);
    j.prepare(order.clone(), order.created_ms).unwrap();
    let view = j
        .prepared_allocation(&order.client_order_id, NOW + 100)
        .unwrap();
    let mut i = input(&j, NOW + 100, "100");
    i.expected_balance_digest = view.balance_digest;
    assert!(j
        .mark_prepared_allocation(view.scope, i, NOW + 100)
        .is_err());
}
