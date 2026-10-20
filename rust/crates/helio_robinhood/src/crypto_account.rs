//! Bounded v2 account/holding reads. Broker availability caps a strategy allocation;
//! it never adopts unrelated holdings or proves that a particular fill has settled.

use std::collections::{BTreeMap, BTreeSet};

use helio_crypto::{floor_crypto_notional_micros, CryptoAllocationBalance};
use helio_execution::{BrokerDecimal, BrokerError};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::Url;

use crate::{
    required_next, ApiDecimal, CryptoJournalBinding, HttpMethod, RequestSigner,
    RobinhoodCryptoBroker, RobinhoodTransport, UnixClock, ROBINHOOD_CRYPTO_API_ORIGIN,
};

const ACCOUNTS: &str = "/api/v2/crypto/trading/accounts/";
const HOLDINGS: &str = "/api/v2/crypto/trading/holdings/";
const MAX_PAGES: usize = 4;
const MAX_ROWS: usize = 256;
const MAX_BYTES: usize = 1_048_576;
pub(crate) const MAX_AGE_MS: u64 = 5000;
const SCALE: u128 = 1_000_000_000_000_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CryptoApiAccountStatus {
    Active,
    SellOnly,
    Deactivated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CryptoBrokerHolding {
    pub total_quantity: BrokerDecimal,
    pub available_quantity: BrokerDecimal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CryptoAccountAvailabilityData {
    pub schema_version: u32,
    pub execution_authority: bool,
    pub account_key: String,
    pub api_account_fingerprint: String,
    pub status: CryptoApiAccountStatus,
    pub api_tradable: bool,
    pub buying_power_usd: BrokerDecimal,
    /// The API's current ratio, not an assumed maker discount or certified fee ceiling.
    pub reported_fee_ratio: Option<BrokerDecimal>,
    pub reported_fee_bps: Option<u32>,
    pub holdings: BTreeMap<String, CryptoBrokerHolding>,
    /// Client receipt window, not broker settlement timestamps.
    pub read_started_ms: u64,
    pub read_completed_ms: u64,
}

/// Constructed only by a successful, complete read. No Deserialize or public constructor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CryptoAccountAvailability {
    snapshot: CryptoAccountAvailabilityData,
    snapshot_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CryptoBrokerAllocationCaps {
    pub execution_authority: bool,
    pub account_key: String,
    pub broker_snapshot_digest: String,
    pub allocation_digest: String,
    pub broker_status: CryptoApiAccountStatus,
    pub api_tradable: bool,
    pub reported_fee_bps: Option<u32>,
    pub new_buying_power_micros: u64,
    pub sellable_quantities: BTreeMap<String, BrokerDecimal>,
    pub read_started_ms: u64,
    pub read_completed_ms: u64,
}

#[derive(Deserialize)]
struct Page<T> {
    #[serde(deserialize_with = "required_next")]
    next: Option<String>,
    results: Vec<T>,
}

#[derive(Deserialize)]
struct ApiAccount {
    account_number: String,
    status: CryptoApiAccountStatus,
    buying_power: ApiDecimal,
    buying_power_currency: String,
    is_api_tradable: bool,
    #[serde(default)]
    fee_tier_status: Option<ApiFee>,
}

#[derive(Deserialize)]
struct ApiFee {
    fee_ratio: ApiDecimal,
}

#[derive(Deserialize)]
struct ApiHolding {
    account_number: String,
    asset_code: String,
    total_quantity: ApiDecimal,
    quantity_available_for_trading: ApiDecimal,
}

// BrokerDecimal's derived ordering is lexical; account comparisons must be numeric.
pub(crate) fn units(value: &BrokerDecimal) -> Result<u128, BrokerError> {
    let (whole, fraction) = value
        .as_str()
        .split_once('.')
        .unwrap_or((value.as_str(), ""));
    if whole.len() > 18 || fraction.len() > 18 {
        return Err(BrokerError::Unavailable);
    }
    let whole: u128 = whole.parse().map_err(|_| BrokerError::Unavailable)?;
    let fraction = if fraction.is_empty() {
        0
    } else {
        fraction
            .parse::<u128>()
            .map_err(|_| BrokerError::Unavailable)?
            * 10_u128.pow(18 - fraction.len() as u32)
    };
    Ok(whole * SCALE + fraction)
}

pub(crate) fn digest(value: &impl Serialize) -> Result<String, BrokerError> {
    // Sort object keys through Value so other local consumers can verify the digest.
    serde_json::to_value(value)
        .and_then(|value| serde_json::to_vec(&value))
        .map(|bytes| hex::encode(Sha256::digest(bytes)))
        .map_err(|_| BrokerError::Unavailable)
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn valid_account(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && value.bytes().all(|b| b.is_ascii_alphanumeric())
}

fn next_path(
    next: &str,
    endpoint: &str,
    scope: Option<(&str, &str)>,
) -> Result<String, BrokerError> {
    if next.len() > 4096 || next.chars().any(char::is_control) {
        return Err(BrokerError::Unavailable);
    }
    let origin = Url::parse(ROBINHOOD_CRYPTO_API_ORIGIN).expect("constant origin");
    let url = origin.join(next).map_err(|_| BrokerError::Unavailable)?;
    if url.origin() != origin.origin()
        || url.path() != endpoint
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(BrokerError::Unavailable);
    }
    let (mut cursor, mut limit, mut scope_seen) = (None, None, false);
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "cursor" if cursor.is_none() && !value.is_empty() && value.len() <= 2048 => {
                cursor = Some(value.into_owned());
            }
            name if !scope_seen && scope == Some((name, value.as_ref())) => {
                scope_seen = true;
            }
            "limit"
                if limit.is_none()
                    && value.bytes().all(|b| b.is_ascii_digit())
                    && value
                        .parse::<usize>()
                        .is_ok_and(|n| (1..=MAX_ROWS).contains(&n)) =>
            {
                limit = Some(value.into_owned());
            }
            _ => return Err(BrokerError::Unavailable),
        }
    }
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    if let Some((name, value)) = scope {
        query.append_pair(name, value);
    }
    query.append_pair("cursor", &cursor.ok_or(BrokerError::Unavailable)?);
    if let Some(limit) = limit {
        query.append_pair("limit", &limit);
    }
    Ok(format!("{endpoint}?{}", query.finish()))
}

impl<T: RobinhoodTransport, C: UnixClock, S: RequestSigner> RobinhoodCryptoBroker<T, C, S> {
    pub(crate) fn bounded_pages<Row: DeserializeOwned>(
        &mut self,
        endpoint: &str,
        scope: Option<(&str, &str)>,
    ) -> Result<Vec<Row>, BrokerError> {
        let mut path = match scope {
            Some((name, value)) => {
                let query = url::form_urlencoded::Serializer::new(String::new())
                    .append_pair(name, value)
                    .finish();
                format!("{endpoint}?{query}")
            }
            None => endpoint.to_owned(),
        };
        let mut seen = BTreeSet::new();
        let mut rows = Vec::new();
        for index in 0..self.config.max_reconciliation_pages.min(MAX_PAGES) {
            if !seen.insert(path.clone()) {
                return Err(BrokerError::Unavailable);
            }
            let response = self.execute(HttpMethod::Get, path, Vec::new())?;
            let body = Self::require_success(response, 200)?;
            if body.len() > MAX_BYTES {
                return Err(BrokerError::Unavailable);
            }
            let page: Page<Row> =
                serde_json::from_slice(&body).map_err(|_| BrokerError::Unavailable)?;
            if rows.len() + page.results.len() > MAX_ROWS {
                return Err(BrokerError::Unavailable);
            }
            rows.extend(page.results);
            match page.next {
                None => return Ok(rows),
                Some(next) if index + 1 < self.config.max_reconciliation_pages.min(MAX_PAGES) => {
                    path = next_path(&next, endpoint, scope)?;
                }
                Some(_) => return Err(BrokerError::Unavailable),
            }
        }
        Err(BrokerError::Unavailable)
    }

    /// GET only, exact configured account, complete bounded pagination, no credential discovery.
    pub fn read_crypto_account(
        &mut self,
        binding: &CryptoJournalBinding,
    ) -> Result<CryptoAccountAvailability, BrokerError> {
        let account_key = binding
            .account_key_for(&self.config.account_number)?
            .to_owned();
        let started = self
            .clock
            .unix_millis()
            .map_err(|_| BrokerError::Unavailable)?;
        let accounts: Vec<ApiAccount> = self.bounded_pages(ACCOUNTS, None)?;
        let mut seen = BTreeSet::new();
        let mut selected = None;
        for account in accounts {
            if !valid_account(&account.account_number)
                || !seen.insert(account.account_number.clone())
            {
                return Err(BrokerError::Unavailable);
            }
            if account.account_number == self.config.account_number {
                selected = Some(account);
            }
        }
        let account = selected.ok_or(BrokerError::Unavailable)?;
        if account.buying_power_currency != "USD" {
            return Err(BrokerError::Unavailable);
        }
        let buying_power = account.buying_power.into_broker_decimal()?;
        units(&buying_power)?;
        let fee_ratio = account
            .fee_tier_status
            .map(|fee| fee.fee_ratio.into_broker_decimal())
            .transpose()?;
        let fee_bps = fee_ratio
            .as_ref()
            .map(|ratio| {
                let scaled = units(ratio)?;
                if scaled > SCALE {
                    return Err(BrokerError::Unavailable);
                }
                u32::try_from((scaled * 10_000).div_ceil(SCALE))
                    .map_err(|_| BrokerError::Unavailable)
            })
            .transpose()?;
        let configured_account = self.config.account_number.clone();
        let rows: Vec<ApiHolding> =
            self.bounded_pages(HOLDINGS, Some(("account_number", &configured_account)))?;
        let mut holdings = BTreeMap::new();
        for row in rows {
            if row.account_number != configured_account
                || row.asset_code.is_empty()
                || row.asset_code.len() > 20
                || !row
                    .asset_code
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
                || holdings.contains_key(&row.asset_code)
            {
                return Err(BrokerError::Unavailable);
            }
            let total = row.total_quantity.into_broker_decimal()?;
            let available = row.quantity_available_for_trading.into_broker_decimal()?;
            if units(&available)? > units(&total)? {
                return Err(BrokerError::Unavailable);
            }
            holdings.insert(
                row.asset_code,
                CryptoBrokerHolding {
                    total_quantity: total,
                    available_quantity: available,
                },
            );
        }
        let completed = self
            .clock
            .unix_millis()
            .map_err(|_| BrokerError::Unavailable)?;
        if completed < started || completed - started > MAX_AGE_MS {
            return Err(BrokerError::Unavailable);
        }
        let snapshot = CryptoAccountAvailabilityData {
            schema_version: 1,
            execution_authority: false,
            account_key,
            api_account_fingerprint: digest(&("robinhood-crypto-api-v2", &configured_account))?,
            status: account.status,
            api_tradable: account.is_api_tradable,
            buying_power_usd: buying_power,
            reported_fee_ratio: fee_ratio,
            reported_fee_bps: fee_bps,
            holdings,
            read_started_ms: started,
            read_completed_ms: completed,
        };
        Ok(CryptoAccountAvailability {
            snapshot_digest: digest(&snapshot)?,
            snapshot,
        })
    }
}

impl CryptoAccountAvailability {
    pub fn data(&self) -> &CryptoAccountAvailabilityData {
        &self.snapshot
    }
    pub fn snapshot_digest(&self) -> &str {
        &self.snapshot_digest
    }

    /// Intersect broker availability with a freshly read Golem allocation. This is not a
    /// complete risk/preparation context: valuations, loss limits and certified fee bounds
    /// must still be supplied and checked at admission and again before dispatch.
    pub fn cap_allocation(
        &self,
        allocation: &CryptoAllocationBalance,
        now_ms: u64,
        max_age_ms: u64,
    ) -> Result<CryptoBrokerAllocationCaps, BrokerError> {
        let data = &self.snapshot;
        if allocation.execution_authority
            || allocation.account_key != data.account_key
            || !valid_digest(&allocation.ledger_digest)
            || allocation.observed_ms > now_ms
            || allocation.available_cash_micros > allocation.cash_micros
            || max_age_ms == 0
            || max_age_ms > MAX_AGE_MS
            || now_ms < data.read_completed_ms
            || now_ms.saturating_sub(data.read_started_ms) >= max_age_ms
        {
            return Err(BrokerError::Unavailable);
        }
        let one = BrokerDecimal::try_new("1").expect("constant decimal");
        let broker_cash = floor_crypto_notional_micros(&data.buying_power_usd, &one)
            .map_err(|_| BrokerError::Unavailable)?;
        let new_buying_power_micros = if data.api_tradable
            && data.status == CryptoApiAccountStatus::Active
            && data.reported_fee_bps.is_some()
            && !allocation.unresolved_orders
            && allocation.cash_deficit_micros == 0
            && allocation.quantity_deficits.is_empty()
        {
            allocation.available_cash_micros.min(broker_cash)
        } else {
            0
        };
        let mut sellable = BTreeMap::new();
        for (symbol, amount) in &allocation.available_quantities {
            let code = symbol
                .strip_suffix("-USD")
                .ok_or(BrokerError::Unavailable)?;
            let owned = allocation
                .quantities
                .get(symbol)
                .ok_or(BrokerError::Unavailable)?;
            if units(amount)? > units(owned)? {
                return Err(BrokerError::Unavailable);
            }
            if data.api_tradable
                && data.status != CryptoApiAccountStatus::Deactivated
                && !allocation.unresolved_orders
            {
                if let Some(holding) = data.holdings.get(code) {
                    let capped = if units(amount)? <= units(&holding.available_quantity)? {
                        amount.clone()
                    } else {
                        holding.available_quantity.clone()
                    };
                    if units(&capped)? > 0 {
                        sellable.insert(symbol.clone(), capped);
                    }
                }
            }
        }
        Ok(CryptoBrokerAllocationCaps {
            execution_authority: false,
            account_key: data.account_key.clone(),
            broker_snapshot_digest: self.snapshot_digest.clone(),
            allocation_digest: allocation.ledger_digest.clone(),
            broker_status: data.status,
            api_tradable: data.api_tradable,
            reported_fee_bps: data.reported_fee_bps,
            new_buying_power_micros,
            sellable_quantities: sellable,
            read_started_ms: data.read_started_ms,
            read_completed_ms: data.read_completed_ms,
        })
    }
}
