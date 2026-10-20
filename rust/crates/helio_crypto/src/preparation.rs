//! Exact-quantity spot order preparation. Successful preparation is not execution authority.

use helio_execution::BrokerDecimal;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::broker_journal::is_digest;
use crate::exact::{decimal_from_units, decimal_units};
use crate::{
    ceil_crypto_notional_micros, floor_crypto_notional_micros, mul_div, CryptoOrderIntent,
    CryptoOrderSide, CryptoRiskRejection as R, PolicyRevision,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExactCryptoQuote {
    pub account_key: String,
    pub symbol: String,
    pub bid: BrokerDecimal,
    pub ask: BrokerDecimal,
    pub observed_ms: u64,
}

/// Values belong to this strategy's allocation, with broker/local reservations accounted for.
/// The trusted account reconciler must supply them; this struct does not authenticate a broker.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExactCryptoAccountSnapshot {
    pub account_key: String,
    pub symbol: String,
    pub snapshot_digest: String,
    pub observed_ms: u64,
    pub day_utc: u64,
    pub buying_power_micros: u64,
    pub equity_micros: u64,
    pub day_open_equity_micros: u64,
    pub peak_equity_micros: u64,
    pub gross_exposure_micros: u64,
    pub symbol_exposure_micros: u64,
    pub available_quantity: BrokerDecimal,
    pub daily_order_count: u32,
    pub unresolved_orders: bool,
    pub paused: bool,
    pub loss_stop_latched: bool,
    /// Worst applicable execution fee, not an assumed maker discount.
    pub fee_reserve_bps: u32,
    /// Conservative USD rounding quantum for both payable notional and fee.
    /// This must come from the broker contract, not its asset/price increment.
    pub settlement_quantum_micros: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CryptoMarketStatus {
    Tradable,
    SellOnly,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CryptoMarketRules {
    pub symbol: String,
    pub observed_ms: u64,
    pub asset_increment: BrokerDecimal,
    pub quote_increment: BrokerDecimal,
    pub max_order_quantity: BrokerDecimal,
    /// Broker minimum converted conservatively to whole USD micros.
    pub minimum_order_micros: u64,
    pub status: CryptoMarketStatus,
    pub api_tradable: bool,
}

impl CryptoMarketRules {
    fn increments(&self) -> Result<(u128, u128, u128), R> {
        let quantity = decimal_units(&self.asset_increment).map_err(|_| R::InvalidMarketRules)?;
        let price = decimal_units(&self.quote_increment).map_err(|_| R::InvalidMarketRules)?;
        let maximum = decimal_units(&self.max_order_quantity).map_err(|_| R::InvalidMarketRules)?;
        if quantity == 0 || price == 0 || maximum < quantity {
            return Err(R::InvalidMarketRules);
        }
        Ok((quantity, price, maximum))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CryptoSignalValidity {
    pub input_digest: String,
    /// Original immutable decision input availability and deadline, not the newest quote time.
    pub available_ms: u64,
    pub expires_ms: u64,
}

impl CryptoSignalValidity {
    pub(crate) fn valid_at(&self, now_ms: u64) -> bool {
        is_digest(&self.input_digest)
            && self.available_ms <= now_ms
            && now_ms < self.expires_ms
            && self
                .expires_ms
                .checked_sub(self.available_ms)
                .is_some_and(|lifetime| lifetime > 0 && lifetime <= 120_000)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CryptoPreparationContext {
    pub policy: PolicyRevision,
    pub account: ExactCryptoAccountSnapshot,
    pub quote: ExactCryptoQuote,
    pub rules: CryptoMarketRules,
    pub signal: CryptoSignalValidity,
}

/// Immutable preparation receipt, never a capital authorization or a dispatch permit.
/// Its deadline covers all input freshness limits. A dispatcher must recheck current
/// policy, pause state, account reservations, and independent capital admission.
#[derive(Debug, Clone, Serialize)]
pub struct PreparedCryptoOrder {
    intent: CryptoOrderIntent,
    intent_digest: String,
    context_digest: String,
    policy_digest: String,
    signal: CryptoSignalValidity,
    prepared_ms: u64,
    expires_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    external_valid_until_ms: Option<u64>,
    notional_micros: u64,
    fee_reserve_micros: u64,
    cash_reserve_micros: u64,
    exposure_reserve_micros: u64,
    execution_authority: bool,
}

impl PreparedCryptoOrder {
    pub fn intent(&self) -> &CryptoOrderIntent {
        &self.intent
    }
    pub fn intent_digest(&self) -> &str {
        &self.intent_digest
    }
    pub const fn expires_ms(&self) -> u64 {
        self.expires_ms
    }
    pub const fn notional_micros(&self) -> u64 {
        self.notional_micros
    }
    pub const fn fee_reserve_micros(&self) -> u64 {
        self.fee_reserve_micros
    }
    pub const fn cash_reserve_micros(&self) -> u64 {
        self.cash_reserve_micros
    }

    /// An adapter may impose a shorter validity window for the data it verified.
    /// This never extends the existing receipt or changes the immutable order intent.
    pub fn restrict_valid_until(mut self, valid_until_ms: u64) -> Result<Self, R> {
        if valid_until_ms <= self.prepared_ms {
            return Err(R::ExpiredIntent);
        }
        self.expires_ms = self.expires_ms.min(valid_until_ms);
        self.external_valid_until_ms = Some(
            self.external_valid_until_ms
                .map_or(valid_until_ms, |old| old.min(valid_until_ms)),
        );
        Ok(self)
    }

    /// A fresh context is required; a cached success is not renewed by changing its timestamp.
    pub fn revalidate(&self, context: &CryptoPreparationContext, now_ms: u64) -> Result<Self, R> {
        if context.policy.revision != self.intent.policy_revision
            || context.policy.sha256 != self.policy_digest
        {
            return Err(R::PolicyChanged);
        }
        if context.signal != self.signal {
            return Err(R::StaleSignal);
        }
        let fresh = prepare_exact_crypto_order(self.intent.clone(), context, now_ms)?;
        match self.external_valid_until_ms {
            Some(deadline) => fresh.restrict_valid_until(deadline),
            None => Ok(fresh),
        }
    }
}

fn digest(value: &impl Serialize) -> Result<String, R> {
    serde_json::to_vec(value)
        .map(|bytes| hex::encode(Sha256::digest(bytes)))
        .map_err(|_| R::InvalidOrder)
}

fn fresh_until(observed_ms: u64, max_age: u64, now_ms: u64, failure: R) -> Result<u64, R> {
    if now_ms
        .checked_sub(observed_ms)
        .is_none_or(|age| age > max_age)
    {
        return Err(failure);
    }
    observed_ms
        .checked_add(max_age)
        .and_then(|v| v.checked_add(1))
        .ok_or(R::Overflow)
}

fn debit(notional: u64, fee_bps: u32, quantum: u64) -> Result<(u64, u64), R> {
    if fee_bps > 10_000 || quantum == 0 {
        return Err(R::InvalidOrder);
    }
    let round = |amount: u64| {
        amount
            .div_ceil(quantum)
            .checked_mul(quantum)
            .ok_or(R::Overflow)
    };
    let notional = round(notional)?;
    let fee = round(mul_div(notional, u64::from(fee_bps), 10_000, true).map_err(|_| R::Overflow)?)?;
    Ok((notional.checked_add(fee).ok_or(R::Overflow)?, fee))
}

/// Largest increment-aligned buy quantity whose conservative debit fits the given budget.
/// Pure sizing only: account freshness, limits and authorization are checked separately.
pub fn size_exact_crypto_buy(
    price: &BrokerDecimal,
    rules: &CryptoMarketRules,
    budget_micros: u64,
    fee_bps: u32,
    settlement_quantum_micros: u64,
) -> Result<BrokerDecimal, R> {
    let (increment, price_increment, maximum) = rules.increments()?;
    let price_units = decimal_units(price).map_err(|_| R::InvalidOrder)?;
    if price_units == 0
        || !price_units.is_multiple_of(price_increment)
        || fee_bps > 10_000
        || settlement_quantum_micros == 0
    {
        return Err(R::InvalidOrder);
    }
    if !rules.api_tradable || rules.status != CryptoMarketStatus::Tradable {
        return Err(R::InstrumentUnavailable);
    }
    let mut lower = 0_u128;
    let mut upper = maximum / increment;
    // At most 120 iterations over the bounded 18+18 digit input domain.
    while lower < upper {
        let midpoint = lower + (upper - lower).div_ceil(2);
        let quantity = decimal_from_units(midpoint * increment).map_err(|_| R::Overflow)?;
        let affordable = ceil_crypto_notional_micros(price, &quantity)
            .ok()
            .and_then(|notional| debit(notional, fee_bps, settlement_quantum_micros).ok())
            .is_some_and(|(cost, _)| cost <= budget_micros);
        if affordable {
            lower = midpoint;
        } else {
            upper = midpoint - 1;
        }
    }
    if lower == 0 {
        return Err(R::BelowMinimum);
    }
    let quantity = decimal_from_units(lower * increment).map_err(|_| R::Overflow)?;
    if floor_crypto_notional_micros(price, &quantity).map_err(|_| R::Overflow)?
        < rules.minimum_order_micros
    {
        return Err(R::BelowMinimum);
    }
    Ok(quantity)
}

pub fn prepare_exact_crypto_order(
    intent: CryptoOrderIntent,
    context: &CryptoPreparationContext,
    now_ms: u64,
) -> Result<PreparedCryptoOrder, R> {
    let CryptoPreparationContext {
        policy,
        account,
        quote,
        rules,
        signal,
    } = context;
    let limits = &policy.limits;
    limits.validate().map_err(|_| R::InvalidPolicy)?;
    if policy.schema_version != 1
        || policy.revision == 0
        || policy.updated_ns / 1_000_000 > now_ms
        || digest(limits)? != policy.sha256
    {
        return Err(R::InvalidPolicy);
    }
    if intent.policy_revision != policy.revision {
        return Err(R::PolicyChanged);
    }
    if intent.account_key != account.account_key
        || quote.account_key != account.account_key
        || !is_digest(&account.snapshot_digest)
    {
        return Err(R::AccountMismatch);
    }
    intent
        .validate_for_account(&account.account_key)
        .map_err(|_| R::InvalidOrder)?;
    if !signal.valid_at(now_ms)
        || signal.input_digest != intent.input_digest
        || signal.available_ms > intent.created_ms
        || intent.expires_ms > signal.expires_ms
    {
        return Err(R::StaleSignal);
    }
    if !limits.symbols.contains(&intent.symbol)
        || quote.symbol != intent.symbol
        || account.symbol != intent.symbol
        || rules.symbol != intent.symbol
    {
        return Err(R::SymbolNotAllowed);
    }
    if intent.created_ms > now_ms || now_ms >= intent.expires_ms {
        return Err(R::ExpiredIntent);
    }
    if account.paused {
        return Err(R::Paused);
    }
    if account.unresolved_orders {
        return Err(R::ReconciliationRequired);
    }
    if account.day_utc != now_ms / 86_400_000 {
        return Err(R::StaleAccount);
    }
    let expires_ms = intent
        .expires_ms
        .min(signal.expires_ms)
        .min(fresh_until(
            quote.observed_ms,
            limits.max_quote_age_ms,
            now_ms,
            R::StaleQuote,
        )?)
        .min(fresh_until(
            account.observed_ms,
            limits.max_quote_age_ms,
            now_ms,
            R::StaleAccount,
        )?)
        .min(fresh_until(
            rules.observed_ms,
            limits.max_quote_age_ms,
            now_ms,
            R::StaleMarketRules,
        )?);
    let (increment, price_increment, maximum) = rules.increments()?;
    let quantity = decimal_units(&intent.quantity).map_err(|_| R::InvalidOrder)?;
    let price = decimal_units(&intent.limit_price).map_err(|_| R::InvalidOrder)?;
    if !quantity.is_multiple_of(increment) || !price.is_multiple_of(price_increment) {
        return Err(R::InvalidIncrement);
    }
    if quantity > maximum {
        return Err(R::OrderLimit);
    }
    if !rules.api_tradable
        || rules.status == CryptoMarketStatus::Unavailable
        || (rules.status == CryptoMarketStatus::SellOnly && intent.side == CryptoOrderSide::Buy)
    {
        return Err(R::InstrumentUnavailable);
    }
    let bid = decimal_units(&quote.bid).map_err(|_| R::InvalidQuote)?;
    let ask = decimal_units(&quote.ask).map_err(|_| R::InvalidQuote)?;
    if bid == 0 || ask < bid {
        return Err(R::InvalidQuote);
    }
    // floor(bid * bps / 10_000), without overflowing on large scaled-18 prices.
    let bps = u128::from(limits.max_spread_bps);
    let allowed_spread = (bid / 10_000) * bps + (bid % 10_000) * bps / 10_000;
    if ask - bid > allowed_spread {
        return Err(R::SpreadTooWide);
    }
    if account.symbol_exposure_micros > account.gross_exposure_micros
        || account.peak_equity_micros < account.equity_micros
    {
        return Err(R::InvalidOrder);
    }
    let notional = ceil_crypto_notional_micros(&intent.limit_price, &intent.quantity)
        .map_err(|_| R::Overflow)?;
    let floor = floor_crypto_notional_micros(&intent.limit_price, &intent.quantity)
        .map_err(|_| R::Overflow)?;
    if floor < rules.minimum_order_micros {
        return Err(R::BelowMinimum);
    }
    if notional > limits.order_micros {
        return Err(R::OrderLimit);
    }
    let (required_cash, fee) = debit(
        notional,
        account.fee_reserve_bps,
        account.settlement_quantum_micros,
    )?;
    let payable_notional = required_cash - fee;
    if payable_notional > limits.order_micros {
        return Err(R::OrderLimit);
    }
    let mut exposure = 0;
    if intent.side == CryptoOrderSide::Sell {
        if quantity > decimal_units(&account.available_quantity).map_err(|_| R::InvalidOrder)? {
            return Err(R::InsufficientHoldings);
        }
    } else {
        if account.daily_order_count >= limits.max_daily_orders {
            return Err(R::DailyOrderLimit);
        }
        if account.loss_stop_latched
            || account
                .day_open_equity_micros
                .saturating_sub(account.equity_micros)
                >= limits.daily_loss_micros
            || account
                .peak_equity_micros
                .saturating_sub(account.equity_micros)
                >= limits.drawdown_micros
        {
            return Err(R::LossLimit);
        }
        if intent.reserved_cash_micros < required_cash {
            return Err(R::ReservationTooSmall);
        }
        if intent.reserved_cash_micros > limits.order_micros {
            return Err(R::OrderLimit);
        }
        if intent.reserved_cash_micros > account.buying_power_micros {
            return Err(R::InsufficientCash);
        }
        exposure = ceil_crypto_notional_micros(
            if ask > price {
                &quote.ask
            } else {
                &intent.limit_price
            },
            &intent.quantity,
        )
        .map_err(|_| R::Overflow)?;
        if account
            .gross_exposure_micros
            .checked_add(exposure)
            .ok_or(R::Overflow)?
            > limits.capital_micros
            || account
                .symbol_exposure_micros
                .checked_add(exposure)
                .ok_or(R::Overflow)?
                > limits.symbol_exposure_micros
        {
            return Err(R::ExposureLimit);
        }
    }
    Ok(PreparedCryptoOrder {
        intent_digest: digest(&intent)?,
        context_digest: digest(context)?,
        policy_digest: policy.sha256.clone(),
        signal: signal.clone(),
        prepared_ms: now_ms,
        expires_ms,
        external_valid_until_ms: None,
        notional_micros: payable_notional,
        fee_reserve_micros: fee,
        cash_reserve_micros: intent.reserved_cash_micros,
        exposure_reserve_micros: exposure,
        execution_authority: false,
        intent,
    })
}
