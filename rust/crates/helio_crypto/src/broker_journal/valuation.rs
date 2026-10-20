//! Durable allocation valuation and loss history. Inputs are authenticated by the host;
//! broker availability and execution admission remain independent checks.

use super::*;
use crate::{
    ceil_crypto_notional_micros as ceil, floor_crypto_notional_micros as floor,
    ExactCryptoAccountSnapshot, ExactCryptoQuote, PolicyRevision,
};

const DAY_MS: u64 = 86_400_000;
const MAX_AGE_MS: u64 = 5_000;

/// Quantity-specific liquidation/cover quote for the entire owned or deficient position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CryptoAllocationQuote {
    pub quote: ExactCryptoQuote,
    pub quantity: BrokerDecimal,
    pub source_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CryptoAllocationValuationInput {
    pub schema_version: u32,
    pub account_key: String,
    pub expected_balance_digest: String,
    pub policy: PolicyRevision,
    pub observed_ms: u64,
    pub source_digest: String,
    /// Independently verified worst execution fee, not a quoted maker discount.
    pub fee_reserve_bps: u32,
    pub paused: bool,
    pub quotes: Vec<CryptoAllocationQuote>,
}

// Deserialize historical bases only inside validated journal snapshots. The public balance
// type deliberately remains Serialize-only.
#[derive(Serialize, Deserialize)]
#[serde(remote = "CryptoAllocationBalance", deny_unknown_fields)]
struct StoredBalance {
    execution_authority: bool,
    account_key: String,
    ledger_digest: String,
    observed_ms: u64,
    cash_micros: u64,
    cash_deficit_micros: u64,
    quantities: BTreeMap<String, BrokerDecimal>,
    quantity_deficits: BTreeMap<String, BrokerDecimal>,
    available_cash_micros: u64,
    available_quantities: BTreeMap<String, BrokerDecimal>,
    accounted_orders: usize,
    unresolved_orders: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Valuation {
    equity_micros: u64,
    equity_deficit_micros: u64,
    gross_exposure_micros: u64,
    symbol_exposure_micros: BTreeMap<String, u64>,
    valid_until_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RiskHistory {
    input: CryptoAllocationValuationInput,
    #[serde(with = "StoredBalance")]
    basis: CryptoAllocationBalance,
    orders_digest: String,
    valuation: Valuation,
    committed_ms: u64,
    day_utc: u64,
    day_open_equity_micros: u64,
    peak_equity_micros: u64,
    daily_order_count: u32,
    day_loss_latched: bool,
    drawdown_latched: bool,
    deficit_latched: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    prepared_order: Option<CryptoPreparedOrderScope>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CryptoAllocationRiskView {
    pub current: bool,
    pub valuation_digest: String,
    pub balance_digest: String,
    pub source_digest: String,
    pub observed_ms: u64,
    pub valid_until_ms: u64,
    pub policy_revision: u64,
    pub policy_digest: String,
    pub day_utc: u64,
    pub equity_micros: u64,
    pub equity_deficit_micros: u64,
    pub day_open_equity_micros: u64,
    pub peak_equity_micros: u64,
    pub gross_exposure_micros: u64,
    pub symbol_exposure_micros: BTreeMap<String, u64>,
    pub daily_order_count: u32,
    pub day_loss_latched: bool,
    pub drawdown_latched: bool,
    pub deficit_latched: bool,
    pub paused: bool,
    pub fee_reserve_bps: u32,
    pub settlement_quantum_micros: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CryptoAllocationRiskState {
    pub execution_authority: bool,
    pub account_key: String,
    pub evaluated_ms: u64,
    pub balance: Option<CryptoAllocationBalance>,
    pub balance_digest: Option<String>,
    /// None means history has never been initialized; it does not mean zero losses.
    pub risk: Option<CryptoAllocationRiskView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CryptoAllocationValuationReceipt {
    pub changed: bool,
    pub execution_authority: bool,
    pub risk: CryptoAllocationRiskView,
}

/// A trusted caller must bind this deadline and policy to order preparation, then separately
/// recheck current controller pause/policy, broker spending caps, and execution admission.
#[derive(Debug, Clone, Serialize)]
pub struct CryptoValuedAccount {
    pub execution_authority: bool,
    pub account: ExactCryptoAccountSnapshot,
    pub policy: PolicyRevision,
    pub valuation_digest: String,
    pub balance_digest: String,
    pub valid_until_ms: u64,
    /// When present, the balance includes only this provably unsent order's own reservation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prepared_order: Option<CryptoPreparedOrderScope>,
}

fn invalid<T>() -> Result<T> {
    Err(CryptoJournalError::InvalidValuation)
}

fn number(value: u128) -> Result<u64> {
    value.try_into().map_err(|_| CryptoJournalError::Capacity)
}

fn add(total: &mut u128, value: u128) -> Result<()> {
    *total = total
        .checked_add(value)
        .ok_or(CryptoJournalError::Capacity)?;
    Ok(())
}

fn validate_basis(basis: &CryptoAllocationBalance, account: &str) -> Result<()> {
    if basis.execution_authority
        || basis.account_key != account
        || !is_digest(&basis.ledger_digest)
        || basis.unresolved_orders
        || basis.accounted_orders > MAX_ORDERS
        || basis.available_cash_micros != basis.cash_micros
        || basis.available_quantities != basis.quantities
        || (basis.cash_micros > 0 && basis.cash_deficit_micros > 0)
        || basis.quantities.len() + basis.quantity_deficits.len() > 2
    {
        return invalid();
    }
    for (symbol, quantity) in basis.quantities.iter().chain(&basis.quantity_deficits) {
        if !matches!(symbol.as_str(), "BTC-USD" | "ETH-USD")
            || units(quantity)? == 0
            || (basis.quantities.contains_key(symbol)
                && basis.quantity_deficits.contains_key(symbol))
        {
            return invalid();
        }
    }
    Ok(())
}

fn calculate(
    basis: &CryptoAllocationBalance,
    input: &CryptoAllocationValuationInput,
    quantum: u64,
    now_ms: u64,
) -> Result<Valuation> {
    validate_basis(basis, &input.account_key)?;
    let policy = &input.policy;
    policy
        .limits
        .validate()
        .map_err(|_| CryptoJournalError::InvalidValuation)?;
    if input.schema_version != 1
        || !is_digest(&input.account_key)
        || !is_digest(&input.source_digest)
        || input.expected_balance_digest != digest(basis)?
        || policy.schema_version != 1
        || policy.revision == 0
        || policy.sha256 != digest(&policy.limits)?
        || policy.updated_ns / 1_000_000 > now_ms
        || input.observed_ms < basis.observed_ms
        || input.observed_ms > now_ms
        || input.fee_reserve_bps > 10_000
        || quantum == 0
        || input.quotes.len() != basis.quantities.len() + basis.quantity_deficits.len()
        || input.quotes.len() > 2
    {
        return invalid();
    }
    let mut oldest_ms = input.observed_ms;
    let mut credits = u128::from(basis.cash_micros);
    let mut debits = u128::from(basis.cash_deficit_micros);
    let mut exposures = BTreeMap::new();
    let mut gross = 0_u128;
    for item in &input.quotes {
        let q = &item.quote;
        let deficit = basis.quantity_deficits.contains_key(&q.symbol);
        let quantity = basis
            .quantities
            .get(&q.symbol)
            .or_else(|| basis.quantity_deficits.get(&q.symbol))
            .ok_or(CryptoJournalError::InvalidValuation)?;
        if q.account_key != input.account_key
            || !is_digest(&item.source_digest)
            || q.observed_ms > now_ms
            || units(&q.bid)? == 0
            || units(&q.ask)? < units(&q.bid)?
            || units(&item.quantity)? != units(quantity)?
            || exposures.contains_key(&q.symbol)
        {
            return invalid();
        }
        oldest_ms = oldest_ms.min(q.observed_ms);
        let ask = ceil(&q.ask, quantity).map_err(|_| CryptoJournalError::Capacity)?;
        let payable = if deficit {
            ask
        } else {
            ceil(&q.bid, quantity).map_err(|_| CryptoJournalError::Capacity)?
        };
        let fee = (u128::from(payable) * u128::from(input.fee_reserve_bps)).div_ceil(10_000);
        add(&mut debits, accounting::rounded(fee, quantum, true)?)?;
        if deficit {
            add(
                &mut debits,
                accounting::rounded(u128::from(ask), quantum, true)?,
            )?;
        } else {
            let credit = floor(&q.bid, quantity).map_err(|_| CryptoJournalError::Capacity)?;
            add(
                &mut credits,
                accounting::rounded(u128::from(credit), quantum, false)?,
            )?;
        }
        // Use the ask value for exposure caps even when liquidation receives only the bid.
        exposures.insert(q.symbol.clone(), ask);
        add(&mut gross, u128::from(ask))?;
    }
    let valid_until_ms = oldest_ms
        .checked_add(policy.limits.max_quote_age_ms.min(MAX_AGE_MS))
        .ok_or(CryptoJournalError::Capacity)?;
    if now_ms >= valid_until_ms {
        return invalid();
    }
    Ok(Valuation {
        equity_micros: number(credits.saturating_sub(debits))?,
        equity_deficit_micros: number(debits.saturating_sub(credits))?,
        gross_exposure_micros: number(gross)?,
        symbol_exposure_micros: exposures,
        valid_until_ms,
    })
}

impl CryptoBrokerJournal {
    fn buy_dispatches(&self, day: u64) -> u32 {
        self.records()
            .filter(|r| {
                r.intent.side == CryptoOrderSide::Buy
                    && r.dispatch_started_ms.is_some_and(|at| at / DAY_MS == day)
            })
            .count() as u32 // Journal capacity is 256; attempts are never refunded or evicted.
    }

    fn risk_view(
        &self,
        now_ms: u64,
        balance_digest: &str,
    ) -> Result<Option<CryptoAllocationRiskView>> {
        let Some(h) = &self.state.risk_history else {
            return Ok(None);
        };
        let quantum = self
            .state
            .accounting
            .as_ref()
            .ok_or(CryptoJournalError::InvalidAccounting)?
            .seed
            .settlement_quantum_micros;
        let valid_until_ms = h
            .prepared_order
            .as_ref()
            .map(|scope| {
                self.record(&scope.client_order_id)
                    .map(|r| r.intent.expires_ms)
            })
            .transpose()?
            .unwrap_or(u64::MAX)
            .min(h.valuation.valid_until_ms);
        Ok(Some(CryptoAllocationRiskView {
            current: now_ms >= h.committed_ms
                && now_ms < valid_until_ms
                && now_ms / DAY_MS == h.day_utc
                && balance_digest == h.input.expected_balance_digest
                && digest(&self.state.records)? == h.orders_digest,
            valuation_digest: digest(h)?,
            balance_digest: h.input.expected_balance_digest.clone(),
            source_digest: h.input.source_digest.clone(),
            observed_ms: h.input.observed_ms,
            valid_until_ms,
            policy_revision: h.input.policy.revision,
            policy_digest: h.input.policy.sha256.clone(),
            day_utc: h.day_utc,
            equity_micros: h.valuation.equity_micros,
            equity_deficit_micros: h.valuation.equity_deficit_micros,
            day_open_equity_micros: h.day_open_equity_micros,
            peak_equity_micros: h.peak_equity_micros,
            gross_exposure_micros: h.valuation.gross_exposure_micros,
            symbol_exposure_micros: h.valuation.symbol_exposure_micros.clone(),
            daily_order_count: h.daily_order_count,
            day_loss_latched: h.day_loss_latched,
            drawdown_latched: h.drawdown_latched,
            deficit_latched: h.deficit_latched,
            paused: h.input.paused,
            fee_reserve_bps: h.input.fee_reserve_bps,
            settlement_quantum_micros: quantum,
        }))
    }

    pub fn allocation_risk_state(&self, now_ms: u64) -> Result<CryptoAllocationRiskState> {
        self.check_clock(now_ms)?;
        let balance = self.allocation_balance()?;
        let balance_digest = balance.as_ref().map(digest).transpose()?;
        let risk = self.risk_view(now_ms, balance_digest.as_deref().unwrap_or(""))?;
        Ok(CryptoAllocationRiskState {
            execution_authority: false,
            account_key: self.state.account_key.clone(),
            evaluated_ms: now_ms,
            balance,
            balance_digest,
            risk,
        })
    }

    /// Only a trusted reconciler can supply the policy and broker-source provenance. The first
    /// mark must precede every order, so installing this feature cannot reset a traded ledger.
    pub fn mark_allocation(
        &mut self,
        input: CryptoAllocationValuationInput,
        now_ms: u64,
    ) -> Result<CryptoAllocationValuationReceipt> {
        let basis = self
            .allocation_balance()?
            .ok_or(CryptoJournalError::InvalidAccounting)?;
        self.mark_allocation_basis(input, basis, None, now_ms)
    }

    pub fn mark_prepared_allocation(
        &mut self,
        scope: CryptoPreparedOrderScope,
        input: CryptoAllocationValuationInput,
        now_ms: u64,
    ) -> Result<CryptoAllocationValuationReceipt> {
        let prepared = self.checked_prepared_allocation(&scope, now_ms)?;
        self.mark_allocation_basis(input, prepared.balance, Some(scope), now_ms)
    }

    fn mark_allocation_basis(
        &mut self,
        input: CryptoAllocationValuationInput,
        basis: CryptoAllocationBalance,
        prepared_order: Option<CryptoPreparedOrderScope>,
        now_ms: u64,
    ) -> Result<CryptoAllocationValuationReceipt> {
        self.check_clock(now_ms)?;
        if input.account_key != self.state.account_key {
            return invalid();
        }
        let quantum = self
            .state
            .accounting
            .as_ref()
            .ok_or(CryptoJournalError::InvalidAccounting)?
            .seed
            .settlement_quantum_micros;
        let valuation = calculate(&basis, &input, quantum, now_ms)?;
        let day = now_ms / DAY_MS;
        let equity = valuation.equity_micros;
        let previous = self.state.risk_history.as_ref();
        if let Some(old) = previous {
            if input.observed_ms < old.input.observed_ms
                || input.policy.revision < old.input.policy.revision
                || input.policy.updated_ns < old.input.policy.updated_ns
                || (input.policy.revision == old.input.policy.revision
                    && input.policy != old.input.policy)
            {
                return invalid();
            }
            if input.quotes.iter().any(|quote| {
                old.input.quotes.iter().any(|before| {
                    before.quote.symbol == quote.quote.symbol
                        && quote.quote.observed_ms < before.quote.observed_ms
                })
            }) {
                return invalid();
            }
            if old.day_utc == day
                && old.input == input
                && old.orders_digest == digest(&self.state.records)?
                && old.prepared_order == prepared_order
            {
                return Ok(CryptoAllocationValuationReceipt {
                    changed: false,
                    execution_authority: false,
                    risk: self
                        .risk_view(now_ms, &input.expected_balance_digest)?
                        .ok_or(CryptoJournalError::InvalidValuation)?,
                });
            }
        } else if !self.state.records.is_empty()
            || equity == 0
            || basis.cash_deficit_micros > 0
            || !basis.quantity_deficits.is_empty()
        {
            return invalid();
        }
        // Carry the previous mark through an offline/midnight gap; the first quote after a
        // restart must not erase overnight losses by replacing opening equity with itself.
        let day_open = previous.map_or(equity, |h| {
            if h.day_utc == day {
                h.day_open_equity_micros
            } else {
                h.valuation.equity_micros
            }
        });
        let peak = previous.map_or(equity, |h| h.peak_equity_micros.max(equity));
        let history = RiskHistory {
            day_loss_latched: previous.is_some_and(|h| h.day_utc == day && h.day_loss_latched)
                || day_open.saturating_sub(equity) >= input.policy.limits.daily_loss_micros,
            drawdown_latched: previous.is_some_and(|h| h.drawdown_latched)
                || peak.saturating_sub(equity) >= input.policy.limits.drawdown_micros,
            deficit_latched: previous.is_some_and(|h| h.deficit_latched)
                || valuation.equity_deficit_micros > 0
                || basis.cash_deficit_micros > 0
                || !basis.quantity_deficits.is_empty(),
            input,
            basis,
            valuation,
            orders_digest: digest(&self.state.records)?,
            committed_ms: now_ms,
            day_utc: day,
            day_open_equity_micros: day_open,
            peak_equity_micros: peak,
            daily_order_count: self.buy_dispatches(day),
            prepared_order,
        };
        let balance_digest = history.input.expected_balance_digest.clone();
        let mut next = self.clone();
        next.state.risk_history = Some(history);
        next.state.last_modified_ms = now_ms;
        next.validate_risk_history()?;
        next.save_snapshot()?;
        let risk = next
            .risk_view(now_ms, &balance_digest)?
            .ok_or(CryptoJournalError::InvalidValuation)?;
        *self = next;
        Ok(CryptoAllocationValuationReceipt {
            changed: true,
            execution_authority: false,
            risk,
        })
    }

    pub fn valued_account(&self, symbol: &str, now_ms: u64) -> Result<CryptoValuedAccount> {
        if !matches!(symbol, "BTC-USD" | "ETH-USD") {
            return invalid();
        }
        self.check_clock(now_ms)?;
        let balance = self
            .allocation_balance()?
            .ok_or(CryptoJournalError::InvalidAccounting)?;
        self.valued_account_basis(symbol, balance, None, now_ms)
    }

    pub fn prepared_valued_account(
        &self,
        scope: &CryptoPreparedOrderScope,
        now_ms: u64,
    ) -> Result<CryptoValuedAccount> {
        let prepared = self.checked_prepared_allocation(scope, now_ms)?;
        self.valued_account_basis(
            &prepared.intent.symbol,
            prepared.balance,
            Some(scope.clone()),
            now_ms,
        )
    }

    fn valued_account_basis(
        &self,
        symbol: &str,
        balance: CryptoAllocationBalance,
        prepared_order: Option<CryptoPreparedOrderScope>,
        now_ms: u64,
    ) -> Result<CryptoValuedAccount> {
        let h = self
            .state
            .risk_history
            .as_ref()
            .ok_or(CryptoJournalError::InvalidValuation)?;
        let risk = self
            .risk_view(now_ms, &balance.snapshot_digest()?)?
            .ok_or(CryptoJournalError::InvalidValuation)?;
        if !risk.current || h.prepared_order != prepared_order {
            return invalid();
        }
        Ok(CryptoValuedAccount {
            execution_authority: false,
            policy: h.input.policy.clone(),
            valuation_digest: risk.valuation_digest,
            balance_digest: risk.balance_digest,
            valid_until_ms: risk.valid_until_ms,
            prepared_order,
            account: ExactCryptoAccountSnapshot {
                account_key: self.state.account_key.clone(),
                symbol: symbol.into(),
                snapshot_digest: balance.ledger_digest,
                // The oldest held-asset quote also limits freshness downstream.
                observed_ms: h
                    .input
                    .quotes
                    .iter()
                    .map(|q| q.quote.observed_ms)
                    .fold(h.input.observed_ms, u64::min),
                day_utc: risk.day_utc,
                buying_power_micros: balance.available_cash_micros,
                equity_micros: risk.equity_micros,
                day_open_equity_micros: risk.day_open_equity_micros,
                peak_equity_micros: risk.peak_equity_micros,
                gross_exposure_micros: risk.gross_exposure_micros,
                symbol_exposure_micros: *risk.symbol_exposure_micros.get(symbol).unwrap_or(&0),
                available_quantity: balance
                    .available_quantities
                    .get(symbol)
                    .cloned()
                    .unwrap_or(decimal(0)?),
                daily_order_count: risk.daily_order_count,
                unresolved_orders: balance.unresolved_orders,
                paused: risk.paused,
                loss_stop_latched: risk.day_loss_latched
                    || risk.drawdown_latched
                    || risk.deficit_latched,
                fee_reserve_bps: risk.fee_reserve_bps,
                settlement_quantum_micros: risk.settlement_quantum_micros,
            },
        })
    }

    pub(super) fn validate_risk_history(&self) -> Result<()> {
        let Some(h) = &self.state.risk_history else {
            return Ok(());
        };
        let accounting = self
            .state
            .accounting
            .as_ref()
            .ok_or(CryptoJournalError::InvalidAccounting)?;
        let value = calculate(
            &h.basis,
            &h.input,
            accounting.seed.settlement_quantum_micros,
            h.committed_ms,
        )?;
        if h.input.account_key != self.state.account_key
            || h.committed_ms > self.state.last_modified_ms
            || h.committed_ms < accounting.initialized_ms
            || h.day_utc != h.committed_ms / DAY_MS
            || !is_digest(&h.orders_digest)
            || value != h.valuation
            || h.day_open_equity_micros > h.peak_equity_micros
            || value.equity_micros > h.peak_equity_micros
            || h.peak_equity_micros == 0
            || h.daily_order_count > self.buy_dispatches(h.day_utc)
            || (!h.day_loss_latched
                && h.day_open_equity_micros.saturating_sub(value.equity_micros)
                    >= h.input.policy.limits.daily_loss_micros)
            || (!h.drawdown_latched
                && h.peak_equity_micros.saturating_sub(value.equity_micros)
                    >= h.input.policy.limits.drawdown_micros)
            || (!h.deficit_latched
                && (value.equity_deficit_micros > 0
                    || h.basis.cash_deficit_micros > 0
                    || !h.basis.quantity_deficits.is_empty()))
        {
            return invalid();
        }
        if h.orders_digest == digest(&self.state.records)? {
            let basis = match &h.prepared_order {
                Some(scope) => {
                    self.checked_prepared_allocation(scope, h.committed_ms)?
                        .balance
                }
                None => self
                    .allocation_balance()?
                    .ok_or(CryptoJournalError::InvalidAccounting)?,
            };
            if h.daily_order_count != self.buy_dispatches(h.day_utc) || basis != h.basis {
                return invalid();
            }
        }
        if let Some(scope) = &h.prepared_order {
            let record = self.record(&scope.client_order_id)?;
            if scope.revision == 0
                || scope.revision > record.revision
                || scope.intent_digest != record.intent_digest
                || h.committed_ms < record.intent.created_ms
                || h.committed_ms >= record.intent.expires_ms
            {
                return invalid();
            }
        }
        Ok(())
    }
}
