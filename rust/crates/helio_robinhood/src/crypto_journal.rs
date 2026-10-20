//! Exact order drafts, reads and cancellation of durably recorded crypto orders.

use std::fmt;

use helio_crypto::{
    prepare_exact_crypto_order, CryptoOrderFill, CryptoOrderIntent, CryptoOrderObservation,
    CryptoOrderPhase, CryptoOrderRecord, CryptoOrderSide, CryptoPreparationContext,
    PreparedCryptoOrder,
};
use helio_execution::BrokerError;
use sha2::{Digest, Sha256};

use crate::{
    ApiDecimal, ApiOrder, HttpMethod, LimitOrderConfig, PlaceLimitOrder, RequestSigner,
    RobinhoodCryptoBroker, RobinhoodTransport, TimeInForce, UnixClock, V2_ORDERS_PATH,
};

/// Exact unsigned request body for review or a future authorized dispatcher.
/// Preparation never signs, sends, or reserves money. No execution authority is attached.
pub struct CryptoLimitOrderDraft {
    preparation: PreparedCryptoOrder,
    body: Vec<u8>,
    body_sha256: String,
}

impl CryptoLimitOrderDraft {
    pub fn preparation(&self) -> &PreparedCryptoOrder {
        &self.preparation
    }
    pub fn body(&self) -> &[u8] {
        &self.body
    }
    pub fn body_sha256(&self) -> &str {
        &self.body_sha256
    }

    pub(crate) fn restrict_valid_until(mut self, deadline: u64) -> Result<Self, BrokerError> {
        self.preparation = self
            .preparation
            .restrict_valid_until(deadline)
            .map_err(|_| BrokerError::Unavailable)?;
        Ok(self)
    }
}

impl fmt::Debug for CryptoLimitOrderDraft {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CryptoLimitOrderDraft")
            .field("body_sha256", &self.body_sha256)
            .field("expires_ms", &self.preparation.expires_ms())
            .field("body", &"[redacted]")
            .finish()
    }
}

/// Explicit operator configuration joining an API account to a journal.
///
/// This is an assertion to be verified during account provisioning, not evidence of account
/// ownership, credential validity, or trading permission. MCP brokerage and crypto identifiers
/// are never guessed, converted, or treated as interchangeable with Trading API identifiers.
#[derive(Clone)]
pub struct CryptoJournalBinding {
    api_account_number: String,
    journal_account_key: String,
}

impl fmt::Debug for CryptoJournalBinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CryptoJournalBinding")
            .field("api_account_number", &"[redacted]")
            .field("journal_account_key", &"[redacted]")
            .finish()
    }
}

impl CryptoJournalBinding {
    pub(crate) fn account_key_for(&self, account: &str) -> Result<&str, BrokerError> {
        if self.api_account_number != account {
            return Err(BrokerError::Rejected(
                "configured crypto account binding mismatch".into(),
            ));
        }
        Ok(&self.journal_account_key)
    }

    pub fn try_new(
        api_account_number: impl Into<String>,
        journal_account_key: impl Into<String>,
    ) -> Result<Self, BrokerError> {
        let api_account_number = api_account_number.into();
        let journal_account_key = journal_account_key.into();
        if api_account_number.is_empty()
            || api_account_number.len() > 128
            || !api_account_number
                .bytes()
                .all(|b| b.is_ascii_alphanumeric())
            || journal_account_key.len() != 64
            || !journal_account_key
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(BrokerError::Rejected(
                "invalid explicit crypto journal account binding".to_owned(),
            ));
        }
        Ok(Self {
            api_account_number,
            journal_account_key,
        })
    }
}

impl<T: RobinhoodTransport, C: UnixClock, S: RequestSigner> RobinhoodCryptoBroker<T, C, S> {
    /// Constructs the same immutable GTC limit body on every retry, using exact decimals.
    /// A fresh preflight is required every time; this method never performs broker I/O.
    pub fn prepare_crypto_limit_order(
        &self,
        binding: &CryptoJournalBinding,
        intent: CryptoOrderIntent,
        context: &CryptoPreparationContext,
        now_ms: u64,
    ) -> Result<CryptoLimitOrderDraft, BrokerError> {
        self.validate_crypto_binding(binding, &intent)?;
        // Duration is part of the fixed v1 journal contract, not a mutable broker setting.
        if self.config.time_in_force != TimeInForce::GoodTilCanceled {
            return Err(BrokerError::Rejected(
                "the v1 crypto journal requires good-til-canceled limit orders".into(),
            ));
        }
        let preparation =
            prepare_exact_crypto_order(intent, context, now_ms).map_err(|reason| {
                BrokerError::Rejected(format!("crypto preflight rejected: {reason:?}"))
            })?;
        let intent = preparation.intent();
        let body = serde_json::to_vec(&PlaceLimitOrder {
            symbol: &intent.symbol,
            client_order_id: &intent.client_order_id,
            side: match intent.side {
                CryptoOrderSide::Buy => "buy",
                CryptoOrderSide::Sell => "sell",
            },
            order_type: "limit",
            limit_order_config: LimitOrderConfig {
                asset_quantity: intent.quantity.as_str().to_owned(),
                limit_price: intent.limit_price.as_str().to_owned(),
                time_in_force: TimeInForce::GoodTilCanceled.as_api_str(),
            },
        })
        .map_err(|_| BrokerError::Rejected("order serialization failed".into()))?;
        Ok(CryptoLimitOrderDraft {
            body_sha256: hex::encode(Sha256::digest(&body)),
            preparation,
            body,
        })
    }

    fn validate_crypto_binding(
        &self,
        binding: &CryptoJournalBinding,
        intent: &CryptoOrderIntent,
    ) -> Result<(), BrokerError> {
        if binding.api_account_number != self.config.account_number {
            return Err(BrokerError::Rejected(
                "journal binding targets a different configured API account".to_owned(),
            ));
        }
        intent
            .validate_for_account(&binding.journal_account_key)
            .map_err(|_| BrokerError::Rejected("invalid bound crypto order intent".to_owned()))
    }

    /// Looks up the original client ID using GET requests only. An expired intent may still
    /// have filled and must remain recoverable. Missing orders, failed lookups, and malformed
    /// replies never authorize a resubmission or release the journal's reservation.
    ///
    /// This accepts an explicit account mapping, not an account certification. Returned
    /// observations must still pass the durable journal's monotonic lifecycle checks.
    pub fn read_crypto_order(
        &mut self,
        binding: &CryptoJournalBinding,
        intent: &CryptoOrderIntent,
    ) -> Result<Option<CryptoOrderObservation>, BrokerError> {
        self.validate_crypto_binding(binding, intent)?;
        let Some(order) = self.lookup_api_order(&intent.client_order_id)? else {
            return Ok(None);
        };
        let received_ms = self
            .clock
            .unix_millis()
            .map_err(|_| BrokerError::Unavailable)?;
        self.normalize_crypto_order(order, intent, received_ms)
            .map(Some)
    }

    /// Recover the original order, optionally sending a cancellation already recorded by
    /// the durable journal. This never submits or replaces an order. Re-read `record` from
    /// the journal after committing its cancellation marker; it is not trade authority.
    ///
    /// Unknown, missing or malformed replies keep the durable reservation unresolved.
    /// A retry starts with GET: a fill racing with cancellation remains an actual fill.
    pub fn recover_and_cancel_crypto_order(
        &mut self,
        binding: &CryptoJournalBinding,
        record: &CryptoOrderRecord,
    ) -> Result<Option<CryptoOrderObservation>, BrokerError> {
        self.validate_crypto_binding(binding, &record.intent)?;
        let now_ms = self
            .clock
            .unix_millis()
            .map_err(|_| BrokerError::Unavailable)?;
        let dispatched = record.dispatch_started_ms.ok_or_else(|| {
            BrokerError::Rejected("cannot recover a never-dispatched order".into())
        })?;
        let digest = serde_json::to_vec(&record.intent)
            .map(|bytes| hex::encode(Sha256::digest(bytes)))
            .map_err(|_| BrokerError::Unavailable)?;
        if record.intent_digest != digest
            || record.revision == 0
            || dispatched < record.intent.created_ms
            || dispatched >= record.intent.expires_ms
            || dispatched > record.modified_ms
            || record.modified_ms > now_ms
            || matches!(
                record.phase,
                CryptoOrderPhase::Prepared | CryptoOrderPhase::ExpiredUnsent
            )
            || record
                .cancel_requested_ms
                .is_some_and(|at| at < dispatched || at > record.modified_ms)
        {
            return Err(BrokerError::Rejected(
                "invalid durable recovery record".into(),
            ));
        }
        let Some(current) = self.read_crypto_order(binding, &record.intent)? else {
            return Ok(None);
        };
        let current = current
            .validated_after(
                record,
                self.clock
                    .unix_millis()
                    .map_err(|_| BrokerError::Unavailable)?,
            )
            .map_err(|_| BrokerError::Unavailable)?;
        if current.state.is_terminal() || record.cancel_requested_ms.is_none() {
            return Ok(Some(current));
        }

        let path = format!("{V2_ORDERS_PATH}{}/cancel/", current.broker_order_id);
        let response = self.execute(HttpMethod::Post, path, Vec::new())?;
        let order = Self::write_response(response, 200)?;
        let received_ms = self
            .clock
            .unix_millis()
            .map_err(|_| BrokerError::AmbiguousOutcome)?;
        let canceled = self
            .normalize_crypto_order(order, &record.intent, received_ms)
            .map_err(|_| BrokerError::AmbiguousOutcome)?;
        // Validate against the fresh GET as well as the durable record. A successful
        // HTTP status cannot substitute a different order or discard a partial fill.
        let mut latest = record.clone();
        latest.observation = Some(current);
        canceled
            .validated_after(&latest, received_ms)
            .map(Some)
            .map_err(|_| BrokerError::AmbiguousOutcome)
    }

    fn normalize_crypto_order(
        &self,
        mut order: ApiOrder,
        intent: &CryptoOrderIntent,
        received_ms: u64,
    ) -> Result<CryptoOrderObservation, BrokerError> {
        let side = match order.side.as_str() {
            "buy" => CryptoOrderSide::Buy,
            "sell" => CryptoOrderSide::Sell,
            _ => return Err(BrokerError::Unavailable),
        };
        if order.symbol != intent.symbol
            || side != intent.side
            || order.order_type != "limit"
            || Self::accepted_at_ns(&order.created_at)? / 1_000_000 < intent.created_ms
        {
            return Err(BrokerError::Unavailable);
        }
        // These are broker-reported values, never copies filled in from the intent.
        // Missing fees are unknown, not zero. The older generic snapshot omits these fields.
        let limit = order
            .limit_order_config
            .take()
            .ok_or(BrokerError::Unavailable)?;
        // The API schema does not promise this response field. If provided, it must
        // agree with the fixed duration submitted by the v1 journal request contract.
        if limit
            .time_in_force
            .as_deref()
            .is_some_and(|value| value != "gtc")
        {
            return Err(BrokerError::Unavailable);
        }
        let ordered_quantity = limit
            .asset_quantity
            .ok_or(BrokerError::Unavailable)?
            .into_broker_decimal()?;
        let limit_price = limit
            .limit_price
            .ok_or(BrokerError::Unavailable)?
            .into_broker_decimal()?;
        let fee_usd = order
            .fee_charged
            .take()
            .ok_or(BrokerError::Unavailable)
            .and_then(ApiDecimal::into_broker_decimal)?;
        let snapshot = self.normalize_order(order)?;
        let executions = snapshot
            .executions
            .into_iter()
            .map(|fill| {
                Ok(CryptoOrderFill {
                    execution_id: fill.execution_id,
                    quantity: fill.quantity,
                    price: fill.effective_price,
                    occurred_ms: Self::accepted_at_ns(&fill.occurred_at)? / 1_000_000,
                })
            })
            .collect::<Result<_, BrokerError>>()?;
        CryptoOrderObservation {
            account_key: intent.account_key.clone(),
            client_order_id: snapshot.acknowledgement.client_order_id,
            broker_order_id: snapshot.acknowledgement.broker_order_id,
            symbol: intent.symbol.clone(),
            side,
            ordered_quantity,
            limit_price,
            state: snapshot.state,
            filled_quantity: snapshot.filled_quantity,
            fee_usd,
            executions,
            updated_ms: Self::accepted_at_ns(&snapshot.updated_at)? / 1_000_000,
            received_ms,
        }
        .validated_for(intent, received_ms)
        .map_err(|_| BrokerError::Unavailable)
    }
}
