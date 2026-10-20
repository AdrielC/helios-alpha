//! Exact-quantity recovery state. This journal does not authorize or submit orders.
//!
//! The durable host must persist the dispatch marker before contacting a broker. An uncertain
//! submission is reconciled by its original client ID; neither a timeout nor a cancel request
//! releases its reservation. A trusted account reconciler explicitly covers terminal results.
//! Caller authentication and current capital/risk admission belong outside this pure kernel.

use std::collections::BTreeMap;

use helio_execution::{BrokerDecimal, BrokerOrderState};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::exact::{decimal_units, DECIMAL_SCALE as SCALE};
const MAX_ORDERS: usize = 256;
const MAX_FILLS: usize = 64;
const MAX_SNAPSHOT_BYTES: usize = 8 * 1024 * 1024;
const MAX_INTENT_LIFETIME_MS: u64 = 120_000;

mod accounting;
use accounting::AccountingState;
pub use accounting::{CryptoAccountingReceipt, CryptoAccountingSeed, CryptoAllocationBalance};
mod valuation;
use valuation::RiskHistory;
pub use valuation::*;
mod prepared_allocation;
pub use prepared_allocation::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CryptoOrderSide {
    Buy,
    Sell,
}

/// A frozen good-til-canceled limit-order intent. This v1 contract has no configurable duration.
/// Hashes attest to external inputs, not to permission to trade.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CryptoOrderIntent {
    pub account_key: String,
    pub client_order_id: String,
    pub symbol: String,
    pub side: CryptoOrderSide,
    pub quantity: BrokerDecimal,
    pub limit_price: BrokerDecimal,
    pub reserved_cash_micros: u64,
    pub policy_revision: u64,
    pub strategy_fingerprint: String,
    pub input_digest: String,
    pub risk_reservation_id: String,
    pub created_ms: u64,
    pub expires_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CryptoOrderFill {
    pub execution_id: String,
    pub quantity: BrokerDecimal,
    pub price: BrokerDecimal,
    pub occurred_ms: u64,
}

/// A normalized broker report. Exact fill quantities and fees must be retained by the adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CryptoOrderObservation {
    pub account_key: String,
    pub client_order_id: String,
    pub broker_order_id: String,
    pub symbol: String,
    pub side: CryptoOrderSide,
    pub ordered_quantity: BrokerDecimal,
    pub limit_price: BrokerDecimal,
    pub state: BrokerOrderState,
    pub filled_quantity: BrokerDecimal,
    pub fee_usd: BrokerDecimal,
    pub executions: Vec<CryptoOrderFill>,
    pub updated_ms: u64,
    pub received_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CryptoOrderPhase {
    Prepared,
    SubmissionUncertain,
    Pending,
    Open,
    PartiallyFilled,
    Filled,
    Canceled,
    Failed,
    ExpiredUnsent,
}

impl From<BrokerOrderState> for CryptoOrderPhase {
    fn from(state: BrokerOrderState) -> Self {
        match state {
            BrokerOrderState::Pending => Self::Pending,
            BrokerOrderState::Open => Self::Open,
            BrokerOrderState::PartiallyFilled => Self::PartiallyFilled,
            BrokerOrderState::Filled => Self::Filled,
            BrokerOrderState::Canceled => Self::Canceled,
            BrokerOrderState::Failed => Self::Failed,
        }
    }
}

/// The trusted reconciler has incorporated this exact terminal observation in account state.
/// The journal cannot authenticate the underlying broker connection or validate that accounting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CryptoAccountCoverage {
    pub account_key: String,
    pub observation_digest: String,
    pub account_snapshot_digest: String,
    pub snapshot_observed_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CryptoOrderRecord {
    pub intent: CryptoOrderIntent,
    pub intent_digest: String,
    pub phase: CryptoOrderPhase,
    pub dispatch_started_ms: Option<u64>,
    pub cancel_requested_ms: Option<u64>,
    pub observation: Option<CryptoOrderObservation>,
    pub observation_digest: Option<String>,
    pub account_coverage: Option<CryptoAccountCoverage>,
    pub revision: u64,
    pub modified_ms: u64,
}

impl CryptoOrderRecord {
    pub fn reservation_held(&self) -> bool {
        self.phase != CryptoOrderPhase::ExpiredUnsent && self.account_coverage.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CryptoJournalReceipt {
    pub changed: bool,
    /// Always false: recording a dispatch marker is not execution authorization.
    pub execution_authority: bool,
    pub record: CryptoOrderRecord,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CryptoJournalReservations {
    pub cash_micros: u64,
    pub sell_quantities: BTreeMap<String, BrokerDecimal>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CryptoRecoveryAction {
    LookupOrder,
    CancelOrder,
    ReconcileAccount,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CryptoRecoveryItem {
    pub client_order_id: String,
    pub revision: u64,
    pub expires_ms: u64,
    pub action: CryptoRecoveryAction,
}

/// A bounded work list, never permission to submit or replace an order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CryptoRecoveryWork {
    pub evaluated_ms: u64,
    pub execution_authority: bool,
    pub items: Vec<CryptoRecoveryItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct JournalSnapshot {
    schema_version: u32,
    account_key: String,
    last_modified_ms: u64,
    records: BTreeMap<String, CryptoOrderRecord>,
    // Omitted until explicitly initialized, preserving historical snapshot digests and replay.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    accounting: Option<AccountingState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    risk_history: Option<RiskHistory>,
}

/// Persist through the durable host, then restore only through `restore_snapshot`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CryptoBrokerJournal {
    state: JournalSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CryptoJournalError {
    #[error("invalid journal identity or intent")]
    InvalidIntent,
    #[error("journal clock regressed or input is from the future")]
    InvalidClock,
    #[error("client order identity already has different immutable contents")]
    IdentityConflict,
    #[error("order is not present")]
    UnknownOrder,
    #[error("operation is incompatible with the recorded order phase")]
    InvalidPhase,
    #[error("broker observation is incomplete or inconsistent")]
    InvalidObservation,
    #[error("broker observation regressed")]
    ObservationRegression,
    #[error("account snapshot does not cover the current terminal observation")]
    InvalidCoverage,
    #[error("invalid or uninitialized allocation accounting")]
    InvalidAccounting,
    #[error("allocated cash or assets are unavailable")]
    InsufficientAllocation,
    #[error("allocation valuation is missing, stale, or inconsistent")]
    InvalidValuation,
    #[error("journal capacity or arithmetic bound exceeded")]
    Capacity,
    #[error("invalid journal snapshot")]
    InvalidSnapshot,
}

type Result<T> = std::result::Result<T, CryptoJournalError>;

fn digest<T: Serialize>(value: &T) -> Result<String> {
    let bytes = serde_json::to_vec(value).map_err(|_| CryptoJournalError::InvalidSnapshot)?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

pub(crate) fn is_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn is_uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
            }
        })
        && value
            .bytes()
            .any(|b| matches!(b, b'1'..=b'9' | b'a'..=b'f'))
}

// BrokerDecimal's Ord is lexical. Numeric comparisons must use this bounded exact conversion.
fn units(value: &BrokerDecimal) -> Result<u128> {
    decimal_units(value).map_err(|_| CryptoJournalError::Capacity)
}

fn decimal(value: u128) -> Result<BrokerDecimal> {
    BrokerDecimal::try_new(format!("{}.{:018}", value / SCALE, value % SCALE))
        .map_err(|_| CryptoJournalError::Capacity)
}

impl CryptoOrderIntent {
    pub fn snapshot_digest(&self) -> Result<String> {
        self.validate_for_account(&self.account_key)?;
        digest(self)
    }

    /// Checks immutable intent structure and account binding, not trading permission.
    /// Expiry does not prevent later read-only recovery of an already dispatched order.
    pub fn validate_for_account(&self, account: &str) -> Result<()> {
        if self.account_key != account
            || !is_digest(account)
            || !is_uuid(&self.client_order_id)
            || !matches!(self.symbol.as_str(), "BTC-USD" | "ETH-USD")
            || units(&self.quantity)? == 0
            || units(&self.limit_price)? == 0
            || self.policy_revision == 0
            || !is_digest(&self.strategy_fingerprint)
            || !is_digest(&self.input_digest)
            || !is_digest(&self.risk_reservation_id)
            || self.expires_ms <= self.created_ms
            || self.expires_ms - self.created_ms > MAX_INTENT_LIFETIME_MS
            || (self.side == CryptoOrderSide::Buy) != (self.reserved_cash_micros > 0)
        {
            return Err(CryptoJournalError::InvalidIntent);
        }
        Ok(())
    }
}

impl CryptoOrderObservation {
    /// Validates one report and returns its canonical fill order. This does not check
    /// progression from a previous report or release a reservation; the journal does that.
    pub fn validated_for(mut self, intent: &CryptoOrderIntent, now_ms: u64) -> Result<Self> {
        intent.validate_for_account(&intent.account_key)?;
        self.normalize_and_validate(intent, now_ms)?;
        Ok(self)
    }

    /// Check identity, exact amounts and monotonic progression before acting on a report.
    /// The caller must obtain `record` from its authoritative journal.
    pub fn validated_after(self, record: &CryptoOrderRecord, now_ms: u64) -> Result<Self> {
        let dispatched = record
            .dispatch_started_ms
            .ok_or(CryptoJournalError::InvalidPhase)?;
        let observation = self.validated_for(&record.intent, now_ms)?;
        if observation.received_ms < dispatched {
            return Err(CryptoJournalError::InvalidObservation);
        }
        if let Some(previous) = &record.observation {
            observation.follows(previous)?;
        }
        Ok(observation)
    }

    fn normalize_and_validate(&mut self, intent: &CryptoOrderIntent, now_ms: u64) -> Result<()> {
        if self.account_key != intent.account_key
            || self.client_order_id != intent.client_order_id
            || self.symbol != intent.symbol
            || self.side != intent.side
            || self.ordered_quantity != intent.quantity
            || self.limit_price != intent.limit_price
            || !is_uuid(&self.broker_order_id)
            || self.updated_ms < intent.created_ms
            || self.updated_ms > self.received_ms
            || self.received_ms > now_ms
            || self.executions.len() > MAX_FILLS
        {
            return Err(CryptoJournalError::InvalidObservation);
        }
        self.executions
            .sort_by(|a, b| a.execution_id.cmp(&b.execution_id));
        let requested = units(&intent.quantity)?;
        let filled = units(&self.filled_quantity)?;
        let limit = units(&intent.limit_price)?;
        units(&self.fee_usd)?;
        let mut sum = 0_u128;
        let mut previous_id: Option<&str> = None;
        for fill in &self.executions {
            let quantity = units(&fill.quantity)?;
            let price = units(&fill.price)?;
            if fill.execution_id.is_empty()
                || fill.execution_id.len() > 128
                || !fill
                    .execution_id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_:.".contains(&b))
                || previous_id == Some(fill.execution_id.as_str())
                || quantity == 0
                || price == 0
                || fill.occurred_ms < intent.created_ms
                || fill.occurred_ms > self.updated_ms
                || (intent.side == CryptoOrderSide::Buy && price > limit)
                || (intent.side == CryptoOrderSide::Sell && price < limit)
            {
                return Err(CryptoJournalError::InvalidObservation);
            }
            sum = sum
                .checked_add(quantity)
                .ok_or(CryptoJournalError::Capacity)?;
            previous_id = Some(&fill.execution_id);
        }
        if filled > requested
            || sum != filled
            || match self.state {
                BrokerOrderState::Pending | BrokerOrderState::Open => filled != 0,
                BrokerOrderState::PartiallyFilled => filled == 0 || filled == requested,
                BrokerOrderState::Filled => filled != requested,
                BrokerOrderState::Canceled | BrokerOrderState::Failed => false,
            }
        {
            return Err(CryptoJournalError::InvalidObservation);
        }
        Ok(())
    }

    fn financial_digest(&self) -> Result<String> {
        let mut observation = self.clone();
        // Receipt time is local metadata, not a new broker result or a new reservation.
        observation.received_ms = 0;
        digest(&observation)
    }

    fn follows(&self, previous: &Self) -> Result<()> {
        if self.broker_order_id != previous.broker_order_id
            || self.updated_ms < previous.updated_ms
            || self.received_ms < previous.received_ms
            || units(&self.filled_quantity)? < units(&previous.filled_quantity)?
            || units(&self.fee_usd)? < units(&previous.fee_usd)?
            || previous
                .executions
                .iter()
                .any(|old| !self.executions.iter().any(|new| new == old))
            || match previous.state {
                BrokerOrderState::Pending => false,
                BrokerOrderState::Open => self.state == BrokerOrderState::Pending,
                BrokerOrderState::PartiallyFilled => {
                    matches!(
                        self.state,
                        BrokerOrderState::Pending | BrokerOrderState::Open
                    )
                }
                BrokerOrderState::Filled => self.state != BrokerOrderState::Filled,
                BrokerOrderState::Canceled => !matches!(
                    self.state,
                    BrokerOrderState::Canceled | BrokerOrderState::Filled
                ),
                BrokerOrderState::Failed => !matches!(
                    self.state,
                    BrokerOrderState::Failed
                        | BrokerOrderState::Canceled
                        | BrokerOrderState::Filled
                ),
            }
        {
            return Err(CryptoJournalError::ObservationRegression);
        }
        if self.updated_ms == previous.updated_ms
            && self.financial_digest()? != previous.financial_digest()?
        {
            return Err(CryptoJournalError::ObservationRegression);
        }
        Ok(())
    }
}

impl CryptoBrokerJournal {
    pub fn new(account_key: String) -> Result<Self> {
        if !is_digest(&account_key) {
            return Err(CryptoJournalError::InvalidIntent);
        }
        Ok(Self {
            state: JournalSnapshot {
                schema_version: 1,
                account_key,
                last_modified_ms: 0,
                records: BTreeMap::new(),
                accounting: None,
                risk_history: None,
            },
        })
    }

    pub fn record(&self, client_order_id: &str) -> Result<&CryptoOrderRecord> {
        self.state
            .records
            .get(client_order_id)
            .ok_or(CryptoJournalError::UnknownOrder)
    }

    /// Bounded by journal capacity; retained identities are never silently evicted.
    pub fn records(&self) -> impl Iterator<Item = &CryptoOrderRecord> {
        self.state.records.values()
    }

    pub fn reservations(&self) -> Result<CryptoJournalReservations> {
        let mut cash_micros = 0_u64;
        let mut sells = BTreeMap::<String, u128>::new();
        for record in self
            .state
            .records
            .values()
            .filter(|record| record.reservation_held())
        {
            cash_micros = cash_micros
                .checked_add(record.intent.reserved_cash_micros)
                .ok_or(CryptoJournalError::Capacity)?;
            if record.intent.side == CryptoOrderSide::Sell {
                let entry = sells.entry(record.intent.symbol.clone()).or_default();
                *entry = entry
                    .checked_add(units(&record.intent.quantity)?)
                    .ok_or(CryptoJournalError::Capacity)?;
            }
        }
        Ok(CryptoJournalReservations {
            cash_micros,
            sell_quantities: sells
                .into_iter()
                .map(|(symbol, n)| Ok((symbol, decimal(n)?)))
                .collect::<Result<_>>()?,
        })
    }

    fn check_clock(&self, now_ms: u64) -> Result<()> {
        if now_ms < self.state.last_modified_ms {
            return Err(CryptoJournalError::InvalidClock);
        }
        Ok(())
    }

    fn receipt(record: CryptoOrderRecord, changed: bool) -> CryptoJournalReceipt {
        CryptoJournalReceipt {
            changed,
            execution_authority: false,
            record,
        }
    }

    fn commit(
        &mut self,
        mut record: CryptoOrderRecord,
        now_ms: u64,
    ) -> Result<CryptoJournalReceipt> {
        record.revision = record
            .revision
            .checked_add(1)
            .ok_or(CryptoJournalError::Capacity)?;
        record.modified_ms = now_ms;
        // Commit only after all bounds pass. Invalid input must not partly change durable state.
        let mut next = self.clone();
        next.state
            .records
            .insert(record.intent.client_order_id.clone(), record.clone());
        next.state.last_modified_ms = now_ms;
        next.reservations()?;
        next.save_snapshot()?;
        *self = next;
        Ok(Self::receipt(record, true))
    }

    pub fn prepare(
        &mut self,
        intent: CryptoOrderIntent,
        now_ms: u64,
    ) -> Result<CryptoJournalReceipt> {
        self.check_clock(now_ms)?;
        intent.validate_for_account(&self.state.account_key)?;
        if let Some(existing) = self.state.records.get(&intent.client_order_id) {
            if existing.intent != intent {
                return Err(CryptoJournalError::IdentityConflict);
            }
            return Ok(Self::receipt(existing.clone(), false));
        }
        if self.state.records.len() >= MAX_ORDERS {
            return Err(CryptoJournalError::Capacity);
        }
        if self
            .state
            .records
            .values()
            .any(|record| record.intent.risk_reservation_id == intent.risk_reservation_id)
        {
            return Err(CryptoJournalError::IdentityConflict);
        }
        if intent.created_ms > now_ms || now_ms >= intent.expires_ms {
            return Err(CryptoJournalError::InvalidClock);
        }
        self.check_allocation(&intent)?;
        self.commit(
            CryptoOrderRecord {
                intent_digest: digest(&intent)?,
                intent,
                phase: CryptoOrderPhase::Prepared,
                dispatch_started_ms: None,
                cancel_requested_ms: None,
                observation: None,
                observation_digest: None,
                account_coverage: None,
                revision: 0,
                modified_ms: now_ms,
            },
            now_ms,
        )
    }

    /// A repeated call never starts a second attempt. A lost reply requires reconciliation.
    /// RPC caches can replay a receipt: callers must never interpret `changed` as trade authority.
    pub fn mark_dispatch_started(&mut self, id: &str, now_ms: u64) -> Result<CryptoJournalReceipt> {
        self.check_clock(now_ms)?;
        let mut record = self.record(id)?.clone();
        if record.phase != CryptoOrderPhase::Prepared {
            return Ok(Self::receipt(record, false));
        }
        if now_ms >= record.intent.expires_ms {
            record.phase = CryptoOrderPhase::ExpiredUnsent;
        } else {
            self.check_dispatch_allocation(&record.intent)?;
            record.dispatch_started_ms = Some(now_ms);
            record.phase = CryptoOrderPhase::SubmissionUncertain;
        }
        self.commit(record, now_ms)
    }

    pub fn request_cancel(&mut self, id: &str, now_ms: u64) -> Result<CryptoJournalReceipt> {
        self.check_clock(now_ms)?;
        let mut record = self.record(id)?.clone();
        if record.dispatch_started_ms.is_none() {
            return Err(CryptoJournalError::InvalidPhase);
        }
        if record.cancel_requested_ms.is_some()
            || record
                .observation
                .as_ref()
                .is_some_and(|obs| obs.state.is_terminal())
        {
            return Ok(Self::receipt(record, false));
        }
        record.cancel_requested_ms = Some(now_ms);
        self.commit(record, now_ms)
    }

    /// Persist deadline effects before returning recovery work. A GTC order does not expire
    /// at the broker when this journal's signal expires: dispatched orders require a lookup
    /// and cancellation, and keep their reservation until terminal account reconciliation.
    /// The host must durably commit this call before performing any returned work.
    pub fn plan_recovery(&mut self, now_ms: u64) -> Result<CryptoRecoveryWork> {
        self.check_clock(now_ms)?;
        let mut next = self.clone();
        let ids: Vec<_> = next.state.records.keys().cloned().collect();
        let mut items = Vec::new();
        for id in ids {
            let record = next.record(&id)?;
            if record.phase == CryptoOrderPhase::Prepared {
                if now_ms >= record.intent.expires_ms {
                    // This cannot dispatch: the expiry check above and the transition
                    // use the same timestamp. Only provably unsent money is released.
                    next.mark_dispatch_started(&id, now_ms)?;
                }
                continue;
            }
            if !record.reservation_held() {
                continue;
            }
            let terminal = record
                .observation
                .as_ref()
                .is_some_and(|observation| observation.state.is_terminal());
            if !terminal && now_ms >= record.intent.expires_ms {
                next.request_cancel(&id, now_ms)?;
            }
            let record = next.record(&id)?;
            let action = if terminal {
                CryptoRecoveryAction::ReconcileAccount
            } else if record.cancel_requested_ms.is_some() {
                CryptoRecoveryAction::CancelOrder
            } else {
                CryptoRecoveryAction::LookupOrder
            };
            items.push(CryptoRecoveryItem {
                client_order_id: id,
                revision: record.revision,
                expires_ms: record.intent.expires_ms,
                action,
            });
        }
        *self = next;
        Ok(CryptoRecoveryWork {
            evaluated_ms: now_ms,
            execution_authority: false,
            items,
        })
    }

    pub fn observe(
        &mut self,
        observation: CryptoOrderObservation,
        now_ms: u64,
    ) -> Result<CryptoJournalReceipt> {
        self.check_clock(now_ms)?;
        let mut record = self.record(&observation.client_order_id)?.clone();
        let observation = observation.validated_after(&record, now_ms)?;
        let observation_digest = observation.financial_digest()?;
        if record.observation_digest.as_ref() == Some(&observation_digest) {
            return Ok(Self::receipt(record, false));
        }
        record.phase = observation.state.into();
        record.observation = Some(observation);
        record.observation_digest = Some(observation_digest);
        record.account_coverage = None;
        self.commit(record, now_ms)
    }

    pub fn cover_account(
        &mut self,
        id: &str,
        coverage: CryptoAccountCoverage,
        now_ms: u64,
    ) -> Result<CryptoJournalReceipt> {
        self.check_clock(now_ms)?;
        // Once accounting is enabled, only the atomic booking path can release funds.
        if self.state.accounting.is_some() {
            return Err(CryptoJournalError::InvalidCoverage);
        }
        let mut record = self.record(id)?.clone();
        Self::validate_coverage(&record, &coverage, now_ms)?;
        if record.account_coverage.as_ref() == Some(&coverage) {
            return Ok(Self::receipt(record, false));
        }
        if record
            .account_coverage
            .as_ref()
            .is_some_and(|previous| coverage.snapshot_observed_ms < previous.snapshot_observed_ms)
        {
            return Err(CryptoJournalError::InvalidCoverage);
        }
        record.account_coverage = Some(coverage);
        self.commit(record, now_ms)
    }

    fn validate_coverage(
        record: &CryptoOrderRecord,
        coverage: &CryptoAccountCoverage,
        now_ms: u64,
    ) -> Result<()> {
        let observation = record
            .observation
            .as_ref()
            .ok_or(CryptoJournalError::InvalidCoverage)?;
        if !observation.state.is_terminal()
            || coverage.account_key != record.intent.account_key
            || Some(&coverage.observation_digest) != record.observation_digest.as_ref()
            || !is_digest(&coverage.account_snapshot_digest)
            || coverage.snapshot_observed_ms < observation.received_ms
            || coverage.snapshot_observed_ms > now_ms
        {
            return Err(CryptoJournalError::InvalidCoverage);
        }
        Ok(())
    }

    pub fn save_snapshot(&self) -> Result<Vec<u8>> {
        let bytes =
            serde_json::to_vec(&self.state).map_err(|_| CryptoJournalError::InvalidSnapshot)?;
        if bytes.len() > MAX_SNAPSHOT_BYTES {
            return Err(CryptoJournalError::Capacity);
        }
        Ok(bytes)
    }

    pub fn restore_snapshot(account_key: &str, bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_SNAPSHOT_BYTES {
            return Err(CryptoJournalError::InvalidSnapshot);
        }
        let state: JournalSnapshot =
            serde_json::from_slice(bytes).map_err(|_| CryptoJournalError::InvalidSnapshot)?;
        let journal = Self { state };
        journal
            .validate_snapshot(account_key)
            .map_err(|_| CryptoJournalError::InvalidSnapshot)?;
        Ok(journal)
    }

    fn validate_snapshot(&self, account_key: &str) -> Result<()> {
        if self.state.schema_version != 1
            || self.state.account_key != account_key
            || !is_digest(account_key)
            || self.state.records.len() > MAX_ORDERS
        {
            return Err(CryptoJournalError::InvalidSnapshot);
        }
        let mut reservation_ids = std::collections::BTreeSet::new();
        for (id, record) in &self.state.records {
            record.intent.validate_for_account(account_key)?;
            if id != &record.intent.client_order_id
                || record.intent_digest != digest(&record.intent)?
                || record.revision == 0
                || record.modified_ms < record.intent.created_ms
                || record.modified_ms > self.state.last_modified_ms
                || !reservation_ids.insert(&record.intent.risk_reservation_id)
            {
                return Err(CryptoJournalError::InvalidSnapshot);
            }
            match record.phase {
                CryptoOrderPhase::Prepared | CryptoOrderPhase::ExpiredUnsent => {
                    if record.dispatch_started_ms.is_some()
                        || record.observation.is_some()
                        || record.cancel_requested_ms.is_some()
                        || (record.phase == CryptoOrderPhase::ExpiredUnsent
                            && record.modified_ms < record.intent.expires_ms)
                        || (record.phase == CryptoOrderPhase::Prepared
                            && record.modified_ms >= record.intent.expires_ms)
                    {
                        return Err(CryptoJournalError::InvalidSnapshot);
                    }
                }
                _ => {
                    let dispatch = record
                        .dispatch_started_ms
                        .ok_or(CryptoJournalError::InvalidSnapshot)?;
                    if dispatch < record.intent.created_ms
                        || dispatch >= record.intent.expires_ms
                        || dispatch > record.modified_ms
                        || record
                            .cancel_requested_ms
                            .is_some_and(|at| at < dispatch || at > record.modified_ms)
                    {
                        return Err(CryptoJournalError::InvalidSnapshot);
                    }
                }
            }
            if let Some(observation) = &record.observation {
                let mut checked = observation.clone();
                checked.normalize_and_validate(&record.intent, record.modified_ms)?;
                if &checked != observation
                    || record.phase != observation.state.into()
                    || Some(&observation.financial_digest()?) != record.observation_digest.as_ref()
                    || record
                        .dispatch_started_ms
                        .is_none_or(|at| at > observation.received_ms)
                {
                    return Err(CryptoJournalError::InvalidSnapshot);
                }
            } else if record.observation_digest.is_some()
                || !matches!(
                    record.phase,
                    CryptoOrderPhase::Prepared
                        | CryptoOrderPhase::SubmissionUncertain
                        | CryptoOrderPhase::ExpiredUnsent
                )
            {
                return Err(CryptoJournalError::InvalidSnapshot);
            }
            if let Some(coverage) = &record.account_coverage {
                Self::validate_coverage(record, coverage, record.modified_ms)?;
            }
        }
        self.reservations()?;
        self.validate_accounting()?;
        self.validate_risk_history()?;
        Ok(())
    }
}
