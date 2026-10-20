use helio_execution::{checked_notional, OrderProposal, Side};
use serde::{Deserialize, Serialize};

use crate::{mul_div, CryptoError};

/// JSON contract shared with Python's control.models.RiskLimits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CryptoRiskLimits {
    pub capital_micros: u64,
    pub order_micros: u64,
    pub daily_loss_micros: u64,
    pub drawdown_micros: u64,
    pub symbol_exposure_micros: u64,
    pub max_daily_orders: u32,
    pub max_spread_bps: u32,
    pub max_quote_age_ms: u64,
    pub symbols: Vec<String>,
    pub leverage: bool,
}

impl CryptoRiskLimits {
    pub fn validate(&self) -> Result<(), CryptoError> {
        if self.order_micros == 0
            || self.daily_loss_micros == 0
            || self.order_micros > self.symbol_exposure_micros
            || self.symbol_exposure_micros > self.capital_micros
            || self.daily_loss_micros > self.drawdown_micros
            || self.drawdown_micros > self.capital_micros
            || self.capital_micros > 1_000_000_000_000
            || self.max_daily_orders == 0
            || self.max_daily_orders > 1000
            || !(1..=500).contains(&self.max_spread_bps)
            || !(100..=60_000).contains(&self.max_quote_age_ms)
            || self.symbols.is_empty()
            || self.symbols.len() > 2
            || self
                .symbols
                .iter()
                .any(|s| s != "BTC-USD" && s != "ETH-USD")
            || self
                .symbols
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != self.symbols.len()
            || self.leverage
        {
            return Err(CryptoError::InvalidLimits);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CryptoQuote {
    pub symbol: String,
    pub bid_micros: u64,
    pub ask_micros: u64,
    pub observed_ms: u64,
}

/// Account-wide values include open-order reservations. Equity includes unrealized P&L.
/// Daily opening equity and high water mark must survive restarts and cash-flow adjustments.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CryptoAccountSnapshot {
    pub observed_ms: u64,
    pub buying_power_micros: u64,
    pub equity_micros: u64,
    pub day_open_equity_micros: u64,
    pub peak_equity_micros: u64,
    pub gross_exposure_micros: u64,
    pub symbol_exposure_micros: u64,
    pub available_quantity_micros: u64,
    pub daily_order_count: u32,
    pub unresolved_orders: bool,
    pub paused: bool,
    pub loss_stop_latched: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CryptoRiskRejection {
    InvalidPolicy,
    InvalidOrder,
    Paused,
    ReconciliationRequired,
    StaleQuote,
    StaleAccount,
    InvalidQuote,
    SpreadTooWide,
    SymbolNotAllowed,
    OrderLimit,
    DailyOrderLimit,
    LossLimit,
    ExposureLimit,
    InsufficientCash,
    InsufficientHoldings,
    Overflow,
    AccountMismatch,
    PolicyChanged,
    ExpiredIntent,
    StaleMarketRules,
    InvalidMarketRules,
    InstrumentUnavailable,
    BelowMinimum,
    InvalidIncrement,
    ReservationTooSmall,
    StaleSignal,
}

/// Additional spot-crypto checks; an Ok result is NOT a capital authorization.
/// Live submission must also pass the existing independent risk/admission/gateway controls.
pub fn assess_crypto_order(
    limits: &CryptoRiskLimits,
    quote: &CryptoQuote,
    account: &CryptoAccountSnapshot,
    order: &OrderProposal,
    fee_reserve_bps: u32,
    now_ms: u64,
) -> Result<(), CryptoRiskRejection> {
    use CryptoRiskRejection as R;
    limits.validate().map_err(|_| R::InvalidPolicy)?;
    if account.paused {
        return Err(R::Paused);
    }
    if account.unresolved_orders {
        return Err(R::ReconciliationRequired);
    }
    if !limits.symbols.contains(&order.symbol) || quote.symbol != order.symbol {
        return Err(R::SymbolNotAllowed);
    }
    if order.currency != "USD" || fee_reserve_bps > 10_000 {
        return Err(R::InvalidOrder);
    }
    if now_ms
        .checked_sub(quote.observed_ms)
        .is_none_or(|age| age > limits.max_quote_age_ms)
    {
        return Err(R::StaleQuote);
    }
    if now_ms
        .checked_sub(account.observed_ms)
        .is_none_or(|age| age > limits.max_quote_age_ms)
    {
        return Err(R::StaleAccount);
    }
    if quote.bid_micros == 0 || quote.ask_micros < quote.bid_micros {
        return Err(R::InvalidQuote);
    }
    // Conservative denominator: spread / bid.
    if u128::from(quote.ask_micros - quote.bid_micros) * 10_000
        > u128::from(quote.bid_micros) * u128::from(limits.max_spread_bps)
    {
        return Err(R::SpreadTooWide);
    }
    let notional = checked_notional(order.limit_price, order.quantity)
        .map_err(|_| R::InvalidOrder)?
        .0;
    if notional > limits.order_micros {
        return Err(R::OrderLimit);
    }
    // Held-asset sells can reduce risk after loss/order-count limits; no shorting.
    if order.side == Side::Sell {
        if order.quantity.0 > account.available_quantity_micros {
            return Err(R::InsufficientHoldings);
        }
        return Ok(());
    }
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
    // Reserve exposure at the larger of market ask and the submitted buy limit.
    let exposure = mul_div(
        quote.ask_micros.max(order.limit_price.0),
        order.quantity.0,
        1_000_000,
        true,
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
    let fee =
        mul_div(notional, u64::from(fee_reserve_bps), 10_000, true).map_err(|_| R::Overflow)?;
    if notional.checked_add(fee).ok_or(R::Overflow)? > limits.order_micros {
        return Err(R::OrderLimit);
    }
    if notional.checked_add(fee).ok_or(R::Overflow)? > account.buying_power_micros {
        return Err(R::InsufficientCash);
    }
    Ok(())
}
