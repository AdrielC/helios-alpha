//! Quantity-specific broker prices and instrument rules for exact, unsigned preparation.
//! These reads do not establish a strategy edge, authorize trading, or guarantee a fill.

use chrono::DateTime;
use helio_crypto::{
    ceil_crypto_notional_micros, CryptoAllocationBalance, CryptoMarketRules, CryptoMarketStatus,
    CryptoOrderIntent, CryptoPreparationContext, CryptoSignalValidity, CryptoValuedAccount,
    ExactCryptoQuote,
};
use helio_execution::{BrokerDecimal, BrokerError};
use serde::{Deserialize, Serialize};

use crate::crypto_account::{digest, units, MAX_AGE_MS};
use crate::{
    ApiDecimal, CryptoAccountAvailability, CryptoJournalBinding, CryptoLimitOrderDraft, HttpMethod,
    RequestSigner, RobinhoodCryptoBroker, RobinhoodTransport, UnixClock,
};

const PAIRS: &str = "/api/v2/crypto/trading/trading_pairs/";
const ESTIMATE: &str = "/api/v2/crypto/trading/estimated_price/";
const MAX_BYTES: usize = 1_048_576;
const SCALE: u128 = 1_000_000_000_000_000_000;

#[derive(Debug, Clone, Serialize)]
pub struct CryptoMarketAvailabilityData {
    pub schema_version: u32,
    pub execution_authority: bool,
    pub account_key: String,
    pub api_account_fingerprint: String,
    pub quantity: BrokerDecimal,
    pub rules: CryptoMarketRules,
    pub quote: ExactCryptoQuote,
    /// Highest currently reported ratio in this estimate, not a certified fee ceiling.
    pub reported_fee_bps: u32,
    pub broker_quote_observed_ms: u64,
    pub read_started_ms: u64,
    pub read_completed_ms: u64,
}

/// Only a complete successful read can construct this value. It cannot be deserialized.
#[derive(Debug, Clone, Serialize)]
pub struct CryptoMarketAvailability {
    snapshot: CryptoMarketAvailabilityData,
    snapshot_digest: String,
}

impl CryptoMarketAvailability {
    pub fn data(&self) -> &CryptoMarketAvailabilityData {
        &self.snapshot
    }
    pub fn snapshot_digest(&self) -> &str {
        &self.snapshot_digest
    }
}

pub struct CryptoPreparationReads<'a> {
    /// Must be freshly read from the authoritative journal. No cached balance fallback.
    pub allocation: &'a CryptoAllocationBalance,
    pub account: &'a CryptoAccountAvailability,
    pub market: &'a CryptoMarketAvailability,
}

#[derive(Deserialize)]
struct ApiPair {
    symbol: String,
    asset_code: String,
    quote_code: String,
    asset_increment: ApiDecimal,
    quote_increment: ApiDecimal,
    max_order_size: ApiDecimal,
    min_order_amount: ApiDecimal,
    status: String,
    is_api_tradable: bool,
}

#[derive(Deserialize)]
struct Estimates {
    results: Vec<Estimate>,
}

#[derive(Deserialize)]
struct Estimate {
    symbol: String,
    side: String,
    quantity: ApiDecimal,
    timestamp: String,
    bid: ApiDecimal,
    ask: ApiDecimal,
    fee_ratio: ApiDecimal,
}

impl<T: RobinhoodTransport, C: UnixClock, S: RequestSigner> RobinhoodCryptoBroker<T, C, S> {
    /// GET only. The v2 estimate includes order size and reports a broker timestamp.
    /// No market-maker v1 quote or reference-feed quote is silently substituted.
    pub fn read_crypto_market(
        &mut self,
        binding: &CryptoJournalBinding,
        symbol: &str,
        quantity: &BrokerDecimal,
    ) -> Result<CryptoMarketAvailability, BrokerError> {
        let account_key = binding
            .account_key_for(&self.config.account_number)?
            .to_owned();
        let requested_units = units(quantity)?;
        if !matches!(symbol, "BTC-USD" | "ETH-USD") || requested_units == 0 {
            return Err(BrokerError::Rejected("invalid market request".into()));
        }
        let started = self
            .clock
            .unix_millis()
            .map_err(|_| BrokerError::Unavailable)?;
        let mut pairs: Vec<ApiPair> = self.bounded_pages(PAIRS, Some(("symbol", symbol)))?;
        if pairs.len() != 1 {
            return Err(BrokerError::Unavailable);
        }
        let pair = pairs.pop().expect("one checked pair");
        if pair.symbol != symbol
            || pair.asset_code != symbol.trim_end_matches("-USD")
            || pair.quote_code != "USD"
        {
            return Err(BrokerError::Unavailable);
        }
        let asset_increment = pair.asset_increment.into_broker_decimal()?;
        let quote_increment = pair.quote_increment.into_broker_decimal()?;
        let max_order_quantity = pair.max_order_size.into_broker_decimal()?;
        let increment = units(&asset_increment)?;
        if increment == 0
            || units(&quote_increment)? == 0
            || units(&max_order_quantity)? < increment
            // The quantity-estimate endpoint requires quantity strictly below max_order_size.
            || requested_units >= units(&max_order_quantity)?
            || !requested_units.is_multiple_of(increment)
        {
            return Err(BrokerError::Rejected(
                "invalid broker quantity increment or maximum".into(),
            ));
        }
        let minimum = pair.min_order_amount.into_broker_decimal()?;
        let one = BrokerDecimal::try_new("1").expect("constant decimal");
        let rules = CryptoMarketRules {
            symbol: symbol.to_owned(),
            observed_ms: started,
            asset_increment,
            quote_increment,
            max_order_quantity,
            minimum_order_micros: ceil_crypto_notional_micros(&minimum, &one)
                .map_err(|_| BrokerError::Unavailable)?,
            status: match pair.status.as_str() {
                "tradable" => CryptoMarketStatus::Tradable,
                "sell_only" => CryptoMarketStatus::SellOnly,
                _ => CryptoMarketStatus::Unavailable,
            },
            api_tradable: pair.is_api_tradable,
        };
        if !rules.api_tradable || rules.status == CryptoMarketStatus::Unavailable {
            return Err(BrokerError::Rejected(
                "broker instrument unavailable".into(),
            ));
        }
        let query = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("symbol", symbol)
            .append_pair("side", "both")
            .append_pair("quantity", quantity.as_str())
            .finish();
        let response = self.execute(HttpMethod::Get, format!("{ESTIMATE}?{query}"), Vec::new())?;
        let body = Self::require_success(response, 200)?;
        if body.len() > MAX_BYTES {
            return Err(BrokerError::Unavailable);
        }
        let estimates: Estimates =
            serde_json::from_slice(&body).map_err(|_| BrokerError::Unavailable)?;
        if estimates.results.len() != 2 {
            return Err(BrokerError::Unavailable);
        }
        let mut bid = None::<(BrokerDecimal, u128)>;
        let mut ask = None::<(BrokerDecimal, u128)>;
        let mut seen_bid = false;
        let mut seen_ask = false;
        let mut broker_stamp = u64::MAX;
        let mut newest_broker_stamp = 0;
        let mut fee_bps = 0;
        for value in estimates.results {
            match value.side.as_str() {
                "bid" if !seen_bid => seen_bid = true,
                "ask" if !seen_ask => seen_ask = true,
                _ => return Err(BrokerError::Unavailable),
            }
            if value.symbol != symbol
                || units(&value.quantity.into_broker_decimal()?)? != requested_units
            {
                return Err(BrokerError::Unavailable);
            }
            let stamp: u64 = DateTime::parse_from_rfc3339(&value.timestamp)
                .map_err(|_| BrokerError::Unavailable)?
                .timestamp_millis()
                .try_into()
                .map_err(|_| BrokerError::Unavailable)?;
            broker_stamp = broker_stamp.min(stamp);
            newest_broker_stamp = newest_broker_stamp.max(stamp);
            let row_bid = value.bid.into_broker_decimal()?;
            let row_ask = value.ask.into_broker_decimal()?;
            let bid_units = units(&row_bid)?;
            let ask_units = units(&row_ask)?;
            if bid_units == 0 || ask_units < bid_units {
                return Err(BrokerError::Unavailable);
            }
            if bid.as_ref().is_none_or(|(_, v)| *v > bid_units) {
                bid = Some((row_bid, bid_units));
            }
            if ask.as_ref().is_none_or(|(_, v)| *v < ask_units) {
                ask = Some((row_ask, ask_units));
            }
            let ratio = units(&value.fee_ratio.into_broker_decimal()?)?;
            if ratio > SCALE {
                return Err(BrokerError::Unavailable);
            }
            fee_bps = fee_bps.max(
                u32::try_from((ratio * 10_000).div_ceil(SCALE))
                    .map_err(|_| BrokerError::Unavailable)?,
            );
        }
        let completed = self
            .clock
            .unix_millis()
            .map_err(|_| BrokerError::Unavailable)?;
        if completed < started
            || completed - started >= MAX_AGE_MS
            || newest_broker_stamp > completed
            || completed - broker_stamp >= MAX_AGE_MS
        {
            return Err(BrokerError::Unavailable);
        }
        let snapshot = CryptoMarketAvailabilityData {
            schema_version: 1,
            execution_authority: false,
            account_key: account_key.clone(),
            api_account_fingerprint: digest(&(
                "robinhood-crypto-api-v2",
                &self.config.account_number,
            ))?,
            quantity: quantity.clone(),
            rules,
            quote: ExactCryptoQuote {
                account_key,
                symbol: symbol.to_owned(),
                bid: bid.ok_or(BrokerError::Unavailable)?.0,
                ask: ask.ok_or(BrokerError::Unavailable)?.0,
                // The oldest broker/receipt time owns the freshness deadline.
                observed_ms: broker_stamp.min(started),
            },
            reported_fee_bps: fee_bps,
            broker_quote_observed_ms: broker_stamp,
            read_started_ms: started,
            read_completed_ms: completed,
        };
        Ok(CryptoMarketAvailability {
            snapshot_digest: digest(&snapshot)?,
            snapshot,
        })
    }

    /// Rechecks broker reads and allocation before invoking the complete exact-order kernel.
    /// The supplied context still requires authoritative durable valuation, daily/peak equity,
    /// order counts, pause/loss latches, and an independently verified worst-fee/settlement bound.
    /// This method does not create those facts or replace independent execution admission.
    pub fn prepare_crypto_limit_order_from_reads(
        &self,
        binding: &CryptoJournalBinding,
        intent: CryptoOrderIntent,
        context: &CryptoPreparationContext,
        reads: CryptoPreparationReads<'_>,
        now_ms: u64,
    ) -> Result<CryptoLimitOrderDraft, BrokerError> {
        let key = binding.account_key_for(&self.config.account_number)?;
        let account = reads.account.data();
        let market = reads.market.data();
        let allocation = reads.allocation;
        let max_age = context.policy.limits.max_quote_age_ms.min(MAX_AGE_MS);
        let fingerprint = digest(&("robinhood-crypto-api-v2", &self.config.account_number))?;
        if account.account_key != key
            || account.api_account_fingerprint != fingerprint
            || market.account_key != key
            || market.api_account_fingerprint != fingerprint
            || market.rules.symbol != intent.symbol
            || units(&market.quantity)? != units(&intent.quantity)?
            || context.account.account_key != key
            || context.account.snapshot_digest != allocation.ledger_digest
            || context.account.observed_ms < allocation.observed_ms
            || now_ms < market.read_completed_ms
            || now_ms.saturating_sub(market.quote.observed_ms) >= max_age
            || context.account.fee_reserve_bps < market.reported_fee_bps
            || context.account.fee_reserve_bps
                < account.reported_fee_bps.ok_or(BrokerError::Unavailable)?
        {
            return Err(BrokerError::Rejected(
                "broker preparation inputs do not match current evidence".into(),
            ));
        }
        let caps = reads.account.cap_allocation(allocation, now_ms, max_age)?;
        let mut checked = context.clone();
        checked.quote = market.quote.clone();
        checked.rules = market.rules.clone();
        checked.account.buying_power_micros = checked
            .account
            .buying_power_micros
            .min(caps.new_buying_power_micros);
        let zero = BrokerDecimal::try_new("0").expect("constant decimal");
        let available = caps
            .sellable_quantities
            .get(&intent.symbol)
            .unwrap_or(&zero);
        if units(available)? < units(&checked.account.available_quantity)? {
            checked.account.available_quantity = available.clone();
        }
        checked.account.unresolved_orders |= allocation.unresolved_orders;
        checked.account.loss_stop_latched |=
            allocation.cash_deficit_micros != 0 || !allocation.quantity_deficits.is_empty();
        checked.account.observed_ms = checked.account.observed_ms.min(account.read_started_ms);
        checked.account.snapshot_digest = digest(&(
            &context.account,
            &allocation.ledger_digest,
            reads.account.snapshot_digest(),
            reads.market.snapshot_digest(),
        ))?;
        let deadline = account
            .read_started_ms
            .min(market.quote.observed_ms)
            .checked_add(max_age)
            .ok_or(BrokerError::Unavailable)?;
        self.prepare_crypto_limit_order(binding, intent, &checked, now_ms)?
            .restrict_valid_until(deadline)
    }

    /// Compose a durable Golem valuation with fresh native broker reads. This prepares an
    /// unsigned limit order only; current controller policy/pause and admission are separate.
    pub fn prepare_crypto_limit_order_from_valuation(
        &self,
        binding: &CryptoJournalBinding,
        intent: CryptoOrderIntent,
        valuation: &CryptoValuedAccount,
        signal: &CryptoSignalValidity,
        reads: CryptoPreparationReads<'_>,
        now_ms: u64,
    ) -> Result<CryptoLimitOrderDraft, BrokerError> {
        if valuation.execution_authority
            || now_ms >= valuation.valid_until_ms
            || valuation.account.symbol != intent.symbol
            || reads
                .allocation
                .snapshot_digest()
                .map_err(|_| BrokerError::Unavailable)?
                != valuation.balance_digest
        {
            return Err(BrokerError::Rejected(
                "allocation valuation changed or expired".into(),
            ));
        }
        if let Some(scope) = &valuation.prepared_order {
            if scope.client_order_id != intent.client_order_id
                || scope.intent_digest
                    != intent
                        .snapshot_digest()
                        .map_err(|_| BrokerError::Unavailable)?
                || scope.revision == 0
            {
                return Err(BrokerError::Rejected(
                    "valuation belongs to another prepared intent".into(),
                ));
            }
        }
        let context = CryptoPreparationContext {
            policy: valuation.policy.clone(),
            account: valuation.account.clone(),
            quote: reads.market.data().quote.clone(),
            rules: reads.market.data().rules.clone(),
            signal: signal.clone(),
        };
        self.prepare_crypto_limit_order_from_reads(binding, intent, &context, reads, now_ms)?
            .restrict_valid_until(valuation.valid_until_ms)
    }
}
