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

use std::collections::BTreeMap;
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
use uuid::Uuid;
use zeroize::Zeroize;

pub const ROBINHOOD_CRYPTO_API_ORIGIN: &str = "https://trading.robinhood.com";
pub const ROBINHOOD_AGENTIC_MCP_URL: &str = "https://agent.robinhood.com/mcp/trading";
const V2_ORDERS_PATH: &str = "/api/v2/crypto/trading/orders/";

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
            .field("path_and_query", &self.path_and_query)
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ClockError {
    #[error("system clock is before the Unix epoch")]
    BeforeUnixEpoch,
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
#[serde(untagged)]
enum ApiDecimal {
    String(String),
    Number(serde_json::Number),
}

impl ApiDecimal {
    fn into_broker_decimal(self) -> Result<BrokerDecimal, BrokerError> {
        let value = match self {
            Self::String(value) => value,
            Self::Number(value) => value.to_string(),
        };
        BrokerDecimal::try_new(value)
            .map_err(|_| BrokerError::Rejected("broker returned an invalid decimal".to_owned()))
    }
}

fn zero_decimal() -> ApiDecimal {
    ApiDecimal::String("0".to_owned())
}

#[derive(Debug, Clone, Deserialize)]
struct ApiExecution {
    effective_price: ApiDecimal,
    quantity: ApiDecimal,
    timestamp: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ApiOrder {
    id: String,
    client_order_id: String,
    state: String,
    #[serde(default)]
    executions: Vec<ApiExecution>,
    #[serde(default = "zero_decimal")]
    filled_asset_quantity: ApiDecimal,
    #[serde(default)]
    average_price: Option<ApiDecimal>,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Deserialize)]
struct ApiOrdersPage {
    next: Option<String>,
    #[serde(default)]
    results: Vec<ApiOrder>,
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
                TransportError::OutcomeUnknown if method == HttpMethod::Post => {
                    BrokerError::AmbiguousOutcome
                }
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

    fn accepted_at_ns(created_at: &str) -> Result<u64, BrokerError> {
        let timestamp = DateTime::parse_from_rfc3339(created_at).map_err(|_| {
            BrokerError::Rejected("broker returned an invalid timestamp".to_owned())
        })?;
        u64::try_from(timestamp.timestamp_nanos_opt().ok_or_else(|| {
            BrokerError::Rejected("broker timestamp is outside nanosecond range".to_owned())
        })?)
        .map_err(|_| BrokerError::Rejected("broker timestamp predates Unix epoch".to_owned()))
    }

    fn normalize_order(order: ApiOrder) -> Result<BrokerOrderSnapshot, BrokerError> {
        let accepted_at_ns = Self::accepted_at_ns(&order.created_at)?;
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
        for (index, execution) in order.executions.into_iter().enumerate() {
            let effective_price = execution.effective_price.into_broker_decimal()?;
            let quantity = execution.quantity.into_broker_decimal()?;
            let mut identity = Sha256::new();
            identity.update(order.id.as_bytes());
            identity.update([0]);
            identity.update(index.to_be_bytes());
            identity.update(execution.timestamp.as_bytes());
            identity.update([0]);
            identity.update(effective_price.as_str().as_bytes());
            identity.update([0]);
            identity.update(quantity.as_str().as_bytes());
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
        serde_json::from_slice(&body)
            .map_err(|_| BrokerError::Rejected("broker returned invalid order JSON".to_owned()))
    }

    fn next_path(next: String) -> Result<String, BrokerError> {
        let relative = next
            .strip_prefix(ROBINHOOD_CRYPTO_API_ORIGIN)
            .unwrap_or(&next);
        if !relative.starts_with(V2_ORDERS_PATH) {
            return Err(BrokerError::Rejected(
                "broker pagination escaped the orders endpoint".to_owned(),
            ));
        }
        Ok(relative.to_owned())
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
        let body = Self::require_success(response, 201)?;
        let order: ApiOrder = serde_json::from_slice(&body)
            .map_err(|_| BrokerError::Rejected("broker returned invalid order JSON".to_owned()))?;
        let snapshot = Self::normalize_order(order)?;
        if snapshot.acknowledgement.client_order_id != intent.client_order_id {
            return Err(BrokerError::Rejected(
                "broker returned a different client order identity".to_owned(),
            ));
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
        let parsed = Uuid::parse_str(client_order_id)
            .map_err(|_| BrokerError::Rejected("client order ID must be a UUID".to_owned()))?;
        if parsed.hyphenated().to_string() != client_order_id {
            return Err(BrokerError::Rejected(
                "client order ID must use canonical lowercase UUID form".to_owned(),
            ));
        }
        let mut path = format!("{V2_ORDERS_PATH}{}", self.account_query());
        for page_index in 0..self.config.max_reconciliation_pages {
            let page = self.list_page(path)?;
            if let Some(order) = page
                .results
                .into_iter()
                .find(|order| order.client_order_id == client_order_id)
            {
                return Self::normalize_order(order).map(Some);
            }
            match page.next {
                Some(next) if page_index + 1 < self.config.max_reconciliation_pages => {
                    path = Self::next_path(next)?;
                }
                Some(_) => return Err(BrokerError::Unavailable),
                None => return Ok(None),
            }
        }
        Err(BrokerError::Unavailable)
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
        let body = Self::require_success(response, 200)?;
        let order: ApiOrder = serde_json::from_slice(&body)
            .map_err(|_| BrokerError::Rejected("broker returned invalid order JSON".to_owned()))?;
        let snapshot = Self::normalize_order(order)?;
        if snapshot.acknowledgement.client_order_id != client_order_id {
            return Err(BrokerError::Rejected(
                "broker returned a different client order identity".to_owned(),
            ));
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
            "client_order_id": CLIENT_ID,
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
}
