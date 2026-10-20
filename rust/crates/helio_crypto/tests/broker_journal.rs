use helio_crypto::{
    CryptoAccountCoverage, CryptoBrokerJournal, CryptoJournalError, CryptoOrderFill,
    CryptoOrderIntent, CryptoOrderObservation, CryptoOrderPhase, CryptoOrderSide,
    CryptoRecoveryAction,
};
use helio_execution::{BrokerDecimal, BrokerOrderState};
use proptest::prelude::*;

fn d(value: &str) -> BrokerDecimal {
    BrokerDecimal::try_new(value).unwrap()
}

fn account() -> String {
    "1".repeat(64)
}

fn intent() -> CryptoOrderIntent {
    CryptoOrderIntent {
        account_key: account(),
        client_order_id: "11111111-1111-4111-8111-000000000001".into(),
        symbol: "BTC-USD".into(),
        side: CryptoOrderSide::Buy,
        quantity: d("0.00012345"),
        limit_price: d("100000"),
        reserved_cash_micros: 20_000_000,
        policy_revision: 7,
        strategy_fingerprint: "2".repeat(64),
        input_digest: "3".repeat(64),
        risk_reservation_id: "4".repeat(64),
        created_ms: 1_000,
        expires_ms: 100_000,
    }
}

fn journal() -> CryptoBrokerJournal {
    let mut journal = CryptoBrokerJournal::new(account()).unwrap();
    journal.prepare(intent(), 1_000).unwrap();
    journal
}

fn dispatched() -> CryptoBrokerJournal {
    let mut journal = journal();
    journal
        .mark_dispatch_started(&intent().client_order_id, 2_000)
        .unwrap();
    journal
}

fn fill(id: &str, quantity: &str) -> CryptoOrderFill {
    CryptoOrderFill {
        execution_id: id.into(),
        quantity: d(quantity),
        price: d("99999.123456789"),
        occurred_ms: 3_000,
    }
}

fn observation(state: BrokerOrderState, quantity: &str) -> CryptoOrderObservation {
    let intent = intent();
    CryptoOrderObservation {
        account_key: account(),
        client_order_id: intent.client_order_id,
        broker_order_id: "22222222-2222-4222-8222-000000000002".into(),
        symbol: intent.symbol,
        side: intent.side,
        ordered_quantity: intent.quantity,
        limit_price: intent.limit_price,
        state,
        filled_quantity: d(quantity),
        fee_usd: d("0"),
        executions: if quantity == "0" {
            vec![]
        } else {
            vec![fill("fill-a", quantity)]
        },
        updated_ms: 4_000,
        received_ms: 5_000,
    }
}

fn coverage(journal: &CryptoBrokerJournal, at: u64) -> CryptoAccountCoverage {
    CryptoAccountCoverage {
        account_key: account(),
        observation_digest: journal
            .record(&intent().client_order_id)
            .unwrap()
            .observation_digest
            .clone()
            .unwrap(),
        account_snapshot_digest: "5".repeat(64),
        snapshot_observed_ms: at,
    }
}

fn roundtrip(journal: &CryptoBrokerJournal) -> CryptoBrokerJournal {
    CryptoBrokerJournal::restore_snapshot(&account(), &journal.save_snapshot().unwrap()).unwrap()
}

#[test]
fn deadline_work_is_durable_and_does_not_release_a_possible_broker_order() {
    let id = intent().client_order_id;
    let mut sent = dispatched();
    assert_eq!(
        sent.plan_recovery(99_999).unwrap().items[0].action,
        CryptoRecoveryAction::LookupOrder
    );
    let work = sent.plan_recovery(100_000).unwrap();
    assert!(!work.execution_authority);
    assert_eq!(work.items.len(), 1);
    assert_eq!(work.items[0].action, CryptoRecoveryAction::CancelOrder);
    assert_eq!(work.items[0].client_order_id, id);
    assert_eq!(sent.record(&id).unwrap().cancel_requested_ms, Some(100_000));
    assert_eq!(
        sent.record(&id).unwrap().phase,
        CryptoOrderPhase::SubmissionUncertain
    );
    sent = roundtrip(&sent);
    let saved = sent.save_snapshot().unwrap();
    assert_eq!(sent.plan_recovery(200_000).unwrap().items, work.items);
    assert_eq!(sent.save_snapshot().unwrap(), saved);
    assert_eq!(sent.reservations().unwrap().cash_micros, 20_000_000);

    let mut unsent = journal();
    assert!(unsent.plan_recovery(99_999).unwrap().items.is_empty());
    assert!(unsent.plan_recovery(100_000).unwrap().items.is_empty());
    assert_eq!(
        unsent.record(&id).unwrap().phase,
        CryptoOrderPhase::ExpiredUnsent
    );
    assert_eq!(unsent.record(&id).unwrap().dispatch_started_ms, None);
    assert_eq!(roundtrip(&unsent).reservations().unwrap().cash_micros, 0);
}

#[test]
fn late_fill_changes_recovery_work_to_account_reconciliation() {
    let id = intent().client_order_id;
    let mut journal = dispatched();
    journal.plan_recovery(100_000).unwrap();
    let mut fill = observation(BrokerOrderState::Filled, "0.00012345");
    fill.updated_ms = 100_001;
    fill.received_ms = 100_002;
    journal.observe(fill, 100_002).unwrap();
    let work = journal.plan_recovery(100_003).unwrap();
    assert_eq!(work.items[0].action, CryptoRecoveryAction::ReconcileAccount);
    assert_eq!(journal.reservations().unwrap().cash_micros, 20_000_000);
    journal
        .cover_account(&id, coverage(&journal, 100_004), 100_004)
        .unwrap();
    assert!(roundtrip(&journal)
        .plan_recovery(100_005)
        .unwrap()
        .items
        .is_empty());
}

#[test]
fn deadline_batch_failure_cannot_partially_release_other_reservations() {
    let mut journal = journal();
    let mut second = intent();
    second.client_order_id = "33333333-3333-4333-8333-000000000003".into();
    second.risk_reservation_id = "6".repeat(64);
    journal.prepare(second.clone(), 1_000).unwrap();
    let mut encoded: serde_json::Value =
        serde_json::from_slice(&journal.save_snapshot().unwrap()).unwrap();
    encoded["records"][&second.client_order_id]["revision"] = u64::MAX.into();
    let mut journal =
        CryptoBrokerJournal::restore_snapshot(&account(), &serde_json::to_vec(&encoded).unwrap())
            .unwrap();
    let before = journal.save_snapshot().unwrap();
    assert_eq!(
        journal.plan_recovery(100_000),
        Err(CryptoJournalError::Capacity)
    );
    assert_eq!(journal.save_snapshot().unwrap(), before);
    assert_eq!(
        journal.plan_recovery(999),
        Err(CryptoJournalError::InvalidClock)
    );
    assert_eq!(journal.save_snapshot().unwrap(), before);
}

proptest! {
    #[test]
    fn deadline_recovery_never_converts_uncertain_submission_to_free_cash(
        now in 100_000_u64..u64::MAX,
        was_dispatched in any::<bool>(),
    ) {
        let mut state = if was_dispatched { dispatched() } else { journal() };
        let work = state.plan_recovery(now).unwrap();
        let restored = roundtrip(&state);
        prop_assert!(!work.execution_authority);
        prop_assert_eq!(restored.reservations().unwrap().cash_micros,
            if was_dispatched { 20_000_000 } else { 0 });
        prop_assert_eq!(work.items.len(), usize::from(was_dispatched));
        if was_dispatched {
            prop_assert_eq!(work.items[0].action, CryptoRecoveryAction::CancelOrder);
            prop_assert_eq!(restored.record(&intent().client_order_id).unwrap().dispatch_started_ms, Some(2_000));
        }
    }
}

#[test]
fn restart_never_turns_a_lost_dispatch_reply_into_a_new_attempt() {
    let mut journal = journal();
    let id = intent().client_order_id;
    let first = journal.mark_dispatch_started(&id, 2_000).unwrap();
    assert!(first.changed);
    assert!(!first.execution_authority);
    assert_eq!(first.record.phase, CryptoOrderPhase::SubmissionUncertain);
    journal = roundtrip(&journal);
    assert!(!journal.prepare(intent(), 3_000).unwrap().changed);
    let repeated = journal.mark_dispatch_started(&id, 4_000).unwrap();
    assert!(!repeated.changed);
    assert_eq!(repeated.record.dispatch_started_ms, Some(2_000));
    assert_eq!(journal.reservations().unwrap().cash_micros, 20_000_000);
    // Expiration after an uncertain send cannot prove the broker did not accept it.
    assert!(!journal.mark_dispatch_started(&id, 200_000).unwrap().changed);
    assert_eq!(journal.reservations().unwrap().cash_micros, 20_000_000);
}

#[test]
fn immutable_client_identity_and_external_reservation_cannot_be_reused() {
    let mut journal = journal();
    let before = journal.save_snapshot().unwrap();
    let mut changed = intent();
    changed.quantity = d("0.0002");
    assert_eq!(
        journal.prepare(changed, 2_000),
        Err(CryptoJournalError::IdentityConflict)
    );
    let mut duplicate_reservation = intent();
    duplicate_reservation.client_order_id = "33333333-3333-4333-8333-000000000003".into();
    assert_eq!(
        journal.prepare(duplicate_reservation, 2_000),
        Err(CryptoJournalError::IdentityConflict)
    );
    assert_eq!(journal.save_snapshot().unwrap(), before);
}

#[test]
fn only_a_never_dispatched_expired_intent_releases_its_reservation_locally() {
    let mut journal = journal();
    let receipt = journal
        .mark_dispatch_started(&intent().client_order_id, 100_000)
        .unwrap();
    assert_eq!(receipt.record.phase, CryptoOrderPhase::ExpiredUnsent);
    assert_eq!(receipt.record.dispatch_started_ms, None);
    assert_eq!(journal.reservations().unwrap().cash_micros, 0);
    assert_eq!(roundtrip(&journal), journal);
    assert_eq!(
        journal.observe(observation(BrokerOrderState::Filled, "0.00012345"), 100_001),
        Err(CryptoJournalError::InvalidPhase)
    );
}

#[test]
fn cancellation_keeps_reservation_and_accepts_a_fill_racing_with_cancel() {
    let mut journal = dispatched();
    let id = intent().client_order_id;
    journal
        .observe(observation(BrokerOrderState::Open, "0"), 5_000)
        .unwrap();
    let cancellation = journal.request_cancel(&id, 6_000).unwrap();
    assert_eq!(cancellation.record.phase, CryptoOrderPhase::Open);
    assert_eq!(journal.reservations().unwrap().cash_micros, 20_000_000);
    journal = roundtrip(&journal);
    let mut report = observation(BrokerOrderState::Canceled, "0.000000000000000007");
    report.updated_ms = 7_000;
    report.received_ms = 8_000;
    journal.observe(report.clone(), 8_000).unwrap();
    assert_eq!(journal.reservations().unwrap().cash_micros, 20_000_000);
    let terminal_coverage = coverage(&journal, 9_000);
    journal
        .cover_account(&id, terminal_coverage.clone(), 9_000)
        .unwrap();
    assert_eq!(journal.reservations().unwrap().cash_micros, 0);

    // The final venue report reveals another fill after cancellation. Re-open the reservation
    // until an account snapshot includes the new result; do not lose the earlier tiny fill.
    report.state = BrokerOrderState::Filled;
    report.filled_quantity = d("0.00012345");
    report
        .executions
        .push(fill("fill-b", "0.000123449999999993"));
    report.updated_ms = 10_000;
    report.received_ms = 11_000;
    journal.observe(report, 11_000).unwrap();
    assert_eq!(journal.reservations().unwrap().cash_micros, 20_000_000);
    assert_eq!(
        journal.cover_account(&id, terminal_coverage, 12_000),
        Err(CryptoJournalError::InvalidCoverage)
    );
    journal
        .cover_account(&id, coverage(&journal, 12_000), 12_000)
        .unwrap();
    assert_eq!(journal.reservations().unwrap().cash_micros, 0);
    assert_eq!(roundtrip(&journal), journal);
}

#[test]
fn revised_terminal_fee_invalidates_account_coverage_but_duplicate_receipts_do_not() {
    let mut journal = dispatched();
    let id = intent().client_order_id;
    let mut report = observation(BrokerOrderState::Filled, "0.00012345");
    journal.observe(report.clone(), 5_000).unwrap();
    journal
        .cover_account(&id, coverage(&journal, 6_000), 6_000)
        .unwrap();
    report.received_ms = 7_000;
    assert!(!journal.observe(report.clone(), 7_000).unwrap().changed);
    assert_eq!(journal.reservations().unwrap().cash_micros, 0);
    report.fee_usd = d("0.000000000000000001");
    report.updated_ms = 8_000;
    report.received_ms = 9_000;
    journal.observe(report, 9_000).unwrap();
    assert_eq!(journal.reservations().unwrap().cash_micros, 20_000_000);
}

#[test]
fn execution_ordering_is_canonical_and_exact_decimal_comparisons_are_numeric() {
    let mut journal = dispatched();
    let mut report = observation(BrokerOrderState::Filled, "0.00012345");
    report.executions = vec![fill("z", "0.00002345"), fill("a", "0.0001")];
    // Lexical comparison would reject "99999" against "100000".
    journal.observe(report.clone(), 5_000).unwrap();
    let before = journal.save_snapshot().unwrap();
    report.executions.reverse();
    report.received_ms = 6_000;
    assert!(!journal.observe(report, 6_000).unwrap().changed);
    assert_eq!(journal.save_snapshot().unwrap(), before);
}

#[test]
fn partial_fills_do_not_release_cash_and_cannot_be_covered_as_terminal() {
    let mut journal = dispatched();
    journal
        .observe(
            observation(BrokerOrderState::PartiallyFilled, "0.0001"),
            5_000,
        )
        .unwrap();
    assert_eq!(
        journal.cover_account(&intent().client_order_id, coverage(&journal, 6_000), 6_000),
        Err(CryptoJournalError::InvalidCoverage)
    );
    assert_eq!(journal.reservations().unwrap().cash_micros, 20_000_000);
}

#[test]
fn sell_inventory_is_reserved_exactly_and_limit_direction_is_checked() {
    let mut journal = CryptoBrokerJournal::new(account()).unwrap();
    let mut sell = intent();
    sell.side = CryptoOrderSide::Sell;
    sell.reserved_cash_micros = 0;
    sell.quantity = d("0.000000000000000007");
    let id = sell.client_order_id.clone();
    journal.prepare(sell.clone(), 1_000).unwrap();
    journal.mark_dispatch_started(&id, 2_000).unwrap();
    assert_eq!(
        journal.reservations().unwrap().sell_quantities["BTC-USD"],
        sell.quantity
    );
    let mut report = observation(BrokerOrderState::Filled, sell.quantity.as_str());
    report.side = sell.side;
    report.ordered_quantity = sell.quantity;
    let before = journal.save_snapshot().unwrap();
    assert_eq!(
        journal.observe(report.clone(), 5_000),
        Err(CryptoJournalError::InvalidObservation)
    );
    assert_eq!(journal.save_snapshot().unwrap(), before);
    report.executions[0].price = d("100001");
    journal.observe(report, 5_000).unwrap();
    journal
        .cover_account(&id, coverage(&journal, 6_000), 6_000)
        .unwrap();
    assert!(journal.reservations().unwrap().sell_quantities.is_empty());
}

#[test]
fn stale_or_wrong_account_coverage_cannot_release_a_reservation() {
    let mut journal = dispatched();
    journal
        .observe(observation(BrokerOrderState::Filled, "0.00012345"), 5_000)
        .unwrap();
    let before = journal.save_snapshot().unwrap();
    let mut wrong = coverage(&journal, 6_000);
    wrong.account_key = "f".repeat(64);
    assert_eq!(
        journal.cover_account(&intent().client_order_id, wrong, 6_000),
        Err(CryptoJournalError::InvalidCoverage)
    );
    assert_eq!(
        journal.cover_account(&intent().client_order_id, coverage(&journal, 4_999), 6_000),
        Err(CryptoJournalError::InvalidCoverage)
    );
    assert_eq!(journal.save_snapshot().unwrap(), before);
}

#[test]
fn changed_snapshots_are_rejected_instead_of_releasing_unknown_orders() {
    let journal = dispatched();
    let id = intent().client_order_id;
    let original: serde_json::Value =
        serde_json::from_slice(&journal.save_snapshot().unwrap()).unwrap();
    for field in ["phase", "dispatch_started_ms", "intent_digest"] {
        let mut snapshot = original.clone();
        snapshot["records"][&id][field] = serde_json::json!(null);
        assert!(CryptoBrokerJournal::restore_snapshot(
            &account(),
            &serde_json::to_vec(&snapshot).unwrap()
        )
        .is_err());
    }
    let mut snapshot = original.clone();
    snapshot["records"][&id]["intent"]["quantity"] = serde_json::json!("1");
    assert!(CryptoBrokerJournal::restore_snapshot(
        &account(),
        &serde_json::to_vec(&snapshot).unwrap()
    )
    .is_err());
    assert!(CryptoBrokerJournal::restore_snapshot(
        &"f".repeat(64),
        &journal.save_snapshot().unwrap()
    )
    .is_err());
    assert!(CryptoBrokerJournal::restore_snapshot(&account(), b"{").is_err());
}

#[test]
fn cumulative_fees_cannot_regress_and_clock_failures_leave_state_unchanged() {
    let mut journal = dispatched();
    let mut report = observation(BrokerOrderState::Filled, "0.00012345");
    report.fee_usd = d("0.1");
    journal.observe(report.clone(), 5_000).unwrap();
    let before = journal.save_snapshot().unwrap();
    report.updated_ms = 6_000;
    report.received_ms = 7_000;
    report.fee_usd = d("0.01");
    assert_eq!(
        journal.observe(report, 7_000),
        Err(CryptoJournalError::ObservationRegression)
    );
    assert_eq!(
        journal.request_cancel(&intent().client_order_id, 4_999),
        Err(CryptoJournalError::InvalidClock)
    );
    assert_eq!(journal.save_snapshot().unwrap(), before);
}

proptest! {
    #[test]
    fn invalid_observations_never_partly_mutate_state(kind in 0_u8..12) {
        let mut journal = dispatched();
        let mut report = observation(BrokerOrderState::PartiallyFilled, "0.0001");
        journal.observe(report.clone(), 5_000).unwrap();
        report.updated_ms = 6_000;
        report.received_ms = 7_000;
        match kind {
            0 => report.account_key = "f".repeat(64),
            1 => report.broker_order_id = "33333333-3333-4333-8333-000000000003".into(),
            2 => report.executions.clear(),
            3 => report.executions.push(report.executions[0].clone()),
            4 => report.filled_quantity = d("0.0002"),
            5 => report.executions[0].price = d("100001"),
            6 => report.executions[0].quantity = d("0.00001"),
            7 => report.state = BrokerOrderState::Open,
            8 => report.updated_ms = 1_000,
            9 => report.received_ms = 8_000,
            10 => report.symbol = "ETH-USD".into(),
            _ => report.side = CryptoOrderSide::Sell,
        }
        let before = journal.save_snapshot().unwrap();
        prop_assert!(journal.observe(report, 7_000).is_err());
        prop_assert_eq!(journal.save_snapshot().unwrap(), before);
    }

    #[test]
    fn snapshot_replay_preserves_partial_fill_arithmetic_and_identity(
        amount in 1_u32..50_000,
        restore_after_prepare in any::<bool>(),
        restore_after_dispatch in any::<bool>(),
        restore_after_fill in any::<bool>(),
    ) {
        let quantity = |n: u32| d(&format!("0.{n:08}"));
        let mut planned = intent();
        planned.quantity = quantity(amount * 2);
        let mut journal = CryptoBrokerJournal::new(account()).unwrap();
        journal.prepare(planned.clone(), 1_000).unwrap();
        if restore_after_prepare { journal = roundtrip(&journal); }
        journal.mark_dispatch_started(&planned.client_order_id, 2_000).unwrap();
        if restore_after_dispatch { journal = roundtrip(&journal); }
        let mut partial = observation(BrokerOrderState::PartiallyFilled, "0.0001");
        partial.ordered_quantity = planned.quantity.clone();
        partial.filled_quantity = quantity(amount);
        partial.executions[0].quantity = quantity(amount);
        journal.observe(partial.clone(), 5_000).unwrap();
        if restore_after_fill { journal = roundtrip(&journal); }
        partial.state = BrokerOrderState::Filled;
        partial.filled_quantity = planned.quantity.clone();
        let mut last = partial.executions[0].clone();
        last.execution_id = "fill-b".into();
        partial.executions.push(last);
        partial.updated_ms = 6_000;
        partial.received_ms = 7_000;
        journal.observe(partial, 7_000).unwrap();
        prop_assert_eq!(&journal.record(&planned.client_order_id).unwrap().intent, &planned);
        prop_assert_eq!(journal.reservations().unwrap().cash_micros, planned.reserved_cash_micros);
        prop_assert!(!journal.mark_dispatch_started(&planned.client_order_id, 8_000).unwrap().changed);
        prop_assert_eq!(roundtrip(&journal), journal);
    }
}
