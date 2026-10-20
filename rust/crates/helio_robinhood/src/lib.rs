//! Official Robinhood integration boundaries for Helios.
//!
//! Robinhood currently exposes two materially different surfaces:
//!
//! - The Crypto Trading API is a signed REST API. [`RobinhoodCryptoBroker`] implements the Helios
//!   broker ports against its v2 order endpoints through an injected transport.
//! - Robinhood Agentic Trading exposes equities, options, and crypto through an authenticated MCP
//!   server. That surface is intentionally not impersonated here: its order identity, error, and
//!   reconciliation contracts must be certified before it can back [`helio_execution::BrokerPort`].
//!
//! This crate never reads credentials from the environment and never logs secret key material.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use chrono::DateTime;
use ed25519_dalek::{Signer, SigningKey};
use helio_execution::{
    checked_notional, BrokerAcknowledgement, BrokerDecimal, BrokerError, BrokerExecution,
    BrokerLifecyclePort, BrokerOrderSnapshot, BrokerOrderState, BrokerPort, ExecutionMode,
    OrderIntent, Side,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use url::Url;
use uuid::Uuid;
use zeroize::Zeroize;

pub const ROBINHOOD_CRYPTO_API_ORIGIN: &str = "https://trading.robinhood.com";
pub const ROBINHOOD_AGENTIC_MCP_URL: &str = "https://agent.robinhood.com/mcp/trading";
const V2_ORDERS_PATH: &str = "/api/v2/crypto/trading/orders/";
const MAX_ORDER_RESPONSE_BYTES: usize = 1_048_576;

mod crypto_journal;
pub use crypto_journal::{CryptoJournalBinding, CryptoLimitOrderDraft};
mod crypto_account;
pub use crypto_account::{
    CryptoAccountAvailability, CryptoAccountAvailabilityData, CryptoApiAccountStatus,
    CryptoBrokerAllocationCaps, CryptoBrokerHolding,
};
mod crypto_market;
pub use crypto_market::{
    CryptoMarketAvailability, CryptoMarketAvailabilityData, CryptoPreparationReads,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
}

impl HttpMethod {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: HttpMethod,
    /// Absolute path and query. This exact value is signed and sent.
    pub path_and_query: String,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

impl fmt::Debug for HttpRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HttpRequest")
            .field("method", &self.method)
            .field("path_and_query", &"[redacted]")
            .field("header_names", &self.headers.keys().collect::<Vec<_>>())
            .field("body_bytes", &self.body.len())
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum TransportError {
    #[error("Robinhood transport is unavailable")]
    Unavailable,
    #[error("Robinhood request outcome is unknown")]
    OutcomeUnknown,
    #[error("Robinhood response exceeded the configured byte limit")]
    ResponseTooLarge,
}

pub trait RobinhoodTransport {
    fn execute(&mut self, request: HttpRequest) -> Result<HttpResponse, TransportError>;
}

pub trait UnixClock {
    fn unix_seconds(&mut self) -> Result<u64, ClockError>;

    /// Receipt timestamps need millisecond precision. Existing clocks can use the
    /// conservative whole-second default; native clocks override it.
    fn unix_millis(&mut self) -> Result<u64, ClockError> {
        self.unix_seconds()?
            .checked_mul(1000)
            .ok_or(ClockError::OutOfRange)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ClockError {
    #[error("system clock is before the Unix epoch")]
    BeforeUnixEpoch,
    #[error("clock is outside the supported timestamp range")]
    OutOfRange,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemUnixClock;

impl UnixClock for SystemUnixClock {
    fn unix_seconds(&mut self) -> Result<u64, ClockError> {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .map_err(|_| ClockError::BeforeUnixEpoch)
    }

    fn unix_millis(&mut self) -> Result<u64, ClockError> {
        let duration = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| ClockError::BeforeUnixEpoch)?;
        u64::try_from(duration.as_millis()).map_err(|_| ClockError::OutOfRange)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct SignedHeaders {
    pub api_key: String,
    pub signature: String,
    pub timestamp: String,
}

impl fmt::Debug for SignedHeaders {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SignedHeaders")
            .field("api_key", &"[redacted]")
            .field("signature", &"[redacted]")
            .field("timestamp", &self.timestamp)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SigningError {
    #[error("Robinhood API key must be non-empty and contain no control characters")]
    InvalidApiKey,
    #[error("Robinhood private key must be base64-encoded raw Ed25519 key bytes")]
    InvalidPrivateKey,
}

pub trait RequestSigner {
    fn sign(
        &self,
        timestamp: u64,
        path_and_query: &str,
        method: HttpMethod,
        body: &[u8],
    ) -> Result<SignedHeaders, SigningError>;
}

/// Ed25519 request signer following Robinhood's documented canonical message:
/// `api_key + timestamp + path + method + body`.
pub struct Ed25519RequestSigner {
    api_key: String,
    signing_key: SigningKey,
}

impl fmt::Debug for Ed25519RequestSigner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Ed25519RequestSigner")
            .field("api_key", &"[redacted]")
            .field("signing_key", &"[redacted]")
            .finish()
    }
}

impl Ed25519RequestSigner {
    pub fn from_base64(
        api_key: impl Into<String>,
        private_key_base64: &str,
    ) -> Result<Self, SigningError> {
        let api_key = api_key.into();
        if api_key.is_empty() || api_key.chars().any(char::is_control) {
            return Err(SigningError::InvalidApiKey);
        }
        let decoded = BASE64_STANDARD
            .decode(private_key_base64)
            .map_err(|_| SigningError::InvalidPrivateKey)?;
        let mut key_bytes: [u8; 32] = decoded
            .try_into()
            .map_err(|_| SigningError::InvalidPrivateKey)?;
        let signing_key = SigningKey::from_bytes(&key_bytes);
        key_bytes.zeroize();
        Ok(Self {
            api_key,
            signing_key,
        })
    }
}

impl RequestSigner for Ed25519RequestSigner {
    fn sign(
        &self,
        timestamp: u64,
        path_and_query: &str,
        method: HttpMethod,
        body: &[u8],
    ) -> Result<SignedHeaders, SigningError> {
        let timestamp = timestamp.to_string();
        let mut message = Vec::with_capacity(
            self.api_key.len()
                + timestamp.len()
                + path_and_query.len()
                + method.as_str().len()
                + body.len(),
        );
        message.extend_from_slice(self.api_key.as_bytes());
        message.extend_from_slice(timestamp.as_bytes());
        message.extend_from_slice(path_and_query.as_bytes());
        message.extend_from_slice(method.as_str().as_bytes());
        message.extend_from_slice(body);
        let signature = self.signing_key.sign(&message);
        Ok(SignedHeaders {
            api_key: self.api_key.clone(),
            signature: BASE64_STANDARD.encode(signature.to_bytes()),
            timestamp,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeInForce {
    GoodTilCanceled,
    GoodForDay,
    GoodForWeek,
    GoodForMonth,
}

impl TimeInForce {
    const fn as_api_str(self) -> &'static str {
        match self {
            Self::GoodTilCanceled => "gtc",
            Self::GoodForDay => "gfd",
            Self::GoodForWeek => "gfw",
            Self::GoodForMonth => "gfm",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ConfigurationError {
    #[error("Robinhood account number must be non-empty ASCII alphanumeric text")]
    InvalidAccountNumber,
    #[error("Robinhood venue identifier must be non-empty")]
    InvalidVenue,
    #[error("reconciliation page bound must be positive")]
    ZeroReconciliationPages,
}

#[derive(Clone, PartialEq, Eq)]
pub struct RobinhoodCryptoConfig {
    pub account_number: String,
    pub venue: String,
    pub time_in_force: TimeInForce,
    pub max_reconciliation_pages: usize,
}

impl fmt::Debug for RobinhoodCryptoConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RobinhoodCryptoConfig")
            .field("account_number", &"[redacted]")
            .field("venue", &self.venue)
            .field("time_in_force", &self.time_in_force)
            .field("max_reconciliation_pages", &self.max_reconciliation_pages)
            .finish()
    }
}

impl RobinhoodCryptoConfig {
    pub fn try_new(account_number: impl Into<String>) -> Result<Self, ConfigurationError> {
        let account_number = account_number.into();
        if account_number.is_empty()
            || !account_number
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric())
        {
            return Err(ConfigurationError::InvalidAccountNumber);
        }
        Ok(Self {
            account_number,
            venue: "ROBINHOOD_CRYPTO".to_owned(),
            time_in_force: TimeInForce::GoodTilCanceled,
            max_reconciliation_pages: 4,
        })
    }

    pub fn validate(&self) -> Result<(), ConfigurationError> {
        if self.account_number.is_empty()
            || !self
                .account_number
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric())
        {
            return Err(ConfigurationError::InvalidAccountNumber);
        }
        if self.venue.trim().is_empty() {
            return Err(ConfigurationError::InvalidVenue);
        }
        if self.max_reconciliation_pages == 0 {
            return Err(ConfigurationError::ZeroReconciliationPages);
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
struct PlaceLimitOrder<'a> {
    symbol: &'a str,
    client_order_id: &'a str,
    side: &'static str,
    #[serde(rename = "type")]
    order_type: &'static str,
    limit_order_config: LimitOrderConfig,
}

#[derive(Debug, Serialize)]
struct LimitOrderConfig {
    asset_quantity: String,
    limit_price: String,
    time_in_force: &'static str,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(transparent)]
struct ApiDecimal(Box<serde_json::value::RawValue>);

impl ApiDecimal {
    fn into_broker_decimal(self) -> Result<BrokerDecimal, BrokerError> {
        let raw = self.0.get();
        // Parse only this scalar from raw JSON. A workspace-wide arbitrary-precision
        // feature changes serde's untagged-enum behavior in unrelated consumers.
        let value = if raw.starts_with('"') {
            serde_json::from_str::<String>(raw).map_err(|_| BrokerError::Unavailable)?
        } else {
            expand_json_decimal(raw)?
        };
        if value.len() > 128 {
            return Err(BrokerError::Unavailable);
        }
        BrokerDecimal::try_new(value)
            .map_err(|_| BrokerError::Rejected("broker returned an invalid decimal".to_owned()))
    }
}

fn expand_json_decimal(value: &str) -> Result<String, BrokerError> {
    if value.len() > 128 || value.starts_with('-') {
        return Err(BrokerError::Unavailable);
    }
    let Some((mantissa, exponent)) = value.split_once(['e', 'E']) else {
        return Ok(value.to_owned());
    };
    let exponent: i32 = exponent.parse().map_err(|_| BrokerError::Unavailable)?;
    let whole_len = mantissa.find('.').unwrap_or(mantissa.len()) as i32;
    let digits = mantissa.replace('.', "");
    let point = whole_len
        .checked_add(exponent)
        .ok_or(BrokerError::Unavailable)?;
    if !(-126..=128).contains(&point) {
        return Err(BrokerError::Unavailable);
    }
    let result = if point <= 0 {
        format!("0.{}{}", "0".repeat((-point) as usize), digits)
    } else if point as usize >= digits.len() {
        format!("{digits}{}", "0".repeat(point as usize - digits.len()))
    } else {
        format!(
            "{}.{}",
            &digits[..point as usize],
            &digits[point as usize..]
        )
    };
    if result.len() > 128 {
        return Err(BrokerError::Unavailable);
    }
    Ok(result)
}

#[derive(Debug, Clone, Deserialize)]
struct ApiExecution {
    effective_price: ApiDecimal,
    quantity: ApiDecimal,
    timestamp: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ApiLimitOrderConfig {
    #[serde(default)]
    asset_quantity: Option<ApiDecimal>,
    #[serde(default)]
    limit_price: Option<ApiDecimal>,
    #[serde(default)]
    time_in_force: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ApiOrder {
    id: String,
    account_number: String,
    client_order_id: String,
    symbol: String,
    side: String,
    #[serde(rename = "type")]
    order_type: String,
    state: String,
    executions: Vec<ApiExecution>,
    filled_asset_quantity: ApiDecimal,
    #[serde(default)]
    average_price: Option<ApiDecimal>,
    #[serde(default)]
    limit_order_config: Option<ApiLimitOrderConfig>,
    #[serde(default)]
    fee_charged: Option<ApiDecimal>,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Deserialize)]
struct ApiOrdersPage {
    #[serde(deserialize_with = "required_next")]
    next: Option<String>,
    results: Vec<ApiOrder>,
}

fn required_next<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    Option::<String>::deserialize(deserializer)
}

pub struct RobinhoodCryptoBroker<T, C, S> {
    config: RobinhoodCryptoConfig,
    transport: T,
    clock: C,
    signer: S,
}

impl<T, C, S> fmt::Debug for RobinhoodCryptoBroker<T, C, S>
where
    T: fmt::Debug,
    C: fmt::Debug,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RobinhoodCryptoBroker")
            .field("config", &self.config)
            .field("transport", &self.transport)
            .field("clock", &self.clock)
            .field("signer", &"[redacted]")
            .finish()
    }
}

impl<T, C, S> RobinhoodCryptoBroker<T, C, S>
where
    T: RobinhoodTransport,
    C: UnixClock,
    S: RequestSigner,
{
    pub fn try_new(
        config: RobinhoodCryptoConfig,
        transport: T,
        clock: C,
        signer: S,
    ) -> Result<Self, ConfigurationError> {
        config.validate()?;
        Ok(Self {
            config,
            transport,
            clock,
            signer,
        })
    }

    pub const fn config(&self) -> &RobinhoodCryptoConfig {
        &self.config
    }

    pub const fn transport(&self) -> &T {
        &self.transport
    }

    fn signed_request(
        &mut self,
        method: HttpMethod,
        path_and_query: String,
        body: Vec<u8>,
    ) -> Result<HttpRequest, BrokerError> {
        let timestamp = self
            .clock
            .unix_seconds()
            .map_err(|_| BrokerError::Unavailable)?;
        let signed = self
            .signer
            .sign(timestamp, &path_and_query, method, &body)
            .map_err(|_| BrokerError::Rejected("Robinhood request signing failed".to_owned()))?;
        let mut headers = BTreeMap::from([
            ("content-type".to_owned(), "application/json".to_owned()),
            ("x-api-key".to_owned(), signed.api_key),
            ("x-signature".to_owned(), signed.signature),
            ("x-timestamp".to_owned(), signed.timestamp),
        ]);
        headers.insert("accept".to_owned(), "application/json".to_owned());
        Ok(HttpRequest {
            method,
            path_and_query,
            headers,
            body,
        })
    }

    fn execute(
        &mut self,
        method: HttpMethod,
        path_and_query: String,
        body: Vec<u8>,
    ) -> Result<HttpResponse, BrokerError> {
        let request = self.signed_request(method, path_and_query, body)?;
        self.transport
            .execute(request)
            .map_err(|error| match error {
                _ if method == HttpMethod::Post => BrokerError::AmbiguousOutcome,
                TransportError::Unavailable
                | TransportError::OutcomeUnknown
                | TransportError::ResponseTooLarge => BrokerError::Unavailable,
            })
    }

    fn require_success(response: HttpResponse, expected: u16) -> Result<Vec<u8>, BrokerError> {
        if response.status == expected {
            return Ok(response.body);
        }
        match response.status {
            400..=404 => Err(BrokerError::Rejected(format!(
                "Robinhood returned HTTP {}",
                response.status
            ))),
            408 | 425 | 429 | 500..=599 => Err(BrokerError::Unavailable),
            _ => Err(BrokerError::Rejected(format!(
                "unexpected Robinhood HTTP status {}",
                response.status
            ))),
        }
    }

    fn write_response(response: HttpResponse, expected: u16) -> Result<ApiOrder, BrokerError> {
        // A reply failure is not proof that a write did not happen. In particular,
        // a duplicate-client-ID validation response may refer to an earlier acceptance.
        // Only local, pre-transmission validation is an unequivocal rejection here.
        if response.status != expected {
            return Err(BrokerError::AmbiguousOutcome);
        }
        serde_json::from_slice(&response.body).map_err(|_| BrokerError::AmbiguousOutcome)
    }

    fn accepted_at_ns(created_at: &str) -> Result<u64, BrokerError> {
        let timestamp = DateTime::parse_from_rfc3339(created_at).map_err(|_| {
            BrokerError::Rejected("broker returned an invalid timestamp".to_owned())
        })?;
        u64::try_from(timestamp.timestamp_nanos_opt().ok_or_else(|| {
            BrokerError::Rejected("broker timestamp is outside nanosecond range".to_owned())
        })?)
        .map_err(|_| BrokerError::Rejected("broker timestamp predates Unix epoch".to_owned()))
    }

    fn normalize_order(&self, order: ApiOrder) -> Result<BrokerOrderSnapshot, BrokerError> {
        let canonical_uuid = |value: &str| {
            Uuid::parse_str(value).is_ok_and(|id| id.hyphenated().to_string() == value)
        };
        if order.account_number != self.config.account_number
            || !canonical_uuid(&order.id)
            || !canonical_uuid(&order.client_order_id)
        {
            return Err(BrokerError::Unavailable);
        }
        let accepted_at_ns = Self::accepted_at_ns(&order.created_at)?;
        let updated_at_ns = Self::accepted_at_ns(&order.updated_at)?;
        if updated_at_ns < accepted_at_ns {
            return Err(BrokerError::Unavailable);
        }
        let acknowledgement = BrokerAcknowledgement {
            broker_order_id: order.id.clone(),
            client_order_id: order.client_order_id.clone(),
            accepted_at_ns,
        };
        let state = match order.state.as_str() {
            "pending" => BrokerOrderState::Pending,
            "open" => BrokerOrderState::Open,
            "partially_filled" => BrokerOrderState::PartiallyFilled,
            "filled" => BrokerOrderState::Filled,
            "canceled" => BrokerOrderState::Canceled,
            "failed" => BrokerOrderState::Failed,
            _ => {
                return Err(BrokerError::Rejected(
                    "broker returned an unknown order state".to_owned(),
                ))
            }
        };
        let mut executions = Vec::with_capacity(order.executions.len());
        let mut occurrences = BTreeMap::<(u64, String, String), u64>::new();
        for execution in order.executions {
            let effective_price = execution.effective_price.into_broker_decimal()?;
            let quantity = execution.quantity.into_broker_decimal()?;
            let occurred_ns = Self::accepted_at_ns(&execution.timestamp)?;
            let occurrence = occurrences
                .entry((
                    occurred_ns,
                    effective_price.as_str().to_owned(),
                    quantity.as_str().to_owned(),
                ))
                .or_default();
            let mut identity = Sha256::new();
            identity.update(order.id.as_bytes());
            identity.update([0]);
            // Fixed-width identity is identical on native and wasm32. Occurrence
            // counts distinguish identical fills without depending on list order.
            identity.update(occurred_ns.to_be_bytes());
            identity.update([0]);
            identity.update(effective_price.as_str().as_bytes());
            identity.update([0]);
            identity.update(quantity.as_str().as_bytes());
            identity.update([0]);
            identity.update(occurrence.to_be_bytes());
            *occurrence += 1;
            executions.push(BrokerExecution {
                execution_id: hex::encode(identity.finalize()),
                effective_price,
                quantity,
                occurred_at: execution.timestamp,
            });
        }
        Ok(BrokerOrderSnapshot {
            acknowledgement,
            state,
            executions,
            filled_quantity: order.filled_asset_quantity.into_broker_decimal()?,
            average_price: order
                .average_price
                .map(ApiDecimal::into_broker_decimal)
                .transpose()?,
            updated_at: order.updated_at,
        })
    }

    fn validate_intent(&self, intent: &OrderIntent) -> Result<(), BrokerError> {
        if !matches!(
            checked_notional(intent.proposal.limit_price, intent.proposal.quantity),
            Ok(notional) if notional == intent.authorized_notional
        ) {
            return Err(BrokerError::Rejected(
                "authorized notional does not match order price and quantity".to_owned(),
            ));
        }
        if intent.proposal.mode != ExecutionMode::Live {
            return Err(BrokerError::Rejected(
                "Robinhood Crypto has no paper execution environment".to_owned(),
            ));
        }
        if intent.proposal.venue != self.config.venue || intent.proposal.currency != "USD" {
            return Err(BrokerError::Rejected(
                "order does not target the configured Robinhood Crypto USD venue".to_owned(),
            ));
        }
        let parsed = Uuid::parse_str(&intent.client_order_id)
            .map_err(|_| BrokerError::Rejected("client order ID must be a UUID".to_owned()))?;
        if parsed.hyphenated().to_string() != intent.client_order_id {
            return Err(BrokerError::Rejected(
                "client order ID must use canonical lowercase UUID form".to_owned(),
            ));
        }
        let mut components = intent.proposal.symbol.split('-');
        let base = components.next().unwrap_or_default();
        let quote = components.next().unwrap_or_default();
        if base.is_empty()
            || quote != "USD"
            || components.next().is_some()
            || !base.bytes().all(|byte| byte.is_ascii_uppercase())
        {
            return Err(BrokerError::Rejected(
                "symbol must be an uppercase USD crypto pair such as BTC-USD".to_owned(),
            ));
        }
        Ok(())
    }

    fn account_query(&self) -> String {
        format!("?account_number={}", self.config.account_number)
    }

    fn list_page(&mut self, path: String) -> Result<ApiOrdersPage, BrokerError> {
        let response = self.execute(HttpMethod::Get, path, Vec::new())?;
        let body = Self::require_success(response, 200)?;
        if body.len() > MAX_ORDER_RESPONSE_BYTES {
            return Err(BrokerError::Unavailable);
        }
        let page: ApiOrdersPage =
            serde_json::from_slice(&body).map_err(|_| BrokerError::Unavailable)?;
        if page
            .results
            .iter()
            .any(|order| order.account_number != self.config.account_number)
        {
            return Err(BrokerError::Unavailable);
        }
        Ok(page)
    }

    fn lookup_api_order(&mut self, client_order_id: &str) -> Result<Option<ApiOrder>, BrokerError> {
        let parsed = Uuid::parse_str(client_order_id)
            .map_err(|_| BrokerError::Rejected("client order ID must be a UUID".to_owned()))?;
        if parsed.hyphenated().to_string() != client_order_id {
            return Err(BrokerError::Rejected(
                "client order ID must use canonical lowercase UUID form".to_owned(),
            ));
        }
        let mut path = format!("{V2_ORDERS_PATH}{}", self.account_query());
        let mut visited = BTreeSet::new();
        for page_index in 0..self.config.max_reconciliation_pages {
            if !visited.insert(path.clone()) {
                return Err(BrokerError::Unavailable);
            }
            let page = self.list_page(path)?;
            let mut matches = page
                .results
                .into_iter()
                .filter(|order| order.client_order_id == client_order_id);
            let found = matches.next();
            if matches.next().is_some() {
                return Err(BrokerError::Unavailable);
            }
            if found.is_some() {
                return Ok(found);
            }
            match page.next {
                Some(next) if page_index + 1 < self.config.max_reconciliation_pages => {
                    path = self.next_path(next)?;
                }
                Some(_) => return Err(BrokerError::Unavailable),
                None => return Ok(None),
            }
        }
        Err(BrokerError::Unavailable)
    }

    fn next_path(&self, next: String) -> Result<String, BrokerError> {
        if next.len() > 4096 || next.chars().any(char::is_control) {
            return Err(BrokerError::Unavailable);
        }
        let base = Url::parse(ROBINHOOD_CRYPTO_API_ORIGIN).expect("constant API origin");
        let url = base.join(&next).map_err(|_| BrokerError::Unavailable)?;
        if url.origin() != base.origin()
            || url.path() != V2_ORDERS_PATH
            || url.fragment().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(BrokerError::Unavailable);
        }
        let mut cursor = None;
        let mut account_seen = false;
        for (key, value) in url.query_pairs() {
            match key.as_ref() {
                "account_number"
                    if !account_seen && value.as_ref() == self.config.account_number =>
                {
                    account_seen = true;
                }
                "cursor" if cursor.is_none() && !value.is_empty() && value.len() <= 2048 => {
                    cursor = Some(value.into_owned());
                }
                _ => return Err(BrokerError::Unavailable),
            }
        }
        let cursor = cursor.ok_or(BrokerError::Unavailable)?;
        let query = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("account_number", &self.config.account_number)
            .append_pair("cursor", &cursor)
            .finish();
        Ok(format!("{V2_ORDERS_PATH}?{query}"))
    }
}

impl<T, C, S> BrokerPort for RobinhoodCryptoBroker<T, C, S>
where
    T: RobinhoodTransport,
    C: UnixClock,
    S: RequestSigner,
{
    fn submit(&mut self, intent: &OrderIntent) -> Result<BrokerAcknowledgement, BrokerError> {
        self.validate_intent(intent)?;
        let body = serde_json::to_vec(&PlaceLimitOrder {
            symbol: &intent.proposal.symbol,
            client_order_id: &intent.client_order_id,
            side: match intent.proposal.side {
                Side::Buy => "buy",
                Side::Sell => "sell",
            },
            order_type: "limit",
            limit_order_config: LimitOrderConfig {
                asset_quantity: micros_to_decimal(intent.proposal.quantity.0),
                limit_price: micros_to_decimal(intent.proposal.limit_price.0),
                time_in_force: self.config.time_in_force.as_api_str(),
            },
        })
        .map_err(|_| BrokerError::Rejected("order serialization failed".to_owned()))?;
        let path = format!("{V2_ORDERS_PATH}{}", self.account_query());
        let response = self.execute(HttpMethod::Post, path, body)?;
        let order = Self::write_response(response, 201)?;
        if order.symbol != intent.proposal.symbol
            || order.side
                != match intent.proposal.side {
                    Side::Buy => "buy",
                    Side::Sell => "sell",
                }
            || order.order_type != "limit"
        {
            return Err(BrokerError::AmbiguousOutcome);
        }
        let snapshot = self
            .normalize_order(order)
            .map_err(|_| BrokerError::AmbiguousOutcome)?;
        if snapshot.acknowledgement.client_order_id != intent.client_order_id {
            return Err(BrokerError::AmbiguousOutcome);
        }
        Ok(snapshot.acknowledgement)
    }

    fn lookup_by_client_order_id(
        &mut self,
        client_order_id: &str,
    ) -> Result<Option<BrokerAcknowledgement>, BrokerError> {
        self.fetch_order_by_client_order_id(client_order_id)
            .map(|snapshot| snapshot.map(|value| value.acknowledgement))
    }
}

impl<T, C, S> BrokerLifecyclePort for RobinhoodCryptoBroker<T, C, S>
where
    T: RobinhoodTransport,
    C: UnixClock,
    S: RequestSigner,
{
    fn fetch_order_by_client_order_id(
        &mut self,
        client_order_id: &str,
    ) -> Result<Option<BrokerOrderSnapshot>, BrokerError> {
        self.lookup_api_order(client_order_id)?
            .map(|order| self.normalize_order(order))
            .transpose()
    }

    fn cancel_by_client_order_id(
        &mut self,
        client_order_id: &str,
    ) -> Result<BrokerOrderSnapshot, BrokerError> {
        let current = self
            .fetch_order_by_client_order_id(client_order_id)?
            .ok_or_else(|| BrokerError::Rejected("broker order was not found".to_owned()))?;
        if current.state.is_terminal() {
            return Ok(current);
        }
        let path = format!(
            "{V2_ORDERS_PATH}{}/cancel/",
            current.acknowledgement.broker_order_id
        );
        let response = self.execute(HttpMethod::Post, path, Vec::new())?;
        let order = Self::write_response(response, 200)?;
        let snapshot = self
            .normalize_order(order)
            .map_err(|_| BrokerError::AmbiguousOutcome)?;
        if snapshot.acknowledgement.client_order_id != client_order_id
            || snapshot.acknowledgement.broker_order_id != current.acknowledgement.broker_order_id
        {
            return Err(BrokerError::AmbiguousOutcome);
        }
        Ok(snapshot)
    }
}

fn micros_to_decimal(value: u64) -> String {
    let whole = value / 1_000_000;
    let fractional = value % 1_000_000;
    if fractional == 0 {
        return whole.to_string();
    }
    let mut fraction = format!("{fractional:06}");
    while fraction.ends_with('0') {
        fraction.pop();
    }
    format!("{whole}.{fraction}")
}

#[cfg(feature = "native-http")]
mod native;

#[cfg(feature = "native-http")]
pub use native::*;

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use base64::Engine;
    use ed25519_dalek::{Signature, Verifier};
    use helio_execution::{MoneyMicros, OrderProposal, PriceMicros, QuantityMicros, Side};

    use super::*;

    const CLIENT_ID: &str = "11299b2b-61e3-43e7-b9f7-dee77210bb29";

    #[derive(Debug, Clone, Copy)]
    struct FixedClock(u64);

    impl UnixClock for FixedClock {
        fn unix_seconds(&mut self) -> Result<u64, ClockError> {
            Ok(self.0)
        }
    }

    #[derive(Debug, Clone, Copy)]
    struct StaticSigner;

    impl RequestSigner for StaticSigner {
        fn sign(
            &self,
            timestamp: u64,
            _path_and_query: &str,
            _method: HttpMethod,
            _body: &[u8],
        ) -> Result<SignedHeaders, SigningError> {
            Ok(SignedHeaders {
                api_key: "test-key".to_owned(),
                signature: "test-signature".to_owned(),
                timestamp: timestamp.to_string(),
            })
        }
    }

    #[derive(Debug, Default)]
    struct RecordingTransport {
        requests: Vec<HttpRequest>,
        responses: VecDeque<Result<HttpResponse, TransportError>>,
    }

    impl RecordingTransport {
        fn respond_json(&mut self, status: u16, body: serde_json::Value) {
            self.responses.push_back(Ok(HttpResponse {
                status,
                body: serde_json::to_vec(&body).unwrap(),
            }));
        }
    }

    impl RobinhoodTransport for RecordingTransport {
        fn execute(&mut self, request: HttpRequest) -> Result<HttpResponse, TransportError> {
            self.requests.push(request);
            self.responses
                .pop_front()
                .unwrap_or(Err(TransportError::Unavailable))
        }
    }

    fn config() -> RobinhoodCryptoConfig {
        RobinhoodCryptoConfig::try_new("cryptoaccount1").unwrap()
    }

    fn broker(
        transport: RecordingTransport,
    ) -> RobinhoodCryptoBroker<RecordingTransport, FixedClock, StaticSigner> {
        RobinhoodCryptoBroker::try_new(config(), transport, FixedClock(1_800_000_000), StaticSigner)
            .unwrap()
    }

    fn intent(mode: ExecutionMode) -> OrderIntent {
        OrderIntent {
            client_order_id: CLIENT_ID.to_owned(),
            proposal: OrderProposal {
                proposal_id: CLIENT_ID.to_owned(),
                strategy_id: "space-weather-crypto-v1".to_owned(),
                symbol: "BTC-USD".to_owned(),
                venue: "ROBINHOOD_CRYPTO".to_owned(),
                currency: "USD".to_owned(),
                side: Side::Buy,
                quantity: QuantityMicros(250_000),
                limit_price: PriceMicros(12_345_600_000),
                mode,
                trading_day: 1,
            },
            authorized_notional: MoneyMicros(3_086_400_000),
            risk_policy_version: "risk-1".to_owned(),
            authorized_at_ns: 100,
        }
    }

    fn order_json(state: &str) -> serde_json::Value {
        serde_json::json!({
            "id": "497f6eca-6276-4993-bfeb-53cbbbba6f08",
            "account_number": "cryptoaccount1",
            "client_order_id": CLIENT_ID,
            "symbol": "BTC-USD",
            "side": "buy",
            "type": "limit",
            "state": state,
            "executions": [{
                "effective_price": "12345.600000",
                "quantity": "0.100000",
                "timestamp": "2026-08-30T12:00:01Z"
            }],
            "filled_asset_quantity": "0.100000",
            "average_price": "12345.600000",
            "created_at": "2026-08-30T12:00:00Z",
            "updated_at": "2026-08-30T12:00:01Z"
        })
    }

    #[test]
    fn signer_uses_the_documented_canonical_message() {
        let private = [7_u8; 32];
        let signer =
            Ed25519RequestSigner::from_base64("rh-api-test", &BASE64_STANDARD.encode(private))
                .unwrap();
        let body = br#"{"client_order_id":"id"}"#;
        let headers = signer
            .sign(1_700_000_000, "/orders/?a=1", HttpMethod::Post, body)
            .unwrap();
        let signature_bytes = BASE64_STANDARD.decode(&headers.signature).unwrap();
        let signature = Signature::from_slice(&signature_bytes).unwrap();
        let message = [
            b"rh-api-test".as_slice(),
            b"1700000000",
            b"/orders/?a=1",
            b"POST",
            body,
        ]
        .concat();
        signer
            .signing_key
            .verifying_key()
            .verify(&message, &signature)
            .unwrap();
        assert!(!format!("{signer:?}").contains("rh-api-test"));
        let debug = format!("{headers:?}");
        assert!(!debug.contains("rh-api-test"));
        assert!(!debug.contains(&headers.signature));
    }

    #[test]
    fn debug_output_redacts_account_and_authenticated_request_values() {
        let config = config();
        assert!(!format!("{config:?}").contains("cryptoaccount1"));
        let request = HttpRequest {
            method: HttpMethod::Post,
            path_and_query: "/orders".to_owned(),
            headers: BTreeMap::from([("x-api-key".to_owned(), "secret-key".to_owned())]),
            body: b"secret-body".to_vec(),
        };
        let debug = format!("{request:?}");
        assert!(!debug.contains("secret-key"));
        assert!(!debug.contains("secret-body"));
    }

    #[test]
    fn submit_signs_the_exact_fixed_point_body_sent_to_robinhood() {
        let mut transport = RecordingTransport::default();
        transport.respond_json(201, order_json("open"));
        let mut broker = broker(transport);

        let acknowledgement = broker.submit(&intent(ExecutionMode::Live)).unwrap();
        assert_eq!(acknowledgement.client_order_id, CLIENT_ID);
        let request = &broker.transport.requests[0];
        assert_eq!(request.method, HttpMethod::Post);
        assert_eq!(
            request.path_and_query,
            "/api/v2/crypto/trading/orders/?account_number=cryptoaccount1"
        );
        assert_eq!(request.headers["x-timestamp"], "1800000000");
        let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
        assert_eq!(body["limit_order_config"]["asset_quantity"], "0.25");
        assert_eq!(body["limit_order_config"]["limit_price"], "12345.6");
        assert_eq!(body["limit_order_config"]["time_in_force"], "gtc");
    }

    #[test]
    fn paper_or_noncanonical_identity_is_rejected_before_transport() {
        let mut broker = broker(RecordingTransport::default());
        assert!(matches!(
            broker.submit(&intent(ExecutionMode::Paper)),
            Err(BrokerError::Rejected(_))
        ));
        let mut invalid = intent(ExecutionMode::Live);
        invalid.client_order_id = "not-a-uuid".to_owned();
        assert!(matches!(
            broker.submit(&invalid),
            Err(BrokerError::Rejected(_))
        ));
        let mut malformed = intent(ExecutionMode::Live);
        malformed.authorized_notional = MoneyMicros(1);
        assert!(matches!(
            broker.submit(&malformed),
            Err(BrokerError::Rejected(_))
        ));
        assert!(broker.transport.requests.is_empty());
    }

    #[test]
    fn reconciliation_follows_bounded_broker_pagination() {
        let mut transport = RecordingTransport::default();
        transport.respond_json(
            200,
            serde_json::json!({
                "next": "https://trading.robinhood.com/api/v2/crypto/trading/orders/?account_number=cryptoaccount1&cursor=next",
                "results": []
            }),
        );
        transport.respond_json(
            200,
            serde_json::json!({"next": null, "results": [order_json("partially_filled")] }),
        );
        let mut broker = broker(transport);
        let snapshot = broker
            .fetch_order_by_client_order_id(CLIENT_ID)
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.state, BrokerOrderState::PartiallyFilled);
        assert_eq!(snapshot.filled_quantity.as_str(), "0.1");
        assert_eq!(snapshot.average_price.unwrap().as_str(), "12345.6");
        assert_eq!(snapshot.executions.len(), 1);
        assert_eq!(broker.transport.requests.len(), 2);
    }

    #[test]
    fn incomplete_bounded_search_never_claims_the_order_is_absent() {
        let mut config = config();
        config.max_reconciliation_pages = 1;
        let mut transport = RecordingTransport::default();
        transport.respond_json(
            200,
            serde_json::json!({
                "next": "https://trading.robinhood.com/api/v2/crypto/trading/orders/?cursor=more",
                "results": []
            }),
        );
        let mut broker = RobinhoodCryptoBroker::try_new(
            config,
            transport,
            FixedClock(1_800_000_000),
            StaticSigner,
        )
        .unwrap();
        assert_eq!(
            broker.lookup_by_client_order_id(CLIENT_ID),
            Err(BrokerError::Unavailable)
        );
    }

    #[test]
    fn decimal_normalization_never_uses_binary_floating_point() {
        assert_eq!(micros_to_decimal(1), "0.000001");
        assert_eq!(micros_to_decimal(1_230_000), "1.23");
        assert_eq!(
            BrokerDecimal::try_new("0001.230000").unwrap().as_str(),
            "1.23"
        );
        assert!(BrokerDecimal::try_new("1e-6").is_err());
        assert!(BrokerDecimal::try_new("-1").is_err());
    }

    #[test]
    fn lost_or_unusable_submit_replies_are_reconciled_not_rejected() {
        for fault in [
            TransportError::OutcomeUnknown,
            TransportError::Unavailable,
            TransportError::ResponseTooLarge,
        ] {
            let mut transport = RecordingTransport::default();
            transport.responses.push_back(Err(fault));
            transport.respond_json(
                200,
                serde_json::json!({"next":null,"results":[order_json("open")]}),
            );
            let mut broker = broker(transport);
            assert_eq!(
                broker.submit(&intent(ExecutionMode::Live)),
                Err(BrokerError::AmbiguousOutcome)
            );
            assert!(broker
                .lookup_by_client_order_id(CLIENT_ID)
                .unwrap()
                .is_some());
            assert_eq!(
                broker
                    .transport
                    .requests
                    .iter()
                    .map(|r| r.method)
                    .collect::<Vec<_>>(),
                vec![HttpMethod::Post, HttpMethod::Get]
            );
        }
        for status in [200, 204, 400, 401, 403, 404, 408, 409, 429, 500, 503] {
            let mut transport = RecordingTransport::default();
            transport.respond_json(status, serde_json::json!({"error":"synthetic"}));
            let mut broker = broker(transport);
            assert_eq!(
                broker.submit(&intent(ExecutionMode::Live)),
                Err(BrokerError::AmbiguousOutcome)
            );
            assert_eq!(broker.transport.requests.len(), 1);
        }
    }

    #[test]
    fn successful_http_status_does_not_certify_an_invalid_acknowledgement() {
        let mut replies = vec![b"{".to_vec(), b"{}".to_vec()];
        for (field, value) in [
            ("account_number", "different"),
            ("client_order_id", "00000000-0000-4000-8000-000000000002"),
            ("id", "../other/cancel"),
            ("side", "sell"),
            ("symbol", "ETH-USD"),
            ("type", "market"),
            ("state", "future_state"),
            ("created_at", "invalid"),
            ("updated_at", "2026-08-30T11:00:00Z"),
        ] {
            let mut order = order_json("open");
            order[field] = value.into();
            replies.push(serde_json::to_vec(&order).unwrap());
        }
        for body in replies {
            let mut transport = RecordingTransport::default();
            transport
                .responses
                .push_back(Ok(HttpResponse { status: 201, body }));
            let mut broker = broker(transport);
            assert_eq!(
                broker.submit(&intent(ExecutionMode::Live)),
                Err(BrokerError::AmbiguousOutcome)
            );
            assert_eq!(broker.transport.requests.len(), 1);
        }
    }

    #[test]
    fn missing_page_fields_and_cross_account_rows_never_prove_absence() {
        let mut other_account = order_json("open");
        other_account["account_number"] = "different".into();
        for page in [
            serde_json::json!({}),
            serde_json::json!({"next":null}),
            serde_json::json!({"results":[]}),
            serde_json::json!({"next":null,"results":null}),
            serde_json::json!({"next":null,"results":[other_account]}),
        ] {
            let mut transport = RecordingTransport::default();
            transport.respond_json(200, page);
            let mut broker = broker(transport);
            assert_eq!(
                broker.lookup_by_client_order_id(CLIENT_ID),
                Err(BrokerError::Unavailable)
            );
            assert_eq!(broker.transport.requests.len(), 1);
        }
    }

    #[test]
    fn pagination_keeps_the_bound_account_and_cannot_narrow_or_escape_the_search() {
        for next in [
            "https://example.com/api/v2/crypto/trading/orders/?cursor=next",
            "https://trading.robinhood.com:444/api/v2/crypto/trading/orders/?cursor=next",
            "https://name@trading.robinhood.com/api/v2/crypto/trading/orders/?cursor=next",
            "/api/v2/crypto/trading/orders/id/cancel/?cursor=next",
            "/api/v2/crypto/trading/orders/?account_number=other&cursor=next",
            "/api/v2/crypto/trading/orders/?account_number=cryptoaccount1&account_number=cryptoaccount1&cursor=next",
            "/api/v2/crypto/trading/orders/?cursor=next&cursor=other",
            "/api/v2/crypto/trading/orders/?cursor=next&state=filled",
            "/api/v2/crypto/trading/orders/?cursor=",
            "/api/v2/crypto/trading/orders/?cursor=next#fragment",
        ] {
            let mut transport = RecordingTransport::default();
            transport.respond_json(200, serde_json::json!({"next":next,"results":[]}));
            let mut broker = broker(transport);
            assert_eq!(
                broker.lookup_by_client_order_id(CLIENT_ID),
                Err(BrokerError::Unavailable)
            );
            assert_eq!(broker.transport.requests.len(), 1);
        }
        let mut transport = RecordingTransport::default();
        transport.respond_json(
            200,
            serde_json::json!({
                "next":"/api/v2/crypto/trading/orders/?cursor=opaque%2B%3D%26",
                "results":[]
            }),
        );
        transport.respond_json(200, serde_json::json!({"next":null,"results":[]}));
        let mut broker = broker(transport);
        assert!(broker
            .lookup_by_client_order_id(CLIENT_ID)
            .unwrap()
            .is_none());
        assert_eq!(
            broker.transport.requests[1].path_and_query,
            "/api/v2/crypto/trading/orders/?account_number=cryptoaccount1&cursor=opaque%2B%3D%26"
        );
        assert!(!format!("{:?}", broker.transport.requests[1]).contains("cryptoaccount1"));
    }

    #[test]
    fn broker_json_numbers_preserve_precision_including_scientific_notation() {
        for (wire, exact) in [
            (
                "0.12345678901234567890123456789",
                "0.12345678901234567890123456789",
            ),
            ("1.234567890123456789e-8", "0.00000001234567890123456789"),
            ("123456789012345678901", "123456789012345678901"),
            ("1e-8", "0.00000001"),
        ] {
            let body = serde_json::to_string(&serde_json::json!({
                "next":null,"results":[order_json("partially_filled")]
            }))
            .unwrap()
            .replace("\"0.100000\"", wire);
            let mut transport = RecordingTransport::default();
            transport.responses.push_back(Ok(HttpResponse {
                status: 200,
                body: body.into_bytes(),
            }));
            let mut broker = broker(transport);
            let snapshot = broker
                .fetch_order_by_client_order_id(CLIENT_ID)
                .unwrap()
                .unwrap();
            assert_eq!(snapshot.filled_quantity.as_str(), exact);
            assert_eq!(snapshot.executions[0].quantity.as_str(), exact);
        }
        for excessive in ["1e1000000", "1e-1000000", "-1", "1".repeat(129).as_str()] {
            assert!(expand_json_decimal(excessive).is_err());
        }
    }

    #[test]
    fn fill_identities_survive_reordering_format_changes_and_identical_fills() {
        let mut order = order_json("partially_filled");
        let first = order["executions"][0].clone();
        let mut second = first.clone();
        second["quantity"] = "0.00000001".into();
        order["executions"] = serde_json::json!([first.clone(), second.clone(), first]);
        let mut reordered = order.clone();
        reordered["executions"] =
            serde_json::json!([order["executions"][2], order["executions"][0], second]);
        for execution in reordered["executions"].as_array_mut().unwrap() {
            execution["timestamp"] = "2026-08-30T06:00:01-06:00".into();
            execution["effective_price"] = "12345.6".into();
        }
        let broker = broker(RecordingTransport::default());
        let ids = |wire| {
            broker
                .normalize_order(serde_json::from_value(wire).unwrap())
                .unwrap()
                .executions
                .into_iter()
                .map(|execution| execution.execution_id)
                .collect::<std::collections::BTreeSet<_>>()
        };
        let original = ids(order);
        assert_eq!(original.len(), 3);
        assert_eq!(original, ids(reordered));
    }

    #[test]
    fn cancel_requires_the_same_broker_identity_and_keeps_ambiguous_outcomes_open() {
        for wrong_identity in [false, true] {
            let mut transport = RecordingTransport::default();
            transport.respond_json(
                200,
                serde_json::json!({"next":null,"results":[order_json("open")]}),
            );
            let mut reply = order_json("canceled");
            if wrong_identity {
                reply["id"] = "00000000-0000-4000-8000-000000000002".into();
                transport.respond_json(200, reply);
            } else {
                transport
                    .responses
                    .push_back(Err(TransportError::OutcomeUnknown));
            }
            let mut broker = broker(transport);
            assert_eq!(
                broker.cancel_by_client_order_id(CLIENT_ID),
                Err(BrokerError::AmbiguousOutcome)
            );
            assert_eq!(broker.transport.requests.len(), 2);
        }
        let mut transport = RecordingTransport::default();
        transport.respond_json(
            200,
            serde_json::json!({"next":null,"results":[order_json("filled")]}),
        );
        let mut broker = broker(transport);
        assert_eq!(
            broker.cancel_by_client_order_id(CLIENT_ID).unwrap().state,
            BrokerOrderState::Filled
        );
        assert_eq!(broker.transport.requests.len(), 1);
    }

    proptest::proptest! {
        #[test]
        fn json_asset_quantities_roundtrip_exactly(
            whole in 0_u64..1_000_000_000,
            fraction in 0_u64..1_000_000_000_000_000_000,
        ) {
            let literal = format!("{whole}.{fraction:018}");
            let decoded: ApiDecimal = serde_json::from_str(&literal).unwrap();
            proptest::prop_assert_eq!(
                decoded.into_broker_decimal().unwrap(),
                BrokerDecimal::try_new(literal).unwrap()
            );
        }

        #[test]
        fn opaque_cursors_cannot_inject_query_parameters(cursor in "[a-zA-Z0-9&=+:/?#%]{1,100}") {
            let broker = broker(RecordingTransport::default());
            let query = url::form_urlencoded::Serializer::new(String::new())
                .append_pair("cursor", &cursor).finish();
            let next = broker.next_path(format!("{V2_ORDERS_PATH}?{query}")).unwrap();
            let parsed = Url::parse(&format!("{ROBINHOOD_CRYPTO_API_ORIGIN}{next}")).unwrap();
            let params = parsed.query_pairs().collect::<Vec<_>>();
            proptest::prop_assert_eq!(params.len(), 2);
            proptest::prop_assert_eq!(&*params[0].0, "account_number");
            proptest::prop_assert_eq!(&*params[0].1, "cryptoaccount1");
            proptest::prop_assert_eq!(&*params[1].0, "cursor");
            proptest::prop_assert_eq!(&*params[1].1, cursor);
        }
    }
}
