//! Strategy-owned allocation accounting. Broker balances remain independent spending caps.
//! A seed is a trusted provisioning input, not proof of account ownership or available funds.

use super::*;
use crate::{ceil_crypto_notional_micros, floor_crypto_notional_micros};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CryptoAccountingSeed {
    pub schema_version: u32,
    pub account_key: String,
    pub cash_micros: u64,
    /// Broker settlement currency quantum, independently verified during provisioning.
    pub settlement_quantum_micros: u64,
    pub quantities: BTreeMap<String, BrokerDecimal>,
    pub source_digest: String,
    pub observed_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AccountingState {
    pub(super) seed: CryptoAccountingSeed,
    pub(super) initialized_ms: u64,
    // Replace an order's cumulative observation on amendments; never add it a second time.
    terminal_observations: BTreeMap<String, CryptoOrderObservation>,
}

/// Conservative allocation balances, not a certified broker snapshot or trading permission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CryptoAllocationBalance {
    pub execution_authority: bool,
    pub account_key: String,
    pub ledger_digest: String,
    pub observed_ms: u64,
    pub cash_micros: u64,
    pub cash_deficit_micros: u64,
    pub quantities: BTreeMap<String, BrokerDecimal>,
    pub quantity_deficits: BTreeMap<String, BrokerDecimal>,
    pub available_cash_micros: u64,
    pub available_quantities: BTreeMap<String, BrokerDecimal>,
    pub accounted_orders: usize,
    pub unresolved_orders: bool,
}

impl CryptoAllocationBalance {
    /// Digest of owned assets, reservations and reconciliation time, in the journal's format.
    pub fn snapshot_digest(&self) -> Result<String> {
        digest(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CryptoAccountingReceipt {
    pub changed: bool,
    pub execution_authority: bool,
    pub balance: CryptoAllocationBalance,
}

impl CryptoAccountingSeed {
    fn validate(&self, account: &str, now_ms: u64) -> Result<()> {
        if self.schema_version != 1
            || self.account_key != account
            || !is_digest(&self.source_digest)
            || self.observed_ms > now_ms
            || self.settlement_quantum_micros == 0
            || self.quantities.len() > 2
        {
            return Err(CryptoJournalError::InvalidAccounting);
        }
        for (symbol, amount) in &self.quantities {
            if !matches!(symbol.as_str(), "BTC-USD" | "ETH-USD") || units(amount)? == 0 {
                return Err(CryptoJournalError::InvalidAccounting);
            }
        }
        Ok(())
    }
}

fn add(total: &mut u128, amount: u128) -> Result<()> {
    *total = total
        .checked_add(amount)
        .ok_or(CryptoJournalError::Capacity)?;
    Ok(())
}

pub(super) fn rounded(micros: u128, quantum: u64, up: bool) -> Result<u128> {
    let quantum = u128::from(quantum);
    let multiples = if up {
        micros.div_ceil(quantum)
    } else {
        micros / quantum
    };
    multiples
        .checked_mul(quantum)
        .ok_or(CryptoJournalError::Capacity)
}

impl CryptoBrokerJournal {
    /// Explicit, immutable seed on an empty journal. Existing account assets are not adopted.
    pub fn initialize_accounting(
        &mut self,
        seed: CryptoAccountingSeed,
        now_ms: u64,
    ) -> Result<CryptoAccountingReceipt> {
        self.check_clock(now_ms)?;
        seed.validate(&self.state.account_key, now_ms)?;
        if let Some(existing) = &self.state.accounting {
            if existing.seed != seed {
                return Err(CryptoJournalError::IdentityConflict);
            }
            return Ok(CryptoAccountingReceipt {
                changed: false,
                execution_authority: false,
                balance: self
                    .allocation_balance()?
                    .ok_or(CryptoJournalError::InvalidAccounting)?,
            });
        }
        if !self.state.records.is_empty() {
            return Err(CryptoJournalError::InvalidAccounting);
        }
        let mut next = self.clone();
        next.state.accounting = Some(AccountingState {
            seed,
            initialized_ms: now_ms,
            terminal_observations: BTreeMap::new(),
        });
        next.state.last_modified_ms = now_ms;
        let balance = next
            .allocation_balance()?
            .ok_or(CryptoJournalError::InvalidAccounting)?;
        next.save_snapshot()?;
        *self = next;
        Ok(CryptoAccountingReceipt {
            changed: true,
            execution_authority: false,
            balance,
        })
    }

    pub fn allocation_balance(&self) -> Result<Option<CryptoAllocationBalance>> {
        let Some(accounting) = &self.state.accounting else {
            return Ok(None);
        };
        let mut credits = u128::from(accounting.seed.cash_micros);
        let mut debits = 0_u128;
        let quantum = accounting.seed.settlement_quantum_micros;
        let mut assets: BTreeMap<String, (u128, u128)> = accounting
            .seed
            .quantities
            .iter()
            .map(|(symbol, amount)| Ok((symbol.clone(), (units(amount)?, 0))))
            .collect::<Result<_>>()?;
        for observation in accounting.terminal_observations.values() {
            // Per-fill settlement rounds conservatively even if the broker rounds only the
            // aggregate order. Preserve exact reports; never claim more spendable cash.
            let fee = units(&observation.fee_usd)?.div_ceil(SCALE / 1_000_000);
            add(&mut debits, rounded(fee, quantum, true)?)?;
            let asset = assets.entry(observation.symbol.clone()).or_default();
            match observation.side {
                CryptoOrderSide::Buy => add(&mut asset.0, units(&observation.filled_quantity)?)?,
                CryptoOrderSide::Sell => add(&mut asset.1, units(&observation.filled_quantity)?)?,
            }
            for fill in &observation.executions {
                match observation.side {
                    CryptoOrderSide::Buy => add(
                        &mut debits,
                        rounded(
                            u128::from(
                                ceil_crypto_notional_micros(&fill.price, &fill.quantity)
                                    .map_err(|_| CryptoJournalError::Capacity)?,
                            ),
                            quantum,
                            true,
                        )?,
                    )?,
                    CryptoOrderSide::Sell => add(
                        &mut credits,
                        rounded(
                            u128::from(
                                floor_crypto_notional_micros(&fill.price, &fill.quantity)
                                    .map_err(|_| CryptoJournalError::Capacity)?,
                            ),
                            quantum,
                            false,
                        )?,
                    )?,
                }
            }
        }
        let cash_micros = u64::try_from(credits.saturating_sub(debits))
            .map_err(|_| CryptoJournalError::Capacity)?;
        let cash_deficit_micros = u64::try_from(debits.saturating_sub(credits))
            .map_err(|_| CryptoJournalError::Capacity)?;
        let reservations = self.reservations()?;
        let mut quantities = BTreeMap::new();
        let mut quantity_deficits = BTreeMap::new();
        let mut available_quantities = BTreeMap::new();
        for (symbol, (bought, sold)) in assets {
            if sold > bought {
                quantity_deficits.insert(symbol, decimal(sold - bought)?);
            } else if bought > sold {
                let remaining = bought - sold;
                quantities.insert(symbol.clone(), decimal(remaining)?);
                let reserved = reservations
                    .sell_quantities
                    .get(&symbol)
                    .map(units)
                    .transpose()?
                    .unwrap_or(0);
                available_quantities.insert(symbol, decimal(remaining.saturating_sub(reserved))?);
            }
        }
        Ok(Some(CryptoAllocationBalance {
            execution_authority: false,
            account_key: self.state.account_key.clone(),
            ledger_digest: digest(accounting)?,
            // Risk-only marks must not make the allocation appear newly reconciled.
            observed_ms: self
                .records()
                .map(|record| record.modified_ms)
                .max()
                .unwrap_or(accounting.initialized_ms)
                .max(accounting.initialized_ms),
            cash_micros,
            cash_deficit_micros,
            quantities,
            quantity_deficits,
            available_cash_micros: cash_micros.saturating_sub(reservations.cash_micros),
            available_quantities,
            accounted_orders: accounting.terminal_observations.len(),
            unresolved_orders: self.records().any(CryptoOrderRecord::reservation_held),
        }))
    }

    pub(super) fn check_allocation(&self, intent: &CryptoOrderIntent) -> Result<()> {
        self.check_allocation_stage(intent, false)
    }

    pub(super) fn check_dispatch_allocation(&self, intent: &CryptoOrderIntent) -> Result<()> {
        self.check_allocation_stage(intent, true)
    }

    fn check_allocation_stage(&self, intent: &CryptoOrderIntent, reserved: bool) -> Result<()> {
        let Some(accounting) = &self.state.accounting else {
            return Ok(());
        };
        if intent.created_ms < accounting.initialized_ms {
            return Err(CryptoJournalError::InvalidAccounting);
        }
        let balance = self
            .allocation_balance()?
            .ok_or(CryptoJournalError::InvalidAccounting)?;
        // A terminal result must be accounted before any further allocation is reserved.
        if self.records().any(|r| {
            r.reservation_held()
                && r.observation
                    .as_ref()
                    .is_some_and(|o| o.state.is_terminal())
        }) {
            return Err(CryptoJournalError::InvalidAccounting);
        }
        let reservations = self.reservations()?;
        let cash_required = if reserved {
            reservations.cash_micros
        } else {
            reservations
                .cash_micros
                .checked_add(intent.reserved_cash_micros)
                .ok_or(CryptoJournalError::Capacity)?
        };
        let asset_reserved = reservations
            .sell_quantities
            .get(&intent.symbol)
            .map(units)
            .transpose()?
            .unwrap_or(0);
        let asset_required = if reserved {
            asset_reserved
        } else {
            asset_reserved
                .checked_add(units(&intent.quantity)?)
                .ok_or(CryptoJournalError::Capacity)?
        };
        match intent.side {
            CryptoOrderSide::Buy
                if balance.cash_deficit_micros > 0
                    || !balance.quantity_deficits.is_empty()
                    || cash_required > balance.cash_micros =>
            {
                Err(CryptoJournalError::InsufficientAllocation)
            }
            CryptoOrderSide::Sell
                if asset_required
                    > balance
                        .quantities
                        .get(&intent.symbol)
                        .map(units)
                        .transpose()?
                        .unwrap_or(0) =>
            {
                Err(CryptoJournalError::InsufficientAllocation)
            }
            _ => Ok(()),
        }
    }

    /// Books the exact current terminal result and releases its reservation in one transition.
    /// The expected digest prevents a stale worker from acknowledging a newer fee/fill amendment.
    /// This updates strategy-owned allocation; broker availability still needs independent checks.
    pub fn reconcile_terminal(
        &mut self,
        id: &str,
        expected_observation_digest: &str,
        now_ms: u64,
    ) -> Result<CryptoJournalReceipt> {
        self.check_clock(now_ms)?;
        let mut record = self.record(id)?.clone();
        let observation = record
            .observation
            .as_ref()
            .ok_or(CryptoJournalError::InvalidCoverage)?;
        if !observation.state.is_terminal()
            || record.observation_digest.as_deref() != Some(expected_observation_digest)
        {
            return Err(CryptoJournalError::InvalidCoverage);
        }
        let mut next = self.clone();
        let accounting = next
            .state
            .accounting
            .as_mut()
            .ok_or(CryptoJournalError::InvalidAccounting)?;
        if let Some(previous) = accounting.terminal_observations.get(id) {
            if previous.financial_digest()? == expected_observation_digest
                && record.account_coverage.is_some()
            {
                return Ok(Self::receipt(record, false));
            }
        }
        accounting
            .terminal_observations
            .insert(id.to_owned(), observation.clone());
        record.account_coverage = Some(CryptoAccountCoverage {
            account_key: self.state.account_key.clone(),
            observation_digest: expected_observation_digest.to_owned(),
            account_snapshot_digest: digest(accounting)?,
            snapshot_observed_ms: now_ms,
        });
        let receipt = next.commit(record, now_ms)?;
        next.validate_accounting()?;
        *self = next;
        Ok(receipt)
    }

    pub(super) fn validate_accounting(&self) -> Result<()> {
        let Some(accounting) = &self.state.accounting else {
            return Ok(());
        };
        accounting
            .seed
            .validate(&self.state.account_key, accounting.initialized_ms)?;
        if accounting.initialized_ms > self.state.last_modified_ms {
            return Err(CryptoJournalError::InvalidAccounting);
        }
        for (id, applied) in &accounting.terminal_observations {
            let record = self.record(id)?;
            let canonical = applied
                .clone()
                .validated_for(&record.intent, record.modified_ms)?;
            if &canonical != applied
                || !applied.state.is_terminal()
                || record
                    .dispatch_started_ms
                    .is_none_or(|at| at > applied.received_ms)
            {
                return Err(CryptoJournalError::InvalidAccounting);
            }
            record
                .observation
                .as_ref()
                .ok_or(CryptoJournalError::InvalidAccounting)?
                .follows(applied)?;
        }
        for record in self.records() {
            if record.intent.created_ms < accounting.initialized_ms {
                return Err(CryptoJournalError::InvalidAccounting);
            }
            if let Some(coverage) = &record.account_coverage {
                let applied = accounting
                    .terminal_observations
                    .get(&record.intent.client_order_id)
                    .ok_or(CryptoJournalError::InvalidAccounting)?;
                if applied.financial_digest()? != coverage.observation_digest {
                    return Err(CryptoJournalError::InvalidAccounting);
                }
            }
        }
        self.allocation_balance()?;
        Ok(())
    }
}
