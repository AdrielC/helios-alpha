use helio_crypto::{
    ceil_crypto_notional_micros as ceil, floor_crypto_notional_micros as floor,
    prepare_exact_crypto_order as prepare, size_exact_crypto_buy, CryptoError, CryptoMarketRules,
    CryptoMarketStatus, CryptoOrderIntent, CryptoOrderSide, CryptoPreparationContext,
    CryptoRiskLimits, CryptoRiskRejection as R, CryptoSignalValidity, ExactCryptoAccountSnapshot,
    ExactCryptoQuote, PolicyRevision,
};
use helio_execution::BrokerDecimal;
use proptest::prelude::*;
use sha2::{Digest, Sha256};

const NOW: u64 = 1_800_000_000_000;

fn d(value: &str) -> BrokerDecimal {
    BrokerDecimal::try_new(value).unwrap()
}
fn scaled(value: u128, places: usize) -> BrokerDecimal {
    let scale = 10_u128.pow(places as u32);
    d(&format!(
        "{}.{:0width$}",
        value / scale,
        value % scale,
        width = places
    ))
}
fn quantity_e8(value: &BrokerDecimal) -> u128 {
    let (whole, fraction) = value
        .as_str()
        .split_once('.')
        .unwrap_or((value.as_str(), ""));
    assert!(fraction.len() <= 8);
    whole.parse::<u128>().unwrap() * 100_000_000
        + if fraction.is_empty() {
            0
        } else {
            fraction.parse::<u128>().unwrap() * 10_u128.pow(8 - fraction.len() as u32)
        }
}
fn policy_hash(context: &mut CryptoPreparationContext) {
    context.policy.sha256 = hex::encode(Sha256::digest(
        serde_json::to_vec(&context.policy.limits).unwrap(),
    ));
}
fn intent() -> CryptoOrderIntent {
    CryptoOrderIntent {
        account_key: "a".repeat(64),
        client_order_id: "11299b2b-61e3-43e7-b9f7-dee77210bb29".into(),
        symbol: "BTC-USD".into(),
        side: CryptoOrderSide::Buy,
        quantity: d("0.00123456"),
        limit_price: d("12345.67"),
        reserved_cash_micros: 16_000_000,
        policy_revision: 3,
        strategy_fingerprint: "2".repeat(64),
        input_digest: "3".repeat(64),
        risk_reservation_id: "4".repeat(64),
        created_ms: NOW - 100,
        expires_ms: NOW + 5000,
    }
}
fn context() -> CryptoPreparationContext {
    let mut context = CryptoPreparationContext {
        policy: PolicyRevision {
            schema_version: 1,
            revision: 3,
            sha256: String::new(),
            updated_ns: (NOW - 1000) * 1_000_000,
            limits: CryptoRiskLimits {
                capital_micros: 500_000_000,
                order_micros: 50_000_000,
                daily_loss_micros: 25_000_000,
                drawdown_micros: 100_000_000,
                symbol_exposure_micros: 200_000_000,
                max_daily_orders: 10,
                max_spread_bps: 20,
                max_quote_age_ms: 1000,
                symbols: vec!["BTC-USD".into(), "ETH-USD".into()],
                leverage: false,
            },
        },
        account: ExactCryptoAccountSnapshot {
            account_key: "a".repeat(64),
            symbol: "BTC-USD".into(),
            snapshot_digest: "9".repeat(64),
            observed_ms: NOW - 50,
            day_utc: NOW / 86_400_000,
            buying_power_micros: 450_000_000,
            equity_micros: 500_000_000,
            day_open_equity_micros: 500_000_000,
            peak_equity_micros: 500_000_000,
            gross_exposure_micros: 50_000_000,
            symbol_exposure_micros: 30_000_000,
            available_quantity: d("0.01"),
            daily_order_count: 0,
            unresolved_orders: false,
            paused: false,
            loss_stop_latched: false,
            fee_reserve_bps: 37,
            settlement_quantum_micros: 1,
        },
        quote: ExactCryptoQuote {
            account_key: "a".repeat(64),
            symbol: "BTC-USD".into(),
            bid: d("12345.60"),
            ask: d("12345.62"),
            observed_ms: NOW - 10,
        },
        rules: CryptoMarketRules {
            symbol: "BTC-USD".into(),
            observed_ms: NOW - 20,
            asset_increment: d("0.00000001"),
            quote_increment: d("0.01"),
            max_order_quantity: d("1000"),
            minimum_order_micros: 1_000_000,
            status: CryptoMarketStatus::Tradable,
            api_tradable: true,
        },
        signal: CryptoSignalValidity {
            input_digest: "3".repeat(64),
            available_ms: NOW - 500,
            expires_ms: NOW + 10_000,
        },
    };
    policy_hash(&mut context);
    context
}

#[test]
fn external_validity_cannot_be_extended_by_restriction_or_generic_revalidation() {
    let context = context();
    let receipt = prepare(intent(), &context, NOW)
        .unwrap()
        .restrict_valid_until(NOW + 10)
        .unwrap()
        .restrict_valid_until(NOW + 500)
        .unwrap();
    assert_eq!(receipt.expires_ms(), NOW + 10);
    assert_eq!(
        receipt.revalidate(&context, NOW + 1).unwrap().expires_ms(),
        NOW + 10
    );
    assert_eq!(
        receipt.revalidate(&context, NOW + 10).unwrap_err(),
        R::ExpiredIntent
    );
    assert!(receipt.restrict_valid_until(NOW).is_err());
}

#[test]
fn notional_handles_scaled_product_overflow_and_rounds_only_at_money_boundary() {
    assert_eq!(ceil(&d("105000.25"), &d("0.004")), Ok(420_001_000));
    assert_eq!(floor(&d("105000.25"), &d("0.004")), Ok(420_001_000));
    assert_eq!(
        ceil(&d("0.000000000000000001"), &d("0.000000000000000001")),
        Ok(1)
    );
    assert_eq!(
        floor(&d("0.000000000000000001"), &d("0.000000000000000001")),
        Ok(0)
    );
    assert_eq!(
        ceil(&d("999999999999999999"), &d("0.000000000000000001")),
        Ok(1_000_000)
    );
    assert_eq!(
        floor(&d("999999999999999999"), &d("0.000000000000000001")),
        Ok(999_999)
    );
    assert_eq!(
        ceil(&d("1.999999999999999999"), &d("1.999999999999999999")),
        Ok(4_000_000)
    );
    assert_eq!(
        floor(&d("1.999999999999999999"), &d("1.999999999999999999")),
        Ok(3_999_999)
    );
    assert_eq!(ceil(&d("18446744073709.551615"), &d("1")), Ok(u64::MAX));
    assert_eq!(
        ceil(&d("18446744073709.551615000000000001"), &d("1")),
        Err(CryptoError::Overflow)
    );
    assert_eq!(
        floor(&d("18446744073709.551615000000000001"), &d("1")),
        Ok(u64::MAX)
    );
    assert_eq!(
        ceil(&d("999999999999999999"), &d("999999999999999999")),
        Err(CryptoError::Overflow)
    );
    assert_eq!(
        ceil(&d("0.0000000000000000001"), &d("1")),
        Err(CryptoError::Overflow)
    );
}

#[test]
fn preparation_keeps_exact_intent_fee_reserve_and_oldest_freshness_deadline() {
    let planned = intent();
    let receipt = prepare(planned.clone(), &context(), NOW).unwrap();
    assert_eq!(receipt.intent(), &planned);
    assert_eq!(receipt.notional_micros(), 15_241_471);
    assert_eq!(receipt.fee_reserve_micros(), 56_394);
    assert_eq!(receipt.cash_reserve_micros(), 16_000_000);
    assert_eq!(receipt.expires_ms(), NOW + 951);
    let value = serde_json::to_value(&receipt).unwrap();
    assert_eq!(value["execution_authority"], false);
    assert_eq!(value["intent"]["quantity"], "0.00123456");
}

#[test]
fn changed_limits_bad_hash_and_wrong_account_cannot_prepare() {
    let mut ctx = context();
    ctx.policy.revision += 1;
    assert_eq!(prepare(intent(), &ctx, NOW).unwrap_err(), R::PolicyChanged);
    ctx = context();
    ctx.policy.limits.order_micros -= 1;
    assert_eq!(prepare(intent(), &ctx, NOW).unwrap_err(), R::InvalidPolicy);
    ctx = context();
    ctx.quote.account_key = "b".repeat(64);
    assert_eq!(
        prepare(intent(), &ctx, NOW).unwrap_err(),
        R::AccountMismatch
    );
    ctx = context();
    ctx.account.account_key = "b".repeat(64);
    assert_eq!(
        prepare(intent(), &ctx, NOW).unwrap_err(),
        R::AccountMismatch
    );
    ctx = context();
    ctx.account.snapshot_digest = "unknown".into();
    assert_eq!(
        prepare(intent(), &ctx, NOW).unwrap_err(),
        R::AccountMismatch
    );
    ctx = context();
    ctx.rules.symbol = "ETH-USD".into();
    assert_eq!(
        prepare(intent(), &ctx, NOW).unwrap_err(),
        R::SymbolNotAllowed
    );
}

#[test]
fn expired_future_and_stale_inputs_require_new_evidence() {
    let planned = intent();
    assert_eq!(
        prepare(planned.clone(), &context(), planned.expires_ms).unwrap_err(),
        R::ExpiredIntent
    );
    let mut future = planned;
    future.created_ms = NOW + 1;
    assert_eq!(
        prepare(future, &context(), NOW).unwrap_err(),
        R::ExpiredIntent
    );
    let mut ctx = context();
    ctx.account.observed_ms = NOW - 1001;
    assert_eq!(prepare(intent(), &ctx, NOW).unwrap_err(), R::StaleAccount);
    ctx = context();
    ctx.quote.observed_ms = NOW + 1;
    assert_eq!(prepare(intent(), &ctx, NOW).unwrap_err(), R::StaleQuote);
    ctx = context();
    ctx.rules.observed_ms = NOW - 1001;
    assert_eq!(
        prepare(intent(), &ctx, NOW).unwrap_err(),
        R::StaleMarketRules
    );
    ctx = context();
    ctx.account.day_utc -= 1;
    assert_eq!(prepare(intent(), &ctx, NOW).unwrap_err(), R::StaleAccount);
}

#[test]
fn new_quotes_cannot_refresh_the_original_signal_deadline() {
    let mut ctx = context();
    ctx.signal.expires_ms = NOW + 200;
    assert_eq!(prepare(intent(), &ctx, NOW).unwrap_err(), R::StaleSignal);
    let mut planned = intent();
    planned.expires_ms = ctx.signal.expires_ms;
    let receipt = prepare(planned, &ctx, NOW).unwrap();
    assert_eq!(receipt.expires_ms(), NOW + 200);
    ctx.quote.observed_ms = NOW + 200;
    ctx.account.observed_ms = NOW + 200;
    ctx.rules.observed_ms = NOW + 200;
    assert_eq!(
        receipt.revalidate(&ctx, NOW + 200).unwrap_err(),
        R::StaleSignal
    );
    ctx.signal.expires_ms = NOW + 5000;
    assert_eq!(
        receipt.revalidate(&ctx, NOW + 100).unwrap_err(),
        R::StaleSignal
    );
    ctx = context();
    ctx.signal.input_digest = "5".repeat(64);
    assert_eq!(prepare(intent(), &ctx, NOW).unwrap_err(), R::StaleSignal);
    ctx = context();
    ctx.signal.expires_ms = ctx.signal.available_ms + 120_001;
    assert_eq!(prepare(intent(), &ctx, NOW).unwrap_err(), R::StaleSignal);
}

#[test]
fn broker_increments_limits_and_true_minimum_are_enforced() {
    let mut planned = intent();
    planned.quantity = d("0.001234567");
    assert_eq!(
        prepare(planned, &context(), NOW).unwrap_err(),
        R::InvalidIncrement
    );
    planned = intent();
    planned.limit_price = d("12345.671");
    assert_eq!(
        prepare(planned, &context(), NOW).unwrap_err(),
        R::InvalidIncrement
    );
    let mut ctx = context();
    ctx.rules.max_order_quantity = d("0.001");
    assert_eq!(prepare(intent(), &ctx, NOW).unwrap_err(), R::OrderLimit);
    ctx = context();
    ctx.rules.asset_increment = d("0");
    assert_eq!(
        prepare(intent(), &ctx, NOW).unwrap_err(),
        R::InvalidMarketRules
    );
    ctx = context();
    ctx.rules.minimum_order_micros = 16_000_000;
    assert_eq!(prepare(intent(), &ctx, NOW).unwrap_err(), R::BelowMinimum);
    ctx = context();
    ctx.rules.api_tradable = false;
    assert_eq!(
        prepare(intent(), &ctx, NOW).unwrap_err(),
        R::InstrumentUnavailable
    );

    // Rounding a cost upward must not make a below-minimum order look big enough.
    planned = intent();
    planned.limit_price = d("0.999999999999999999");
    planned.quantity = d("1");
    planned.reserved_cash_micros = 2_000_000;
    ctx = context();
    ctx.rules.quote_increment = d("0.000000000000000001");
    ctx.quote.bid = planned.limit_price.clone();
    ctx.quote.ask = planned.limit_price.clone();
    assert_eq!(prepare(planned, &ctx, NOW).unwrap_err(), R::BelowMinimum);
}

#[test]
fn spending_limits_include_the_entire_saved_reservation_and_worst_fee() {
    let mut planned = intent();
    planned.reserved_cash_micros = 15_241_471;
    assert_eq!(
        prepare(planned, &context(), NOW).unwrap_err(),
        R::ReservationTooSmall
    );
    planned = intent();
    planned.reserved_cash_micros = 50_000_001;
    assert_eq!(
        prepare(planned, &context(), NOW).unwrap_err(),
        R::OrderLimit
    );
    let mut ctx = context();
    ctx.account.buying_power_micros = 15_999_999;
    assert_eq!(
        prepare(intent(), &ctx, NOW).unwrap_err(),
        R::InsufficientCash
    );
    ctx = context();
    ctx.account.fee_reserve_bps = 10_001;
    assert_eq!(prepare(intent(), &ctx, NOW).unwrap_err(), R::InvalidOrder);
}

#[test]
fn broker_settlement_rounding_is_reserved_before_sizing_and_preparation() {
    let mut ctx = context();
    ctx.account.settlement_quantum_micros = 10_000;
    let receipt = prepare(intent(), &ctx, NOW).unwrap();
    assert_eq!(receipt.notional_micros(), 15_250_000);
    assert_eq!(receipt.fee_reserve_micros(), 60_000);
    let mut planned = intent();
    planned.reserved_cash_micros = 15_297_865;
    assert_eq!(
        prepare(planned, &ctx, NOW).unwrap_err(),
        R::ReservationTooSmall
    );
    let quantity =
        size_exact_crypto_buy(&intent().limit_price, &ctx.rules, 15_297_865, 37, 10_000).unwrap();
    assert!(quantity_e8(&quantity) < quantity_e8(&intent().quantity));
    ctx.account.settlement_quantum_micros = 0;
    assert_eq!(prepare(intent(), &ctx, NOW).unwrap_err(), R::InvalidOrder);
}

#[test]
fn buys_respect_loss_count_and_exposure_while_sells_require_exact_holdings() {
    let mut ctx = context();
    ctx.account.loss_stop_latched = true;
    assert_eq!(prepare(intent(), &ctx, NOW).unwrap_err(), R::LossLimit);
    ctx = context();
    ctx.account.daily_order_count = ctx.policy.limits.max_daily_orders;
    assert_eq!(
        prepare(intent(), &ctx, NOW).unwrap_err(),
        R::DailyOrderLimit
    );
    ctx = context();
    ctx.account.gross_exposure_micros = 490_000_000;
    assert_eq!(prepare(intent(), &ctx, NOW).unwrap_err(), R::ExposureLimit);
    ctx = context();
    ctx.account.gross_exposure_micros = 300_000_000;
    ctx.account.symbol_exposure_micros = 195_000_000;
    assert_eq!(prepare(intent(), &ctx, NOW).unwrap_err(), R::ExposureLimit);
    let mut planned = intent();
    planned.side = CryptoOrderSide::Sell;
    planned.reserved_cash_micros = 0;
    ctx.account.loss_stop_latched = true;
    ctx.account.daily_order_count = 100;
    ctx.rules.status = CryptoMarketStatus::SellOnly;
    assert_eq!(
        prepare(planned.clone(), &ctx, NOW)
            .unwrap()
            .cash_reserve_micros(),
        0
    );
    ctx.account.available_quantity = d("0.00123455");
    assert_eq!(
        prepare(planned, &ctx, NOW).unwrap_err(),
        R::InsufficientHoldings
    );
}

#[test]
fn cached_preparation_cannot_bypass_pause_new_fees_staleness_or_policy_changes() {
    let ctx = context();
    let receipt = prepare(intent(), &ctx, NOW).unwrap();
    let mut changed = ctx.clone();
    changed.account.paused = true;
    assert_eq!(receipt.revalidate(&changed, NOW).unwrap_err(), R::Paused);
    changed = ctx.clone();
    changed.account.unresolved_orders = true;
    assert_eq!(
        receipt.revalidate(&changed, NOW).unwrap_err(),
        R::ReconciliationRequired
    );
    changed = ctx.clone();
    changed.account.fee_reserve_bps = 1000;
    assert_eq!(
        receipt.revalidate(&changed, NOW).unwrap_err(),
        R::ReservationTooSmall
    );
    assert_eq!(
        receipt.revalidate(&ctx, NOW + 2000).unwrap_err(),
        R::StaleQuote
    );
    changed = ctx.clone();
    changed.policy.revision += 1;
    assert_eq!(
        receipt.revalidate(&changed, NOW).unwrap_err(),
        R::PolicyChanged
    );
    changed = ctx.clone();
    changed.policy.limits.order_micros -= 1;
    policy_hash(&mut changed);
    assert_eq!(
        receipt.revalidate(&changed, NOW).unwrap_err(),
        R::PolicyChanged
    );
    changed = ctx;
    changed.quote.observed_ms = NOW + 2000;
    changed.account.observed_ms = NOW + 2000;
    changed.rules.observed_ms = NOW + 2000;
    let renewed = receipt.revalidate(&changed, NOW + 2000).unwrap();
    assert_eq!(renewed.intent_digest(), receipt.intent_digest());
    assert_eq!(renewed.expires_ms(), NOW + 3001);
}

#[test]
fn large_exact_quotes_do_not_overflow_spread_comparison() {
    let mut planned = intent();
    planned.quantity = d("0.000000000000000001");
    planned.limit_price = d("100000000000000000");
    planned.reserved_cash_micros = 1_000_000;
    let mut ctx = context();
    ctx.rules.asset_increment = planned.quantity.clone();
    ctx.rules.minimum_order_micros = 0;
    ctx.quote.bid = planned.limit_price.clone();
    ctx.quote.ask = d("100100000000000000");
    assert!(prepare(planned.clone(), &ctx, NOW).is_ok());
    ctx.quote.ask = d("100300000000000000");
    assert_eq!(prepare(planned, &ctx, NOW).unwrap_err(), R::SpreadTooWide);
}

proptest! {
    #[test]
    fn notional_matches_independent_integer_oracle(
        price_micros in 1_u128..1_000_000_000_000,
        quantity_e18 in 1_u128..1_000_000_000_000_000_000_000_000,
    ) {
        let price = scaled(price_micros, 6);
        let quantity = scaled(quantity_e18, 18);
        let product = price_micros * quantity_e18; // This oracle's domain fits in u128.
        let denominator = 1_000_000_000_000_000_000_u128;
        prop_assert_eq!(floor(&price, &quantity).unwrap(), (product / denominator) as u64);
        prop_assert_eq!(ceil(&price, &quantity).unwrap(), product.div_ceil(denominator) as u64);
        prop_assert_eq!(ceil(&price, &quantity), ceil(&quantity, &price));
    }

    #[test]
    fn all_eighteen_fractional_digits_contribute_to_notional(
        price_e18 in 1_u128..100_000_000_000_000_000_000_000_000,
        quantity_e18 in 1_u128..1_000_000_000_000,
    ) {
        let product = price_e18 * quantity_e18;
        let denominator = 1_000_000_000_000_000_000_000_000_000_000_u128;
        let price = scaled(price_e18, 18); let quantity = scaled(quantity_e18, 18);
        prop_assert_eq!(floor(&price, &quantity).unwrap(), (product / denominator) as u64);
        prop_assert_eq!(ceil(&price, &quantity).unwrap(), product.div_ceil(denominator) as u64);
    }

    #[test]
    fn sized_buy_is_maximal_without_exceeding_fee_inclusive_budget(
        price_cents in 1_u128..100_000_000,
        budget in 1_u64..100_000_000,
        fee_bps in 0_u32..10_001,
        quantum in 1_u64..10_001,
    ) {
        let mut rules = context().rules; rules.minimum_order_micros = 0;
        let price = scaled(price_cents, 2);
        let sized = size_exact_crypto_buy(&price, &rules, budget, fee_bps, quantum);
        let debit = |q: u128| {
            let quantum = u128::from(quantum);
            let notional = (q * price_cents).div_ceil(10_000).div_ceil(quantum) * quantum;
            let fee = (notional * u128::from(fee_bps)).div_ceil(10_000)
                .div_ceil(quantum) * quantum;
            notional + fee
        };
        match sized {
            Ok(quantity) => {
                let q = quantity_e8(&quantity);
                prop_assert!(debit(q) <= u128::from(budget));
                prop_assert!(q == 1000 * 100_000_000 || debit(q + 1) > u128::from(budget));
                let larger = size_exact_crypto_buy(&price, &rules, budget + 1, fee_bps, quantum).unwrap();
                prop_assert!(quantity_e8(&larger) >= q);
            }
            Err(error) => {
                prop_assert_eq!(error, R::BelowMinimum);
                prop_assert!(debit(1) > u128::from(budget));
            }
        }
    }

    #[test]
    fn sizing_and_preparation_never_omit_fees_from_a_successful_buy(
        price_cents in 1_u128..10_000_000,
        budget in 1_000_000_u64..50_000_001,
        fee_bps in 0_u32..1001,
        quantum in 1_u64..10_001,
    ) {
        let mut ctx = context();
        let price = scaled(price_cents, 2);
        ctx.quote.bid = price.clone(); ctx.quote.ask = price.clone();
        ctx.account.fee_reserve_bps = fee_bps;
        ctx.account.settlement_quantum_micros = quantum;
        if let Ok(quantity) = size_exact_crypto_buy(&price, &ctx.rules, budget, fee_bps, quantum) {
            let mut planned = intent(); planned.quantity = quantity; planned.limit_price = price;
            planned.reserved_cash_micros = budget;
            let prepared = prepare(planned, &ctx, NOW).unwrap();
            prop_assert!(prepared.notional_micros() + prepared.fee_reserve_micros() <= budget);
            prop_assert!(prepared.cash_reserve_micros() <= ctx.policy.limits.order_micros);
        }
    }
}
