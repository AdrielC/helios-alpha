//! A deterministic paper account. Golem commits the fill and source cursor together.
//! Reference quotes are not a representation of fills or fees at a real broker.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::{mul_div, ControlStateError, CryptoQuote, CryptoRiskLimits, CryptoSignalValidity};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PaperConfig {
    pub initial_cash_micros: u64,
    pub fee_bps: u32,
    pub slippage_bps: u32,
    pub minimum_order_micros: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PaperStep {
    pub source_id: String,
    pub source_sequence: u64,
    pub expected_cursor: u64,
    pub expected_policy_revision: u64,
    pub event_id: String,
    pub available_ms: u64,
    pub quote: CryptoQuote,
    #[serde(default)]
    pub portfolio_quotes: Vec<CryptoQuote>,
    /// Desired USD inventory value. Private strategy source never enters this module.
    pub target_micros: u64,
    #[serde(default)]
    pub observation_only: bool,
    /// Required to increase inventory. Missing legacy signals are consumed without buying.
    /// Omission preserves the serialized digest of previously committed legacy inputs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal: Option<CryptoSignalValidity>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaperState {
    config: PaperConfig,
    cash_micros: u64,
    positions: BTreeMap<String, u64>,
    quotes: BTreeMap<String, CryptoQuote>,
    source_id: Option<String>,
    cursor: u64,
    last_digest: Option<String>,
    last_receipt: Value,
    last_available: BTreeMap<String, u64>,
    day_utc: u64,
    day_open_equity_micros: u64,
    peak_equity_micros: u64,
    daily_orders: u32,
    day_loss_latched: bool,
    drawdown_latched: bool,
    total_orders: u64,
    fees_micros: u64,
    fills: Vec<Value>,
    #[serde(default)]
    last_time_ms: u64,
    /// Activated once after a compatible upgrade, so historical invocations replay unchanged.
    #[serde(default, skip_serializing_if = "is_false")]
    signal_deadline_enforced: bool,
}

fn is_false(value: &bool) -> bool {
    !value
}

impl PaperConfig {
    pub fn validate(&self, limits: &CryptoRiskLimits) -> Result<(), ControlStateError> {
        if self.initial_cash_micros == 0
            || self.initial_cash_micros > limits.capital_micros
            || self.fee_bps > 1000
            || self.slippage_bps > 1000
            || self.minimum_order_micros == 0
            || self.minimum_order_micros > limits.order_micros
        {
            return Err(ControlStateError::Invalid);
        }
        Ok(())
    }
}

fn arithmetic<T>(value: Result<T, crate::CryptoError>) -> Result<T, ControlStateError> {
    value.map_err(|_| ControlStateError::Invalid)
}

impl PaperState {
    pub fn new(
        config: PaperConfig,
        limits: &CryptoRiskLimits,
        now_ms: u64,
    ) -> Result<Self, ControlStateError> {
        config.validate(limits)?;
        Ok(Self {
            cash_micros: config.initial_cash_micros,
            day_open_equity_micros: config.initial_cash_micros,
            peak_equity_micros: config.initial_cash_micros,
            config,
            positions: BTreeMap::new(),
            quotes: BTreeMap::new(),
            source_id: None,
            cursor: 0,
            last_digest: None,
            last_receipt: json!({}),
            last_available: BTreeMap::new(),
            day_utc: now_ms / 86_400_000,
            daily_orders: 0,
            day_loss_latched: false,
            drawdown_latched: false,
            total_orders: 0,
            fees_micros: 0,
            fills: vec![],
            last_time_ms: now_ms,
            signal_deadline_enforced: false,
        })
    }

    pub fn signal_deadlines_enabled(&self) -> bool {
        self.signal_deadline_enforced
    }

    pub fn enable_signal_deadlines(&mut self) {
        self.signal_deadline_enforced = true;
    }

    pub fn config_matches(&self, config: &PaperConfig) -> bool {
        &self.config == config
    }

    pub fn resize_untraded(
        &mut self,
        expected_micros: u64,
        next_micros: u64,
        limits: &CryptoRiskLimits,
    ) -> Result<bool, ControlStateError> {
        if self.config.initial_cash_micros == next_micros {
            return Ok(false);
        }
        if self.config.initial_cash_micros != expected_micros {
            return Err(ControlStateError::RevisionConflict);
        }
        if self.total_orders != 0
            || self.fees_micros != 0
            || self.positions.values().any(|q| *q != 0)
            || self.cash_micros != expected_micros
            || self.day_loss_latched
            || self.drawdown_latched
        {
            return Err(ControlStateError::Invalid);
        }
        let config = PaperConfig {
            initial_cash_micros: next_micros,
            ..self.config.clone()
        };
        config.validate(limits)?;
        self.config = config;
        self.cash_micros = next_micros;
        self.day_open_equity_micros = next_micros;
        self.peak_equity_micros = next_micros;
        Ok(true)
    }

    pub fn update_costs(
        &mut self,
        expected: &PaperConfig,
        next: PaperConfig,
    ) -> Result<(), ControlStateError> {
        if self.config == next {
            return Ok(());
        }
        if &self.config != expected {
            return Err(ControlStateError::RevisionConflict);
        }
        if next.initial_cash_micros != self.config.initial_cash_micros
            || next.fee_bps > 1000
            || next.slippage_bps > 1000
            || next.minimum_order_micros == 0
            || next.minimum_order_micros > next.initial_cash_micros
        {
            return Err(ControlStateError::Invalid);
        }
        self.config = next;
        Ok(())
    }

    fn liquidation_value(
        &self,
        quote: &CryptoQuote,
        quantity: u64,
    ) -> Result<u64, ControlStateError> {
        let price = arithmetic(mul_div(
            quote.bid_micros,
            10000 - u64::from(self.config.slippage_bps),
            10000,
            false,
        ))?;
        let gross = arithmetic(mul_div(price, quantity, 1_000_000, false))?;
        let fee = arithmetic(mul_div(gross, u64::from(self.config.fee_bps), 10000, true))?;
        Ok(gross.saturating_sub(fee))
    }

    fn equity(&self) -> Result<u64, ControlStateError> {
        self.positions
            .iter()
            .try_fold(self.cash_micros, |equity, (symbol, quantity)| {
                let quote = self.quotes.get(symbol).ok_or(ControlStateError::Invalid)?;
                equity
                    .checked_add(self.liquidation_value(quote, *quantity)?)
                    .ok_or(ControlStateError::Invalid)
            })
    }

    pub fn status(&self) -> Value {
        let mut status = json!({
            "simulation":true, "cursor":self.cursor, "source_id":self.source_id,
            "cash_micros":self.cash_micros, "positions":self.positions,
            "equity_micros":self.equity().ok(), "equity_uses_last_reference_quotes":true,
            "initial_cash_micros":self.config.initial_cash_micros,
            "day_loss_latched":self.day_loss_latched, "drawdown_latched":self.drawdown_latched,
            "day_utc":self.day_utc, "daily_orders":self.daily_orders,
            "total_orders":self.total_orders, "fees_micros":self.fees_micros,
            "cost_assumptions":self.config, "recent_fills":self.fills
        });
        if self.signal_deadline_enforced {
            status["signal_deadline_version"] = json!(1);
        }
        status
    }

    /// Copy-on-success prevents a domain error from partially changing in-memory state.
    pub fn apply(
        &mut self,
        input: &PaperStep,
        limits: &CryptoRiskLimits,
        revision: u64,
        paused: bool,
        now_ms: u64,
    ) -> Result<Value, ControlStateError> {
        let digest = hex::encode(Sha256::digest(
            serde_json::to_vec(input).map_err(|_| ControlStateError::Invalid)?,
        ));
        if self
            .source_id
            .as_ref()
            .is_some_and(|id| id != &input.source_id)
        {
            return Err(ControlStateError::IdentityConflict);
        }
        if input.source_sequence == self.cursor && self.cursor != 0 {
            if self.last_digest.as_deref() != Some(digest.as_str()) {
                return Err(ControlStateError::IdentityConflict);
            }
            return Ok(self.last_receipt.clone());
        }
        if input.source_sequence < self.cursor {
            return Ok(
                json!({"simulation":true,"cursor":self.cursor,"outcome":"already_consumed"}),
            );
        }
        if input.source_sequence == 0
            || input.expected_cursor != self.cursor
            || input.expected_policy_revision != revision
        {
            return Err(ControlStateError::RevisionConflict);
        }
        if input.source_id.is_empty()
            || input.source_id.len() > 160
            || input.event_id.is_empty()
            || input.event_id.len() > 160
            || !limits.symbols.contains(&input.quote.symbol)
            || input.quote.bid_micros == 0
            || input.quote.ask_micros < input.quote.bid_micros
            || input.target_micros > limits.symbol_exposure_micros
            || input.portfolio_quotes.len() > 2
            || input.portfolio_quotes.iter().any(|q| {
                !limits.symbols.contains(&q.symbol)
                    || q.bid_micros == 0
                    || q.ask_micros < q.bid_micros
            })
        {
            return Err(ControlStateError::Invalid);
        }
        let mut next = self.clone();
        let outcome = next.evaluate(input, limits, paused, now_ms)?;
        next.source_id = Some(input.source_id.clone());
        next.cursor = input.source_sequence;
        next.last_digest = Some(digest);
        next.last_time_ms = now_ms;
        next.last_receipt = json!({
            "simulation":true,"cursor":next.cursor,"event_id":input.event_id,
            "outcome":outcome,"total_orders":next.total_orders
        });
        let result = next.last_receipt.clone();
        *self = next;
        Ok(result)
    }

    fn evaluate(
        &mut self,
        input: &PaperStep,
        limits: &CryptoRiskLimits,
        paused: bool,
        now_ms: u64,
    ) -> Result<&'static str, ControlStateError> {
        let quote = &input.quote;
        if now_ms < self.last_time_ms {
            return Err(ControlStateError::ClockRegression);
        }
        if now_ms / 86_400_000 < self.day_utc {
            return Err(ControlStateError::ClockRegression);
        }
        if now_ms
            .checked_sub(input.available_ms)
            .is_none_or(|age| age > 120_000)
            || now_ms
                .checked_sub(quote.observed_ms)
                .is_none_or(|age| age > limits.max_quote_age_ms)
        {
            return Ok("stale_input");
        }
        if self
            .last_available
            .get(&quote.symbol)
            .is_some_and(|at| *at >= input.available_ms)
        {
            return Ok("duplicate_or_out_of_order_observation");
        }
        self.last_available
            .insert(quote.symbol.clone(), input.available_ms);
        for q in &input.portfolio_quotes {
            if now_ms
                .checked_sub(q.observed_ms)
                .is_some_and(|age| age <= limits.max_quote_age_ms)
                && self
                    .quotes
                    .get(&q.symbol)
                    .is_none_or(|old| old.observed_ms <= q.observed_ms)
            {
                self.quotes.insert(q.symbol.clone(), q.clone());
            }
        }
        self.quotes.insert(quote.symbol.clone(), quote.clone());
        // Risk bookkeeping needs fresh valuations even when no order is requested.
        if self.positions.iter().any(|(symbol, quantity)| {
            *quantity > 0
                && self.quotes.get(symbol).is_none_or(|q| {
                    now_ms
                        .checked_sub(q.observed_ms)
                        .is_none_or(|age| age > limits.max_quote_age_ms)
                })
        }) {
            return Ok("other_held_quote_stale");
        }
        let equity = self.equity()?;
        self.peak_equity_micros = self.peak_equity_micros.max(equity);
        if now_ms / 86_400_000 != self.day_utc {
            self.day_utc = now_ms / 86_400_000;
            self.day_open_equity_micros = equity;
            self.daily_orders = 0;
            self.day_loss_latched = false;
        }
        self.day_loss_latched |=
            self.day_open_equity_micros.saturating_sub(equity) >= limits.daily_loss_micros;
        self.drawdown_latched |=
            self.peak_equity_micros.saturating_sub(equity) >= limits.drawdown_micros;
        if input.observation_only {
            return Ok("observation_only");
        }
        if paused {
            return Ok("paused");
        }
        if u128::from(quote.ask_micros - quote.bid_micros) * 10000
            > u128::from(quote.bid_micros) * u128::from(limits.max_spread_bps)
        {
            return Ok("spread_too_wide");
        }
        let quantity = *self.positions.get(&quote.symbol).unwrap_or(&0);
        let held = arithmetic(mul_div(quote.bid_micros, quantity, 1_000_000, false))?;
        let target = if self.day_loss_latched || self.drawdown_latched {
            0
        } else {
            input.target_micros
        };
        let (side, units, price, gross, fee) = if target > held {
            if self.signal_deadline_enforced {
                let Some(signal) = &input.signal else {
                    return Ok("missing_signal_validity");
                };
                if signal.available_ms > input.available_ms || !signal.valid_at(now_ms) {
                    return Ok("stale_signal");
                }
            }
            if self.daily_orders >= limits.max_daily_orders {
                return Ok("daily_order_limit");
            }
            // Sum conservative market inventory values separately from liquidation equity.
            let gross_inventory =
                self.positions
                    .iter()
                    .try_fold(0u64, |total, (symbol, quantity)| {
                        let q = self.quotes.get(symbol).ok_or(ControlStateError::Invalid)?;
                        total
                            .checked_add(arithmetic(mul_div(
                                q.ask_micros,
                                *quantity,
                                1_000_000,
                                true,
                            ))?)
                            .ok_or(ControlStateError::Invalid)
                    })?;
            let price = arithmetic(mul_div(
                quote.ask_micros,
                10000 + u64::from(self.config.slippage_bps),
                10000,
                true,
            ))?;
            let cash_notional = arithmetic(mul_div(
                self.cash_micros,
                10000,
                10000 + u64::from(self.config.fee_bps),
                false,
            ))?;
            let held_ask = arithmetic(mul_div(quote.ask_micros, quantity, 1_000_000, true))?;
            let order_notional = arithmetic(mul_div(
                limits.order_micros,
                10000,
                10000 + u64::from(self.config.fee_bps),
                false,
            ))?;
            let gross_limit = (target - held)
                .min(order_notional)
                .min(cash_notional)
                .min(limits.capital_micros.saturating_sub(gross_inventory))
                .min(limits.symbol_exposure_micros.saturating_sub(held_ask));
            if gross_limit < self.config.minimum_order_micros {
                return Ok("below_minimum_or_no_capacity");
            }
            let units = arithmetic(mul_div(gross_limit, 1_000_000, price, false))?;
            let gross = arithmetic(mul_div(price, units, 1_000_000, true))?;
            let fee = arithmetic(mul_div(gross, u64::from(self.config.fee_bps), 10000, true))?;
            ("buy", units, price, gross, fee)
        } else {
            let gross_limit = (held - target).min(limits.order_micros);
            if gross_limit < self.config.minimum_order_micros {
                return Ok("below_minimum_or_at_target");
            }
            let units =
                arithmetic(mul_div(gross_limit, 1_000_000, quote.bid_micros, false))?.min(quantity);
            let price = arithmetic(mul_div(
                quote.bid_micros,
                10000 - u64::from(self.config.slippage_bps),
                10000,
                false,
            ))?;
            let gross = arithmetic(mul_div(price, units, 1_000_000, false))?;
            let fee = arithmetic(mul_div(gross, u64::from(self.config.fee_bps), 10000, true))?;
            ("sell", units, price, gross, fee)
        };
        if units == 0 {
            return Ok("below_quantity_precision");
        }
        if gross < self.config.minimum_order_micros {
            return Ok("below_minimum_after_rounding");
        }
        if side == "buy" {
            let debit = gross.checked_add(fee).ok_or(ControlStateError::Invalid)?;
            if debit > self.cash_micros || debit > limits.order_micros {
                return Ok("insufficient_cash");
            }
            self.cash_micros -= debit;
            self.positions.insert(
                quote.symbol.clone(),
                quantity
                    .checked_add(units)
                    .ok_or(ControlStateError::Invalid)?,
            );
        } else {
            self.cash_micros = self
                .cash_micros
                .checked_add(gross.saturating_sub(fee))
                .ok_or(ControlStateError::Invalid)?;
            self.positions
                .insert(quote.symbol.clone(), quantity - units);
        }
        self.daily_orders = self
            .daily_orders
            .checked_add(1)
            .ok_or(ControlStateError::Invalid)?;
        self.total_orders = self
            .total_orders
            .checked_add(1)
            .ok_or(ControlStateError::Invalid)?;
        self.fees_micros = self
            .fees_micros
            .checked_add(fee)
            .ok_or(ControlStateError::Invalid)?;
        self.fills.push(json!({
            "simulation":true,"event_id":input.event_id,"symbol":quote.symbol,
            "at_ms":now_ms,"side":side,"quantity_micros":units,"price_micros":price,"fee_micros":fee
        }));
        if self.fills.len() > 200 {
            self.fills.remove(0);
        }
        let equity = self.equity()?;
        self.day_loss_latched |=
            self.day_open_equity_micros.saturating_sub(equity) >= limits.daily_loss_micros;
        self.drawdown_latched |=
            self.peak_equity_micros.saturating_sub(equity) >= limits.drawdown_micros;
        Ok("simulated_fill")
    }

    pub fn validate_snapshot(&self) -> Result<(), ControlStateError> {
        if self.config.initial_cash_micros == 0
            || self.config.fee_bps > 1000
            || self.config.slippage_bps > 1000
            || self.config.minimum_order_micros == 0
            || self.fills.len() > 200
            || self.positions.len() > 2
            || self.quotes.len() > 2
            || self.last_available.len() > 2
            || self
                .positions
                .keys()
                .any(|s| s != "BTC-USD" && s != "ETH-USD")
            || self
                .quotes
                .iter()
                .any(|(s, q)| q.symbol != *s || q.bid_micros == 0 || q.ask_micros < q.bid_micros)
            || self
                .source_id
                .as_ref()
                .is_some_and(|s| s.is_empty() || s.len() > 160)
            || (self.cursor > 0) != self.source_id.is_some()
            || self.last_digest.as_ref().is_some_and(|s| s.len() != 64)
        {
            return Err(ControlStateError::Invalid);
        }
        self.equity()?;
        Ok(())
    }
}
