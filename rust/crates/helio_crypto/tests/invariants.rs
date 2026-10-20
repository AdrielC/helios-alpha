use std::collections::{BTreeMap, BTreeSet};

use helio_crypto::*;
use helio_execution::{ExecutionMode, OrderProposal, PriceMicros, QuantityMicros, Side};
use proptest::prelude::*;
use serde_json::json;

fn limits() -> CryptoRiskLimits {
    CryptoRiskLimits {
        capital_micros: 300_000_000,
        order_micros: 30_000_000,
        daily_loss_micros: 20_000_000,
        drawdown_micros: 60_000_000,
        symbol_exposure_micros: 180_000_000,
        max_daily_orders: 12,
        max_spread_bps: 50,
        max_quote_age_ms: 5000,
        symbols: vec!["BTC-USD".into(), "ETH-USD".into()],
        leverage: false,
    }
}

fn config() -> ControlBootstrap {
    ControlBootstrap {
        limits: limits(),
        clients: BTreeMap::from([
            (
                "observer".into(),
                BTreeSet::from(["risk:read".into(), "status:read".into()]),
            ),
            (
                "owner".into(),
                BTreeSet::from([
                    "risk:read".into(),
                    "risk:write".into(),
                    "status:read".into(),
                    "audit:read".into(),
                    "execution:pause".into(),
                    "decisions:reserve".into(),
                    "runtime:write".into(),
                ]),
            ),
        ]),
        decision_daily_calls: 3,
    }
}

fn snapshot(state: &ControlState) -> ControlState {
    let restored: ControlState =
        serde_json::from_slice(&serde_json::to_vec(state).unwrap()).unwrap();
    restored.validate_snapshot().unwrap();
    restored
}

#[test]
fn revisions_acl_pause_and_decision_budget_survive_snapshot() {
    let mut state = ControlState::new(config(), 10).unwrap();
    let update = RiskPolicyUpdate {
        expected_revision: 1,
        request_id: "u1".into(),
        limits: limits(),
        reason: "test change".into(),
    };
    assert_eq!(
        state.update("observer", update.clone(), 11).unwrap_err(),
        ControlStateError::Forbidden
    );
    assert_eq!(
        state.update("owner", update.clone(), 11).unwrap().revision,
        2
    );
    state.pause("owner", 12).unwrap();
    assert!(state.reserve_decision("owner", "d1", 13).unwrap());
    let mut state = snapshot(&state);
    assert_eq!(
        state.update("owner", update.clone(), 14).unwrap().revision,
        2
    );
    assert!(!state.reserve_decision("owner", "d1", 14).unwrap());
    assert!(state.status("observer").unwrap()["paused"]
        .as_bool()
        .unwrap());
    assert!(state.reserve_decision("owner", "d2", 15).unwrap());
    assert!(state.reserve_decision("owner", "d3", 16).unwrap());
    assert_eq!(
        state.reserve_decision("owner", "d4", 17),
        Err(ControlStateError::BudgetExhausted)
    );
    let mut changed = update;
    changed.reason = "different operation".into();
    assert_eq!(
        state.update("owner", changed, 18).unwrap_err(),
        ControlStateError::IdentityConflict
    );
}

#[test]
fn corrupted_policy_or_audit_is_rejected_on_restore() {
    let state = ControlState::new(config(), 10).unwrap();
    for pointer in [
        "/revisions/0/limits/order_micros",
        "/audit/0/details/revision",
    ] {
        let mut value = serde_json::to_value(&state).unwrap();
        *value.pointer_mut(pointer).unwrap() = json!(2);
        assert!(serde_json::from_value::<ControlState>(value)
            .unwrap()
            .validate_snapshot()
            .is_err());
    }
}

fn market() -> (CryptoQuote, CryptoAccountSnapshot, OrderProposal) {
    (
        CryptoQuote {
            symbol: "BTC-USD".into(),
            bid_micros: 100_000_000,
            ask_micros: 100_100_000,
            observed_ms: 10000,
        },
        CryptoAccountSnapshot {
            observed_ms: 10000,
            buying_power_micros: 300_000_000,
            equity_micros: 300_000_000,
            day_open_equity_micros: 300_000_000,
            peak_equity_micros: 300_000_000,
            gross_exposure_micros: 0,
            symbol_exposure_micros: 0,
            available_quantity_micros: 300000,
            daily_order_count: 0,
            unresolved_orders: false,
            paused: false,
            loss_stop_latched: false,
        },
        OrderProposal {
            proposal_id: "p1".into(),
            strategy_id: "private".into(),
            symbol: "BTC-USD".into(),
            venue: "CRYPTO".into(),
            currency: "USD".into(),
            side: Side::Buy,
            quantity: QuantityMicros(200000),
            limit_price: PriceMicros(100_100_000),
            mode: ExecutionMode::Paper,
            trading_day: 0,
        },
    )
}

#[test]
fn ambiguity_and_pause_block_even_risk_reducing_orders() {
    let (quote, mut account, mut order) = market();
    order.side = Side::Sell;
    account.unresolved_orders = true;
    assert_eq!(
        assess_crypto_order(&limits(), &quote, &account, &order, 50, 10000),
        Err(CryptoRiskRejection::ReconciliationRequired)
    );
    account.unresolved_orders = false;
    account.paused = true;
    assert_eq!(
        assess_crypto_order(&limits(), &quote, &account, &order, 50, 10000),
        Err(CryptoRiskRejection::Paused)
    );
    account.paused = false;
    account.loss_stop_latched = true;
    assert!(assess_crypto_order(&limits(), &quote, &account, &order, 50, 10000).is_ok());
    order.side = Side::Buy;
    assert_eq!(
        assess_crypto_order(&limits(), &quote, &account, &order, 50, 10000),
        Err(CryptoRiskRejection::LossLimit)
    );
}

fn input(prices: &[u64], costs: u32) -> ReplayInput {
    ReplayInput {
        symbol: "BTC-USD".into(),
        bars: prices
            .iter()
            .enumerate()
            .map(|(i, p)| Candle {
                start_ms: (i as u64 + 1) * 3_600_000,
                duration_ms: 3_600_000,
                open_micros: *p,
                high_micros: *p,
                low_micros: *p,
                close_micros: *p,
            })
            .collect(),
        targets: vec![Target {
            available_ms: 3_600_000,
            allocation_bps: 5000,
        }],
        limits: limits(),
        initial_cash_micros: 300_000_000,
        fee_bps: costs,
        friction_bps: costs,
        minimum_order_micros: 5_000_000,
    }
}

#[test]
fn replay_cannot_trade_on_the_bar_that_made_a_signal_available() {
    let report = replay(&input(&[100_000_000; 6], 20)).unwrap();
    assert!(report.fills.iter().all(|f| f.at_ms > f.signal_available_ms));
    assert_eq!(report.equity[0].quantity_micros, 0);
    assert!(report.net_pnl_micros < 0);
}

#[test]
fn replay_rejects_gaps_and_future_information_does_not_change_the_prefix() {
    let base = input(&[100_000_000; 8], 10);
    let first = replay(&base).unwrap();
    let mut changed = base.clone();
    changed.bars[7].close_micros = 200_000_000;
    changed.bars[7].high_micros = 200_000_000;
    let second = replay(&changed).unwrap();
    assert_eq!(first.equity[..7], second.equity[..7]);
    assert_eq!(first.fills, second.fills);
    changed.bars.remove(3);
    assert_eq!(replay(&changed).unwrap_err(), CryptoError::InvalidCandles);
}

fn legacy_paper() -> ControlState {
    let mut state = ControlState::new(config(), 1).unwrap();
    state
        .initialize_paper(
            "owner",
            PaperConfig {
                initial_cash_micros: 300_000_000,
                fee_bps: 50,
                slippage_bps: 10,
                minimum_order_micros: 1_000_000,
            },
            2,
        )
        .unwrap();
    state
}

fn paper() -> ControlState {
    let mut state = legacy_paper();
    state.enable_paper_signal_deadlines("owner", 3).unwrap();
    state
}

#[test]
fn deadline_activation_preserves_legacy_replay_and_is_permanent() {
    let mut state = legacy_paper();
    let original = serde_json::to_value(&state).unwrap();
    assert!(original["paper"].get("signal_deadline_enforced").is_none());
    let mut legacy = step();
    legacy.signal = None;
    assert!(serde_json::to_value(&legacy)
        .unwrap()
        .get("signal")
        .is_none());
    let receipt = state
        .paper_step("owner", legacy.clone(), 10_000_000_000)
        .unwrap();
    assert_eq!(receipt["outcome"], "simulated_fill");
    let mut state = snapshot(&state);
    let before = state.status("owner").unwrap()["paper"].clone();
    assert!(before.get("signal_deadline_version").is_none());
    assert_eq!(
        state.enable_paper_signal_deadlines("observer", 10_000_000_001),
        Err(ControlStateError::Forbidden)
    );
    assert_eq!(state.status("owner").unwrap()["paper"], before);
    let mut expected = before;
    expected["signal_deadline_version"] = json!(1);
    assert_eq!(
        state
            .enable_paper_signal_deadlines("owner", 10_000_000_001)
            .unwrap(),
        expected
    );
    let audit = state.audit("owner", 0, 200).unwrap();
    let mut state = snapshot(&state);
    assert_eq!(
        state
            .enable_paper_signal_deadlines("owner", 10_000_000_002)
            .unwrap(),
        expected
    );
    assert_eq!(
        serde_json::to_value(state.audit("owner", 0, 200).unwrap()).unwrap(),
        serde_json::to_value(audit).unwrap()
    );
    assert_eq!(
        state
            .paper_step("owner", legacy.clone(), 10_000_000_002)
            .unwrap(),
        receipt
    );
    legacy.source_sequence = 2;
    legacy.expected_cursor = 1;
    legacy.available_ms += 1;
    legacy.quote.observed_ms += 1;
    let missing = state.paper_step("owner", legacy, 10_001_000_000).unwrap();
    assert_eq!(missing["outcome"], "missing_signal_validity");
    assert_eq!(missing["total_orders"], 1);
}

fn step() -> PaperStep {
    PaperStep {
        source_id: "test-stream-v1".into(),
        source_sequence: 1,
        expected_cursor: 0,
        expected_policy_revision: 1,
        event_id: "event-1".into(),
        available_ms: 10000,
        quote: market().0,
        portfolio_quotes: vec![],
        target_micros: 100_000_000,
        observation_only: false,
        signal: Some(CryptoSignalValidity {
            input_digest: "a".repeat(64),
            available_ms: 10000,
            expires_ms: 130000,
        }),
    }
}

#[test]
fn paper_fill_and_input_cursor_survive_crash_without_double_fill() {
    let mut state = paper();
    let input = step();
    let receipt = state
        .paper_step("owner", input.clone(), 10_000_000_000)
        .unwrap();
    assert_eq!(receipt["outcome"], "simulated_fill");
    let mut restored = snapshot(&state);
    assert_eq!(
        restored
            .paper_step("owner", input.clone(), 11_000_000_000)
            .unwrap(),
        receipt
    );
    assert_eq!(
        restored.status("owner").unwrap()["paper"]["total_orders"],
        1
    );
    let mut changed = input;
    changed.target_micros = 0;
    assert_eq!(
        restored
            .paper_step("owner", changed, 12_000_000_000)
            .unwrap_err(),
        ControlStateError::IdentityConflict
    );
}

#[test]
fn expired_backlog_advances_without_trading_and_pause_survives_restart() {
    let mut state = paper();
    let receipt = state.paper_step("owner", step(), 200_000_000_000).unwrap();
    assert_eq!(receipt["outcome"], "stale_input");
    assert_eq!(receipt["total_orders"], 0);
    state.pause("owner", 200_000_000_001).unwrap();
    let mut state = snapshot(&state);
    let mut input = step();
    input.source_sequence = 2;
    input.expected_cursor = 1;
    input.available_ms = 200001;
    input.quote.observed_ms = 200001;
    assert_eq!(
        state.paper_step("owner", input, 200_001_000_000).unwrap()["outcome"],
        "paused"
    );
    assert_eq!(state.status("owner").unwrap()["paper"]["total_orders"], 0);
}

#[test]
fn missing_legacy_signal_cannot_increase_inventory_but_remains_consumable() {
    let mut state = paper();
    let before = state.status("owner").unwrap()["paper"].clone();
    let mut input = step();
    input.signal = None;
    let wire = serde_json::to_value(&input).unwrap();
    assert!(wire.get("signal").is_none());
    let input: PaperStep = serde_json::from_value(wire).unwrap();
    let receipt = state
        .paper_step("owner", input.clone(), 10_000_000_000)
        .unwrap();
    assert_eq!(receipt["outcome"], "missing_signal_validity");
    assert_eq!(receipt["cursor"], 1);
    let mut restored = snapshot(&state);
    assert_eq!(
        restored
            .paper_step("owner", input, 200_000_000_000)
            .unwrap(),
        receipt
    );
    let after = restored.status("owner").unwrap()["paper"].clone();
    for field in ["cash_micros", "positions", "fees_micros", "total_orders"] {
        assert_eq!(before[field], after[field], "{field}");
    }
}

#[test]
fn redelivery_cannot_replace_the_original_signal_deadline() {
    let mut state = paper();
    let mut input = step();
    input.signal.as_mut().unwrap().expires_ms = 10000;
    let receipt = state
        .paper_step("owner", input.clone(), 10_000_000_000)
        .unwrap();
    assert_eq!(receipt["outcome"], "stale_signal");
    let mut state = snapshot(&state);
    assert_eq!(
        state
            .paper_step("owner", input.clone(), 11_000_000_000)
            .unwrap(),
        receipt
    );
    input.signal.as_mut().unwrap().expires_ms = 130000;
    assert_eq!(
        state.paper_step("owner", input, 11_000_000_000),
        Err(ControlStateError::IdentityConflict)
    );
    assert_eq!(state.status("owner").unwrap()["paper"]["total_orders"], 0);
}

#[test]
fn expired_or_absent_news_does_not_block_reducing_inventory() {
    for missing in [false, true] {
        let mut state = paper();
        state.paper_step("owner", step(), 10_000_000_000).unwrap();
        let mut exit = step();
        exit.source_sequence = 2;
        exit.expected_cursor = 1;
        exit.available_ms = 20000;
        exit.quote.observed_ms = 20000;
        exit.target_micros = 0;
        exit.signal.as_mut().unwrap().expires_ms = 11000;
        if missing {
            exit.signal = None;
        }
        let receipt = state.paper_step("owner", exit, 20_000_000_000).unwrap();
        assert_eq!(receipt["outcome"], "simulated_fill");
        let after = state.status("owner").unwrap()["paper"].clone();
        assert_eq!(after["total_orders"], 2);
        assert_eq!(after["positions"]["BTC-USD"], 0);
        assert_eq!(after["recent_fills"][1]["side"], "sell");
    }
}

#[test]
fn observation_rolls_the_risk_day_without_placing_an_order() {
    let mut state = paper();
    state.paper_step("owner", step(), 10_000_000_000).unwrap();
    let before = state.status("owner").unwrap()["paper"].clone();
    let mut observation = step();
    observation.observation_only = true;
    observation.source_sequence = 2;
    observation.expected_cursor = 1;
    observation.available_ms = 86_400_001;
    observation.quote.observed_ms = observation.available_ms;
    state
        .paper_step("owner", observation, 86_400_001_000_000)
        .unwrap();
    let after = snapshot(&state).status("owner").unwrap()["paper"].clone();
    assert_eq!(after["day_utc"], 1);
    assert_eq!(after["daily_orders"], 0);
    for field in ["cash_micros", "positions", "fees_micros", "total_orders"] {
        assert_eq!(before[field], after[field], "{field}");
    }
}

#[test]
fn missing_quote_for_held_asset_still_blocks_increasing_another_asset() {
    let mut state = paper();
    let mut buy_eth = step();
    buy_eth.quote.symbol = "ETH-USD".into();
    state.paper_step("owner", buy_eth, 10_000_000_000).unwrap();
    let mut buy_btc = step();
    buy_btc.source_sequence = 2;
    buy_btc.expected_cursor = 1;
    buy_btc.available_ms = 20000;
    buy_btc.quote.observed_ms = 20000;
    let receipt = state
        .paper_step("owner", buy_btc.clone(), 20_000_000_000)
        .unwrap();
    assert_eq!(receipt["outcome"], "other_held_quote_stale");
    assert_eq!(receipt["total_orders"], 1);
    let mut refreshed_eth = buy_btc.quote.clone();
    refreshed_eth.symbol = "ETH-USD".into();
    refreshed_eth.observed_ms = 21000;
    buy_btc.source_sequence = 3;
    buy_btc.expected_cursor = 2;
    buy_btc.available_ms = 21000;
    buy_btc.quote.observed_ms = 21000;
    buy_btc.portfolio_quotes.push(refreshed_eth);
    let receipt = state.paper_step("owner", buy_btc, 21_000_000_000).unwrap();
    assert_eq!(receipt["outcome"], "simulated_fill");
    assert_eq!(receipt["total_orders"], 2);
}

#[test]
fn fee_change_preserves_account_and_redelivery_receipt() {
    let mut state = paper();
    let receipt = state.paper_step("owner", step(), 10_000_000_000).unwrap();
    let before = state.status("owner").unwrap()["paper"].clone();
    let expected: PaperConfig = serde_json::from_value(before["cost_assumptions"].clone()).unwrap();
    let next = PaperConfig {
        fee_bps: 95,
        ..expected.clone()
    };
    assert_eq!(
        state.update_paper_costs("observer", expected.clone(), next.clone(), 11_000_000_000),
        Err(ControlStateError::Forbidden)
    );
    state
        .update_paper_costs("owner", expected.clone(), next.clone(), 11_000_000_000)
        .unwrap();
    let audit_count = state.audit("owner", 0, 200).unwrap().len();
    let mut state = snapshot(&state);
    state
        .update_paper_costs("owner", expected, next, 12_000_000_000)
        .unwrap();
    assert_eq!(state.audit("owner", 0, 200).unwrap().len(), audit_count);
    let after = state.status("owner").unwrap()["paper"].clone();
    for field in [
        "cash_micros",
        "positions",
        "cursor",
        "total_orders",
        "fees_micros",
    ] {
        assert_eq!(before[field], after[field], "{field}");
    }
    assert_eq!(
        state.paper_step("owner", step(), 13_000_000_000).unwrap(),
        receipt
    );
    let mut later = step();
    later.source_sequence = 2;
    later.expected_cursor = 1;
    later.available_ms = 9000;
    later.quote.observed_ms = 9000;
    assert_eq!(
        state.paper_step("owner", later, 9_000_000_000),
        Err(ControlStateError::ClockRegression)
    );
}

#[test]
fn settings_are_a_coherent_versioned_snapshot_with_bound_permissions() {
    let mut state = paper();
    let before = state.settings("observer").unwrap();
    assert_eq!(before["policy"]["revision"], 1);
    assert_eq!(before["client"]["id"], "observer");
    assert_eq!(before["execution"]["live_admitted"], false);
    assert_eq!(before["money_scale"], 1_000_000);
    assert_eq!(state.settings("unknown"), Err(ControlStateError::Forbidden));
    state
        .update(
            "owner",
            RiskPolicyUpdate {
                expected_revision: 1,
                request_id: "settings-change".into(),
                limits: limits(),
                reason: "test coherent settings".into(),
            },
            3,
        )
        .unwrap();
    state.pause("owner", 4).unwrap();
    let restored = snapshot(&state);
    let after = restored.settings("observer").unwrap();
    assert_eq!(after["policy"]["revision"], 2);
    assert_eq!(after["execution"]["paused"], true);
    assert_eq!(before["execution"]["paused"], false);
    assert_eq!(
        after["client"]["capabilities"],
        before["client"]["capabilities"]
    );
}

#[test]
fn resize_preserves_cursor_and_cannot_erase_a_traded_account() {
    let mut state = paper();
    let mut observation = step();
    observation.observation_only = true;
    state
        .paper_step("owner", observation.clone(), 10_000_000_000)
        .unwrap();
    assert_eq!(
        state.resize_untraded_paper("observer", 300_000_000, 250_000_000, 11_000_000_000),
        Err(ControlStateError::Forbidden)
    );
    state
        .resize_untraded_paper("owner", 300_000_000, 250_000_000, 11_000_000_000)
        .unwrap();
    let mut restored = snapshot(&state);
    restored
        .resize_untraded_paper("owner", 300_000_000, 250_000_000, 12_000_000_000)
        .unwrap();
    let status = restored.status("owner").unwrap()["paper"].clone();
    assert_eq!(status["cash_micros"], 250_000_000);
    assert_eq!(status["cursor"], 1);
    observation.observation_only = false;
    observation.source_sequence = 2;
    observation.expected_cursor = 1;
    observation.available_ms = 13000;
    observation.quote.observed_ms = 13000;
    restored
        .paper_step("owner", observation, 13_000_000_000)
        .unwrap();
    assert_eq!(
        restored.resize_untraded_paper("owner", 250_000_000, 300_000_000, 14_000_000_000),
        Err(ControlStateError::Invalid)
    );
    assert_eq!(
        restored.status("owner").unwrap()["paper"]["total_orders"],
        1
    );
}

proptest! {
    #[test]
    fn fresh_quote_and_delivery_time_cannot_refresh_expired_signal(
        lifetime in 1u64..=120_000,
        delay in 0u64..=300_000,
    ) {
        let mut state = snapshot(&paper());
        let mut input = step();
        let expires = input.signal.as_ref().unwrap().available_ms + lifetime;
        input.signal.as_mut().unwrap().expires_ms = expires;
        let now = expires + delay;
        input.available_ms = now;
        input.quote.observed_ms = now;
        let receipt = state.paper_step("owner", input.clone(), now * 1_000_000).unwrap();
        prop_assert_eq!(&receipt["outcome"], &json!("stale_signal"));
        prop_assert_eq!(&receipt["total_orders"], &json!(0));
        prop_assert_eq!(&receipt["cursor"], &json!(1));
        let mut restored = snapshot(&state);
        prop_assert_eq!(
            restored.paper_step("owner", input, (now + 1) * 1_000_000).unwrap(),
            receipt
        );
        let status = restored.status("owner").unwrap();
        prop_assert_eq!(&status["paper"]["cash_micros"], &json!(300_000_000u64));
        prop_assert_eq!(&status["paper"]["positions"], &json!({}));
    }

    #[test]
    fn observation_records_loss_and_peak_drawdown_without_trading(
        low_price in 100_000u64..30_000_000,
        paused in any::<bool>(),
    ) {
        let mut state = paper();
        state.paper_step("owner", step(), 10_000_000_000).unwrap();
        let before = state.status("owner").unwrap()["paper"].clone();
        let mut observation = step();
        observation.observation_only = true;
        observation.source_sequence = 2;
        observation.expected_cursor = 1;
        observation.available_ms = 11000;
        observation.quote.observed_ms = 11000;
        observation.quote.bid_micros = 400_000_000;
        observation.quote.ask_micros = 400_000_000;
        state.paper_step("owner", observation.clone(), 11_000_000_000).unwrap();
        if paused {
            state.pause("owner", 11_000_000_001).unwrap();
        }
        observation.source_sequence = 3;
        observation.expected_cursor = 2;
        observation.available_ms = 12000;
        observation.quote.observed_ms = 12000;
        observation.quote.bid_micros = low_price;
        observation.quote.ask_micros = low_price;
        let receipt = state.paper_step("owner", observation.clone(), 12_000_000_000).unwrap();
        prop_assert_eq!(&receipt["outcome"], &json!("observation_only"));
        let mut restored = snapshot(&state);
        prop_assert_eq!(
            restored.paper_step("owner", observation.clone(), 12_000_000_001).unwrap(),
            receipt
        );
        let after = restored.status("owner").unwrap()["paper"].clone();
        prop_assert_eq!(&after["day_loss_latched"], &json!(true));
        prop_assert_eq!(&after["drawdown_latched"], &json!(true));
        for field in ["cash_micros", "positions", "fees_micros", "total_orders"] {
            prop_assert_eq!(&before[field], &after[field]);
        }
        // A bounce cannot erase a stop that was reached between trade decisions.
        observation.source_sequence = 4;
        observation.expected_cursor = 3;
        observation.available_ms = 13000;
        observation.quote = market().0;
        observation.quote.observed_ms = 13000;
        restored.paper_step("owner", observation, 13_000_000_000).unwrap();
        let after_bounce = snapshot(&restored).status("owner").unwrap()["paper"].clone();
        prop_assert_eq!(&after_bounce["day_loss_latched"], &json!(true));
        prop_assert_eq!(&after_bounce["drawdown_latched"], &json!(true));
        prop_assert_eq!(&after_bounce["total_orders"], &json!(1));
    }

    #[test]
    fn every_buy_includes_fees_inside_the_order_cap(costs in 0u32..1000) {
        let mut state = paper();
        let expected: PaperConfig = serde_json::from_value(
            state.status("owner").unwrap()["paper"]["cost_assumptions"].clone()
        ).unwrap();
        let next = PaperConfig { fee_bps: costs, ..expected.clone() };
        state.update_paper_costs("owner", expected, next, 3).unwrap();
        state.paper_step("owner", step(), 10_000_000_000).unwrap();
        let account = state.status("owner").unwrap()["paper"].clone();
        prop_assert_eq!(&account["total_orders"], &json!(1));
        let debit = 300_000_000 - account["cash_micros"].as_u64().unwrap();
        prop_assert!(debit <= limits().order_micros);
        let report = replay(&input(&[100_000_000; 8], costs)).unwrap();
        for pair in report.equity.windows(2) {
            prop_assert!(pair[0].cash_micros.saturating_sub(pair[1].cash_micros) <= limits().order_micros);
        }
    }

    #[test]
    fn paper_redelivery_cannot_create_additional_exposure(repeats in 1usize..100) {
        let mut state = paper();
        let input = step();
        for _ in 0..repeats {
            state.paper_step("owner", input.clone(), 10_000_000_000).unwrap();
            state = snapshot(&state);
        }
        let status = state.status("owner").unwrap();
        prop_assert_eq!(&status["paper"]["total_orders"], &json!(1));
        prop_assert!(status["paper"]["cash_micros"].as_u64().unwrap() < 300_000_000);
        prop_assert!(status["paper"]["equity_micros"].as_u64().unwrap() <= 300_000_000);
    }
    #[test]
    fn fees_cannot_create_money_in_a_flat_market(price in 1_000_000u64..100_000_000_000, costs in 0u32..1000, n in 2usize..80) {
        let report = replay(&input(&vec![price; n], costs)).unwrap();
        prop_assert!(report.final_equity_micros <= report.initial_cash_micros);
        prop_assert!(report.equity.iter().all(|p| p.cash_micros <= report.initial_cash_micros));
        prop_assert!(report.fills.iter().all(|f| f.at_ms > f.signal_available_ms));
    }

    #[test]
    fn duplicate_policy_operations_do_not_advance_revision(repeats in 1usize..50) {
        let mut state = ControlState::new(config(), 1).unwrap();
        let update = RiskPolicyUpdate {
            expected_revision: 1, request_id: "stable".into(), limits: limits(), reason: "property test".into(),
        };
        for i in 0..repeats {
            state = snapshot(&state);
            prop_assert_eq!(state.update("owner", update.clone(), 2 + i as u64).unwrap().revision, 2);
        }
        prop_assert_eq!(state.audit("owner", 0, 200).unwrap().len(), 2);
    }

    #[test]
    fn stale_or_future_quotes_never_authorize(age in 5001u64..1_000_000) {
        let (mut quote, account, order) = market();
        prop_assert_eq!(assess_crypto_order(&limits(), &quote, &account, &order, 50, 10000 + age), Err(CryptoRiskRejection::StaleQuote));
        quote.observed_ms = 10001;
        prop_assert_eq!(assess_crypto_order(&limits(), &quote, &account, &order, 50, 10000), Err(CryptoRiskRejection::StaleQuote));
    }
}
