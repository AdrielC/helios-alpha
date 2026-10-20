//! An order-specific view may include its own reservation only while dispatch is provably unsent.
//! This never releases the durable reservation or makes it available to a different intent.

use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CryptoPreparedOrderScope {
    pub client_order_id: String,
    pub revision: u64,
    pub intent_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CryptoPreparedAllocation {
    pub execution_authority: bool,
    pub scope: CryptoPreparedOrderScope,
    pub intent: CryptoOrderIntent,
    pub balance: CryptoAllocationBalance,
    pub balance_digest: String,
}

impl CryptoBrokerJournal {
    pub fn prepared_allocation(
        &self,
        client_order_id: &str,
        now_ms: u64,
    ) -> Result<CryptoPreparedAllocation> {
        self.check_clock(now_ms)?;
        let record = self.record(client_order_id)?;
        if record.phase != CryptoOrderPhase::Prepared
            || record.dispatch_started_ms.is_some()
            || record.cancel_requested_ms.is_some()
            || record.observation.is_some()
            || record.account_coverage.is_some()
            || now_ms >= record.intent.expires_ms
            || self.records().any(|other| {
                other.intent.client_order_id != client_order_id && other.reservation_held()
            })
        {
            return Err(CryptoJournalError::InvalidPhase);
        }
        let mut balance = self
            .allocation_balance()?
            .ok_or(CryptoJournalError::InvalidAccounting)?;
        // Restore only actual owned funds, never "available + reservation": late adjustments
        // can make a reservation larger than the remaining cash or inventory.
        balance.available_cash_micros = balance.cash_micros;
        balance.available_quantities = balance.quantities.clone();
        balance.unresolved_orders = false;
        Ok(CryptoPreparedAllocation {
            execution_authority: false,
            scope: CryptoPreparedOrderScope {
                client_order_id: client_order_id.into(),
                revision: record.revision,
                intent_digest: record.intent_digest.clone(),
            },
            intent: record.intent.clone(),
            balance_digest: balance.snapshot_digest()?,
            balance,
        })
    }

    pub(super) fn checked_prepared_allocation(
        &self,
        scope: &CryptoPreparedOrderScope,
        now_ms: u64,
    ) -> Result<CryptoPreparedAllocation> {
        let view = self.prepared_allocation(&scope.client_order_id, now_ms)?;
        if &view.scope != scope {
            return Err(CryptoJournalError::IdentityConflict);
        }
        Ok(view)
    }
}
