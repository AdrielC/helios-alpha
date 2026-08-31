//! Alpaca trading and market-data integration for Helios.
//!
//! The portable core owns request construction, exact decimal normalization, lifecycle mapping,
//! reconciliation, and feed parsing. Network I/O is injected, which keeps broker behavior
//! deterministic in tests and keeps Helios domain crates portable to WASI. Native HTTP is an
//! optional edge adapter.

mod market_data;

use std::collections::BTreeMap;
use std::fmt;

use helio_execution::{
    checked_notional, BrokerAcknowledgement, BrokerDecimal, BrokerError, BrokerExecution,
    BrokerLifecyclePort, BrokerOrderSnapshot, BrokerOrderState, BrokerPort, ExecutionMode,
    OrderIntent, Side,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use zeroize::Zeroize;

pub use market_data::*;

pub const ALPACA_PAPER_API_ORIGIN: &str = "https://paper-api.alpaca.markets";
pub const ALPACA_LIVE_API_ORIGIN: &str = "https://api.alpaca.markets";
pub const ALPACA_MARKET_DATA_STREAM_ORIGIN: &str = "wss://stream.data.alpaca.markets";
pub const ALPACA_PAPER_TRADING_STREAM_URL: &str = "wss://paper-api.alpaca.markets/stream";
pub const ALPACA_LIVE_TRADING_STREAM_URL: &str = "wss://api.alpaca.markets/stream";

const ORDERS_PATH: &str = "/v2/orders";
const POSITIONS_PATH: &str = "/v2/positions";
const ACCOUNT_PATH: &str = "/v2/account";
const FILL_ACTIVITIES_PATH: &str = "/v2/account/activities/FILL";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlpacaEnvironment {
    Paper,
    Live,
}

impl AlpacaEnvironment {
    pub const fn api_origin(self) -> &'static str {
        match self {
            Self::Paper => ALPACA_PAPER_API_ORIGIN,
            Self::Live => ALPACA_LIVE_API_ORIGIN,
        }
    }

    pub const fn trading_stream_url(self) -> &'static str {
        match self {
            Self::Paper => ALPACA_PAPER_TRADING_STREAM_URL,
            Self::Live => ALPACA_LIVE_TRADING_STREAM_URL,
        }
    }

    const fn execution_mode(self) -> ExecutionMode {
        match self {
            Self::Paper => ExecutionMode::Paper,
            Self::Live => ExecutionMode::Live,
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct AlpacaConfig {
    pub environment: AlpacaEnvironment,
    pub venue: String,
    pub extended_hours: bool,
    pub max_reconciliation_pages: usize,
    pub reconciliation_page_size: usize,
}

impl fmt::Debug for AlpacaConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AlpacaConfig")
            .field("environment", &self.environment)
            .field("venue", &self.venue)
            .field("extended_hours", &self.extended_hours)
            .field("max_reconciliation_pages", &self.max_reconciliation_pages)
            .field("reconciliation_page_size", &self.reconciliation_page_size)
            .finish()
    }
}

impl AlpacaConfig {
    pub fn paper() -> Self {
        Self {
            environment: AlpacaEnvironment::Paper,
            venue: "ALPACA".into(),
            extended_hours: false,
            max_reconciliation_pages: 8,
            reconciliation_page_size: 100,
        }
    }

    pub fn validate(&self) -> Result<(), ConfigurationError> {
        if self.venue.trim().is_empty() {
            return Err(ConfigurationError::InvalidVenue);
        }
        if self.max_reconciliation_pages == 0 || !(1..=100).contains(&self.reconciliation_page_size)
        {
            return Err(ConfigurationError::InvalidReconciliationBounds);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ConfigurationError {
    #[error("Alpaca API key and secret must be non-empty printable ASCII")]
    InvalidCredentials,
    #[error("Alpaca venue must be non-empty")]
    InvalidVenue,
    #[error("Alpaca reconciliation bounds are invalid")]
    InvalidReconciliationBounds,
}

pub struct AlpacaCredentials {
    key_id: String,
    secret_key: String,
}

impl fmt::Debug for AlpacaCredentials {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AlpacaCredentials")
            .field("key_id", &"[redacted]")
            .field("secret_key", &"[redacted]")
            .finish()
    }
}

impl Drop for AlpacaCredentials {
    fn drop(&mut self) {
        self.key_id.zeroize();
        self.secret_key.zeroize();
    }
}

impl AlpacaCredentials {
    pub fn try_new(
        key_id: impl Into<String>,
        secret_key: impl Into<String>,
    ) -> Result<Self, ConfigurationError> {
        let credentials = Self {
            key_id: key_id.into(),
            secret_key: secret_key.into(),
        };
        if !valid_secret(&credentials.key_id) || !valid_secret(&credentials.secret_key) {
            return Err(ConfigurationError::InvalidCredentials);
        }
        Ok(credentials)
    }

    fn apply(&self, headers: &mut BTreeMap<String, String>) {
        headers.insert("APCA-API-KEY-ID".into(), self.key_id.clone());
        headers.insert("APCA-API-SECRET-KEY".into(), self.secret_key.clone());
    }

    pub fn websocket_auth_message(&self) -> serde_json::Value {
        serde_json::json!({
            "action": "auth",
            "key": self.key_id,
            "secret": self.secret_key,
        })
    }
}

fn valid_secret(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_graphic())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
    Patch,
    Delete,
}

impl HttpMethod {
    pub const fn is_mutating(self) -> bool {
        !matches!(self, Self::Get)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: HttpMethod,
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
    #[error("Alpaca transport is unavailable")]
    Unavailable,
    #[error("Alpaca request outcome is unknown")]
    OutcomeUnknown,
    #[error("Alpaca response exceeded the configured byte limit")]
    ResponseTooLarge,
}

pub trait AlpacaTransport {
    fn execute(&mut self, request: HttpRequest) -> Result<HttpResponse, TransportError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlpacaOrderType {
    Market,
    Limit,
}

impl AlpacaOrderType {
    const fn as_api_str(self) -> &'static str {
        match self {
            Self::Market => "market",
            Self::Limit => "limit",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlpacaTimeInForce {
    Day,
    GoodTilCanceled,
    ImmediateOrCancel,
    FillOrKill,
}

impl AlpacaTimeInForce {
    const fn as_api_str(self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::GoodTilCanceled => "gtc",
            Self::ImmediateOrCancel => "ioc",
            Self::FillOrKill => "fok",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlpacaOrderRequest {
    pub client_order_id: String,
    pub symbol: String,
    pub side: Side,
    pub quantity: BrokerDecimal,
    pub order_type: AlpacaOrderType,
    pub time_in_force: AlpacaTimeInForce,
    pub limit_price: Option<BrokerDecimal>,
    pub extended_hours: bool,
}

impl AlpacaOrderRequest {
    fn validate(&self) -> Result<(), BrokerError> {
        if self.client_order_id.is_empty()
            || self.client_order_id.len() > 128
            || !self
                .client_order_id
                .bytes()
                .all(|byte| byte.is_ascii_graphic())
        {
            return Err(rejected(
                "client order ID must be 1 to 128 visible ASCII characters",
            ));
        }
        if self.symbol.is_empty()
            || self.symbol.len() > 32
            || !self
                .symbol
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err(rejected(
                "symbol must be a canonical uppercase Alpaca symbol",
            ));
        }
        if self.quantity.as_str() == "0" {
            return Err(rejected("quantity must be positive"));
        }
        match (self.order_type, &self.limit_price) {
            (AlpacaOrderType::Market, None) => {}
            (AlpacaOrderType::Limit, Some(price)) if price.as_str() != "0" => {}
            _ => {
                return Err(rejected(
                    "limit orders require a positive limit price and market orders forbid one",
                ))
            }
        }
        if self.extended_hours
            && !(self.order_type == AlpacaOrderType::Limit
                && self.time_in_force == AlpacaTimeInForce::Day)
        {
            return Err(rejected("extended-hours orders must be day limit orders"));
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
struct PlaceOrder<'a> {
    symbol: &'a str,
    qty: &'a str,
    side: &'static str,
    #[serde(rename = "type")]
    order_type: &'static str,
    time_in_force: &'static str,
    client_order_id: &'a str,
    extended_hours: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    limit_price: Option<&'a str>,
}

#[derive(Debug, Serialize)]
struct ReplaceOrder<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    qty: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    limit_price: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    time_in_force: Option<&'a str>,
    client_order_id: &'a str,
}

#[derive(Debug, Clone, Deserialize)]
struct ApiOrder {
    id: String,
    client_order_id: String,
    status: String,
    symbol: String,
    side: String,
    qty: ApiDecimal,
    filled_qty: ApiDecimal,
    filled_avg_price: Option<ApiDecimal>,
    submitted_at: String,
    updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum ApiDecimal {
    Text(String),
    Number(serde_json::Number),
}

impl ApiDecimal {
    fn as_text(&self) -> String {
        match self {
            Self::Text(value) => value.clone(),
            Self::Number(value) => value.to_string(),
        }
    }

    fn into_broker_decimal(self) -> Result<BrokerDecimal, BrokerError> {
        BrokerDecimal::try_new(self.as_text())
            .map_err(|_| rejected("Alpaca returned an invalid decimal"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlpacaAccount {
    pub id: String,
    pub status: String,
    pub currency: String,
    pub buying_power: SignedDecimal,
    pub cash: SignedDecimal,
    pub portfolio_value: SignedDecimal,
    pub equity: SignedDecimal,
    pub last_equity: SignedDecimal,
    pub long_market_value: SignedDecimal,
    pub short_market_value: SignedDecimal,
    pub initial_margin: SignedDecimal,
    pub maintenance_margin: SignedDecimal,
    pub daytrade_count: u64,
    pub trading_blocked: bool,
    pub transfers_blocked: bool,
    pub account_blocked: bool,
    pub trade_suspended_by_user: bool,
}

#[derive(Debug, Deserialize)]
struct ApiAccount {
    id: String,
    status: String,
    currency: String,
    buying_power: ApiDecimal,
    cash: ApiDecimal,
    portfolio_value: ApiDecimal,
    equity: ApiDecimal,
    last_equity: ApiDecimal,
    long_market_value: ApiDecimal,
    short_market_value: ApiDecimal,
    initial_margin: ApiDecimal,
    maintenance_margin: ApiDecimal,
    daytrade_count: u64,
    trading_blocked: bool,
    transfers_blocked: bool,
    account_blocked: bool,
    trade_suspended_by_user: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlpacaPosition {
    pub asset_id: String,
    pub symbol: String,
    pub exchange: String,
    pub asset_class: String,
    pub quantity: SignedDecimal,
    pub quantity_available: Option<SignedDecimal>,
    pub side: String,
    pub average_entry_price: SignedDecimal,
    pub market_value: SignedDecimal,
    pub cost_basis: SignedDecimal,
    pub unrealized_pl: SignedDecimal,
    pub unrealized_pl_percent: SignedDecimal,
    pub current_price: SignedDecimal,
    pub last_day_price: SignedDecimal,
    pub change_today: SignedDecimal,
}

#[derive(Debug, Deserialize)]
struct ApiPosition {
    asset_id: String,
    symbol: String,
    exchange: String,
    asset_class: String,
    qty: ApiDecimal,
    qty_available: Option<ApiDecimal>,
    side: String,
    avg_entry_price: ApiDecimal,
    market_value: ApiDecimal,
    cost_basis: ApiDecimal,
    unrealized_pl: ApiDecimal,
    unrealized_plpc: ApiDecimal,
    current_price: ApiDecimal,
    lastday_price: ApiDecimal,
    change_today: ApiDecimal,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SignedDecimal(String);

impl SignedDecimal {
    pub fn try_new(value: impl Into<String>) -> Result<Self, DecimalError> {
        let value = value.into();
        let (negative, magnitude) = value
            .strip_prefix('-')
            .map_or((false, value.as_str()), |magnitude| (true, magnitude));
        let canonical = BrokerDecimal::try_new(magnitude.to_owned()).map_err(|_| DecimalError)?;
        Ok(Self(if negative && canonical.as_str() != "0" {
            format!("-{}", canonical.as_str())
        } else {
            canonical.as_str().to_owned()
        }))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for SignedDecimal {
    type Error = DecimalError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::try_new(value)
    }
}

impl From<SignedDecimal> for String {
    fn from(value: SignedDecimal) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("invalid signed base-10 decimal")]
pub struct DecimalError;

#[derive(Debug, Deserialize)]
struct ApiFillActivity {
    id: String,
    transaction_time: String,
    price: ApiDecimal,
    qty: ApiDecimal,
    order_id: String,
}

pub struct AlpacaBroker<T> {
    config: AlpacaConfig,
    credentials: AlpacaCredentials,
    transport: T,
}

impl<T: fmt::Debug> fmt::Debug for AlpacaBroker<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AlpacaBroker")
            .field("config", &self.config)
            .field("credentials", &self.credentials)
            .field("transport", &self.transport)
            .finish()
    }
}

impl<T> AlpacaBroker<T>
where
    T: AlpacaTransport,
{
    pub fn try_new(
        config: AlpacaConfig,
        credentials: AlpacaCredentials,
        transport: T,
    ) -> Result<Self, ConfigurationError> {
        config.validate()?;
        Ok(Self {
            config,
            credentials,
            transport,
        })
    }

    pub fn environment(&self) -> AlpacaEnvironment {
        self.config.environment
    }

    pub fn venue(&self) -> &str {
        &self.config.venue
    }

    pub fn into_transport(self) -> T {
        self.transport
    }

    pub fn submit_order(
        &mut self,
        order: &AlpacaOrderRequest,
    ) -> Result<BrokerOrderSnapshot, BrokerError> {
        order.validate()?;
        let body = serde_json::to_vec(&PlaceOrder {
            symbol: &order.symbol,
            qty: order.quantity.as_str(),
            side: side(order.side),
            order_type: order.order_type.as_api_str(),
            time_in_force: order.time_in_force.as_api_str(),
            client_order_id: &order.client_order_id,
            extended_hours: order.extended_hours,
            limit_price: order.limit_price.as_ref().map(BrokerDecimal::as_str),
        })
        .map_err(|_| rejected("order serialization failed"))?;
        let response = self.execute(HttpMethod::Post, ORDERS_PATH.into(), body)?;
        let api_order: ApiOrder = self.success_json(response, &[200, 201])?;
        let snapshot = normalize_order(api_order, Vec::new())?;
        if snapshot.acknowledgement.client_order_id != order.client_order_id {
            return Err(rejected(
                "Alpaca returned a different client order identity",
            ));
        }
        Ok(snapshot)
    }

    pub fn replace_order(
        &mut self,
        client_order_id: &str,
        new_client_order_id: &str,
        quantity: Option<&BrokerDecimal>,
        limit_price: Option<&BrokerDecimal>,
        time_in_force: Option<AlpacaTimeInForce>,
    ) -> Result<BrokerOrderSnapshot, BrokerError> {
        validate_client_order_id(new_client_order_id)?;
        if quantity.is_none() && limit_price.is_none() && time_in_force.is_none() {
            return Err(rejected(
                "replace must change quantity, limit price, or time in force",
            ));
        }
        let current = self
            .fetch_order_by_client_order_id(client_order_id)?
            .ok_or_else(|| rejected("Alpaca order was not found"))?;
        if current.state.is_terminal() {
            return Err(rejected("terminal Alpaca order cannot be replaced"));
        }
        let body = serde_json::to_vec(&ReplaceOrder {
            qty: quantity.map(BrokerDecimal::as_str),
            limit_price: limit_price.map(BrokerDecimal::as_str),
            time_in_force: time_in_force.map(AlpacaTimeInForce::as_api_str),
            client_order_id: new_client_order_id,
        })
        .map_err(|_| rejected("replace serialization failed"))?;
        let response = self.execute(
            HttpMethod::Patch,
            format!("{ORDERS_PATH}/{}", current.acknowledgement.broker_order_id),
            body,
        )?;
        let api_order: ApiOrder = self.success_json(response, &[200])?;
        normalize_order(api_order, Vec::new())
    }

    pub fn account(&mut self) -> Result<AlpacaAccount, BrokerError> {
        let response = self.execute(HttpMethod::Get, ACCOUNT_PATH.into(), Vec::new())?;
        let account: ApiAccount = self.success_json(response, &[200])?;
        normalize_account(account)
    }

    pub fn positions(&mut self) -> Result<Vec<AlpacaPosition>, BrokerError> {
        let response = self.execute(HttpMethod::Get, POSITIONS_PATH.into(), Vec::new())?;
        let positions: Vec<ApiPosition> = self.success_json(response, &[200])?;
        positions.into_iter().map(normalize_position).collect()
    }

    /// Returns every currently open order in bounded chronological pages.
    ///
    /// This is the startup-reconciliation inventory. It intentionally excludes closed history:
    /// the operator must account for every active broker liability before admitting commands,
    /// while durable OMS history remains available through its own event cursor.
    pub fn open_orders_for_reconciliation(
        &mut self,
    ) -> Result<Vec<BrokerOrderSnapshot>, BrokerError> {
        let mut orders = Vec::new();
        let mut after_order_id: Option<String> = None;
        for _ in 0..self.config.max_reconciliation_pages {
            let mut path = format!(
                "{ORDERS_PATH}?status=open&limit={}&direction=asc&nested=false",
                self.config.reconciliation_page_size
            );
            if let Some(order_id) = &after_order_id {
                path.push_str("&after_order_id=");
                path.push_str(&percent_encode(order_id));
            }
            let response = self.execute(HttpMethod::Get, path, Vec::new())?;
            let page: Vec<ApiOrder> = self.success_json(response, &[200])?;
            let count = page.len();
            after_order_id = page.last().map(|order| order.id.clone());
            orders.extend(
                page.into_iter()
                    .map(|order| normalize_order(order, Vec::new()))
                    .collect::<Result<Vec<_>, _>>()?,
            );
            if count < self.config.reconciliation_page_size {
                return Ok(orders);
            }
        }
        Err(BrokerError::Unavailable)
    }

    fn fetch_api_order(&mut self, client_order_id: &str) -> Result<Option<ApiOrder>, BrokerError> {
        validate_client_order_id(client_order_id)?;
        let response = self.execute(
            HttpMethod::Get,
            format!(
                "{ORDERS_PATH}:by_client_order_id?client_order_id={}",
                percent_encode(client_order_id)
            ),
            Vec::new(),
        )?;
        if response.status == 404 {
            return Ok(None);
        }
        self.success_json(response, &[200]).map(Some)
    }

    fn fills_for_order(&mut self, order_id: &str) -> Result<Vec<BrokerExecution>, BrokerError> {
        let mut executions = Vec::new();
        let mut page_token: Option<String> = None;
        for _ in 0..self.config.max_reconciliation_pages {
            let mut path = format!(
                "{FILL_ACTIVITIES_PATH}?direction=asc&page_size={}",
                self.config.reconciliation_page_size
            );
            if let Some(token) = &page_token {
                path.push_str("&page_token=");
                path.push_str(&percent_encode(token));
            }
            let response = self.execute(HttpMethod::Get, path, Vec::new())?;
            let activities: Vec<ApiFillActivity> = self.success_json(response, &[200])?;
            let count = activities.len();
            page_token = activities.last().map(|activity| activity.id.clone());
            for activity in activities {
                if activity.order_id == order_id {
                    executions.push(BrokerExecution {
                        execution_id: activity.id,
                        effective_price: activity.price.into_broker_decimal()?,
                        quantity: activity.qty.into_broker_decimal()?,
                        occurred_at: activity.transaction_time,
                    });
                }
            }
            if count < self.config.reconciliation_page_size {
                return Ok(executions);
            }
        }
        Err(BrokerError::Unavailable)
    }

    fn execute(
        &mut self,
        method: HttpMethod,
        path_and_query: String,
        body: Vec<u8>,
    ) -> Result<HttpResponse, BrokerError> {
        if !path_and_query.starts_with('/')
            || path_and_query.starts_with("//")
            || path_and_query.contains(['\r', '\n'])
        {
            return Err(rejected("invalid Alpaca request path"));
        }
        let mut headers = BTreeMap::new();
        self.credentials.apply(&mut headers);
        headers.insert("Accept".into(), "application/json".into());
        if !body.is_empty() {
            headers.insert("Content-Type".into(), "application/json".into());
        }
        self.transport
            .execute(HttpRequest {
                method,
                path_and_query,
                headers,
                body,
            })
            .map_err(|error| match error {
                TransportError::OutcomeUnknown if method.is_mutating() => {
                    BrokerError::AmbiguousOutcome
                }
                _ => BrokerError::Unavailable,
            })
    }

    fn success_json<R: for<'de> Deserialize<'de>>(
        &self,
        response: HttpResponse,
        expected: &[u16],
    ) -> Result<R, BrokerError> {
        if !expected.contains(&response.status) {
            return Err(api_rejection(response));
        }
        serde_json::from_slice(&response.body).map_err(|_| rejected("Alpaca returned invalid JSON"))
    }

    fn validate_intent(&self, intent: &OrderIntent) -> Result<(), BrokerError> {
        if !matches!(
            checked_notional(intent.proposal.limit_price, intent.proposal.quantity),
            Ok(notional) if notional == intent.authorized_notional
        ) {
            return Err(rejected(
                "authorized notional does not match price and quantity",
            ));
        }
        if intent.proposal.mode != self.config.environment.execution_mode() {
            return Err(rejected(
                "order execution mode does not match Alpaca environment",
            ));
        }
        if intent.proposal.venue != self.config.venue || intent.proposal.currency != "USD" {
            return Err(rejected(
                "order does not target the configured Alpaca USD venue",
            ));
        }
        validate_client_order_id(&intent.client_order_id)?;
        Ok(())
    }
}

impl<T> BrokerPort for AlpacaBroker<T>
where
    T: AlpacaTransport,
{
    fn submit(&mut self, intent: &OrderIntent) -> Result<BrokerAcknowledgement, BrokerError> {
        self.validate_intent(intent)?;
        self.submit_order(&AlpacaOrderRequest {
            client_order_id: intent.client_order_id.clone(),
            symbol: intent.proposal.symbol.clone(),
            side: intent.proposal.side,
            quantity: BrokerDecimal::try_new(micros_to_decimal(intent.proposal.quantity.0))
                .map_err(|_| rejected("invalid order quantity"))?,
            order_type: AlpacaOrderType::Limit,
            time_in_force: AlpacaTimeInForce::Day,
            limit_price: Some(
                BrokerDecimal::try_new(micros_to_decimal(intent.proposal.limit_price.0))
                    .map_err(|_| rejected("invalid order price"))?,
            ),
            extended_hours: self.config.extended_hours,
        })
        .map(|snapshot| snapshot.acknowledgement)
    }

    fn lookup_by_client_order_id(
        &mut self,
        client_order_id: &str,
    ) -> Result<Option<BrokerAcknowledgement>, BrokerError> {
        self.fetch_api_order(client_order_id)?
            .map(|order| normalize_order(order, Vec::new()).map(|value| value.acknowledgement))
            .transpose()
    }
}

impl<T> BrokerLifecyclePort for AlpacaBroker<T>
where
    T: AlpacaTransport,
{
    fn fetch_order_by_client_order_id(
        &mut self,
        client_order_id: &str,
    ) -> Result<Option<BrokerOrderSnapshot>, BrokerError> {
        let Some(order) = self.fetch_api_order(client_order_id)? else {
            return Ok(None);
        };
        let filled_quantity = order.filled_qty.clone().into_broker_decimal()?;
        let fills = if filled_quantity.as_str() == "0" {
            Vec::new()
        } else {
            self.fills_for_order(&order.id)?
        };
        normalize_order(order, fills).map(Some)
    }

    fn cancel_by_client_order_id(
        &mut self,
        client_order_id: &str,
    ) -> Result<BrokerOrderSnapshot, BrokerError> {
        let current = self
            .fetch_order_by_client_order_id(client_order_id)?
            .ok_or_else(|| rejected("Alpaca order was not found"))?;
        if current.state.is_terminal() {
            return Ok(current);
        }
        let response = self.execute(
            HttpMethod::Delete,
            format!("{ORDERS_PATH}/{}", current.acknowledgement.broker_order_id),
            Vec::new(),
        )?;
        if response.status != 204 {
            return Err(api_rejection(response));
        }
        self.fetch_order_by_client_order_id(client_order_id)?
            .ok_or_else(|| rejected("Alpaca order disappeared after cancel acknowledgement"))
    }
}

fn normalize_order(
    order: ApiOrder,
    executions: Vec<BrokerExecution>,
) -> Result<BrokerOrderSnapshot, BrokerError> {
    if order.id.trim().is_empty()
        || order.client_order_id.trim().is_empty()
        || order.symbol.trim().is_empty()
        || !matches!(order.side.as_str(), "buy" | "sell")
    {
        return Err(rejected("Alpaca returned an invalid order identity"));
    }
    let state = match order.status.as_str() {
        "pending_new" | "accepted" | "pending_cancel" | "pending_replace" => {
            BrokerOrderState::Pending
        }
        "new" | "accepted_for_bidding" | "calculated" | "held" => BrokerOrderState::Open,
        "partially_filled" => BrokerOrderState::PartiallyFilled,
        "filled" => BrokerOrderState::Filled,
        "canceled" | "expired" | "replaced" | "done_for_day" => BrokerOrderState::Canceled,
        "rejected" | "stopped" | "suspended" => BrokerOrderState::Failed,
        _ => return Err(rejected("Alpaca returned an unsupported order status")),
    };
    let accepted_at_ns = parse_timestamp_ns(&order.submitted_at)?;
    let filled_quantity = order.filled_qty.into_broker_decimal()?;
    let average_price = order
        .filled_avg_price
        .map(ApiDecimal::into_broker_decimal)
        .transpose()?;
    let requested_quantity = order.qty.into_broker_decimal()?;
    if decimal_greater(&filled_quantity, &requested_quantity)? {
        return Err(rejected("Alpaca reported an overfilled order"));
    }
    Ok(BrokerOrderSnapshot {
        acknowledgement: BrokerAcknowledgement {
            broker_order_id: order.id,
            client_order_id: order.client_order_id,
            accepted_at_ns,
        },
        state,
        executions,
        filled_quantity,
        average_price,
        updated_at: order.updated_at,
    })
}

fn normalize_account(account: ApiAccount) -> Result<AlpacaAccount, BrokerError> {
    Ok(AlpacaAccount {
        id: account.id,
        status: account.status,
        currency: account.currency,
        buying_power: signed(account.buying_power)?,
        cash: signed(account.cash)?,
        portfolio_value: signed(account.portfolio_value)?,
        equity: signed(account.equity)?,
        last_equity: signed(account.last_equity)?,
        long_market_value: signed(account.long_market_value)?,
        short_market_value: signed(account.short_market_value)?,
        initial_margin: signed(account.initial_margin)?,
        maintenance_margin: signed(account.maintenance_margin)?,
        daytrade_count: account.daytrade_count,
        trading_blocked: account.trading_blocked,
        transfers_blocked: account.transfers_blocked,
        account_blocked: account.account_blocked,
        trade_suspended_by_user: account.trade_suspended_by_user,
    })
}

fn normalize_position(position: ApiPosition) -> Result<AlpacaPosition, BrokerError> {
    Ok(AlpacaPosition {
        asset_id: position.asset_id,
        symbol: position.symbol,
        exchange: position.exchange,
        asset_class: position.asset_class,
        quantity: signed(position.qty)?,
        quantity_available: position.qty_available.map(signed).transpose()?,
        side: position.side,
        average_entry_price: signed(position.avg_entry_price)?,
        market_value: signed(position.market_value)?,
        cost_basis: signed(position.cost_basis)?,
        unrealized_pl: signed(position.unrealized_pl)?,
        unrealized_pl_percent: signed(position.unrealized_plpc)?,
        current_price: signed(position.current_price)?,
        last_day_price: signed(position.lastday_price)?,
        change_today: signed(position.change_today)?,
    })
}

fn signed(value: ApiDecimal) -> Result<SignedDecimal, BrokerError> {
    SignedDecimal::try_new(value.as_text())
        .map_err(|_| rejected("Alpaca returned an invalid signed decimal"))
}

fn parse_timestamp_ns(value: &str) -> Result<u64, BrokerError> {
    let timestamp = OffsetDateTime::parse(value, &Rfc3339)
        .map_err(|_| rejected("Alpaca returned an invalid timestamp"))?;
    u64::try_from(timestamp.unix_timestamp_nanos())
        .map_err(|_| rejected("Alpaca returned a pre-epoch timestamp"))
}

fn decimal_greater(left: &BrokerDecimal, right: &BrokerDecimal) -> Result<bool, BrokerError> {
    let left = decimal_parts(left.as_str())?;
    let right = decimal_parts(right.as_str())?;
    let scale = left.1.len().max(right.1.len());
    let mut left_digits = format!("{}{}", left.0, left.1);
    let mut right_digits = format!("{}{}", right.0, right.1);
    left_digits.extend(std::iter::repeat_n('0', scale - left.1.len()));
    right_digits.extend(std::iter::repeat_n('0', scale - right.1.len()));
    let width = left_digits.len().max(right_digits.len());
    Ok(format!("{left_digits:0>width$}") > format!("{right_digits:0>width$}"))
}

fn decimal_parts(value: &str) -> Result<(&str, &str), BrokerError> {
    let mut parts = value.split('.');
    let whole = parts.next().unwrap_or_default();
    let fraction = parts.next().unwrap_or_default();
    if whole.is_empty() || parts.next().is_some() {
        return Err(rejected("invalid normalized decimal"));
    }
    Ok((whole, fraction))
}

fn validate_client_order_id(value: &str) -> Result<(), BrokerError> {
    if value.is_empty() || value.len() > 128 || !value.bytes().all(|byte| byte.is_ascii_graphic()) {
        return Err(rejected(
            "client order ID must be 1 to 128 visible ASCII characters",
        ));
    }
    Ok(())
}

fn side(side: Side) -> &'static str {
    match side {
        Side::Buy => "buy",
        Side::Sell => "sell",
    }
}

fn api_rejection(response: HttpResponse) -> BrokerError {
    #[derive(Deserialize)]
    struct ApiErrorBody {
        message: Option<String>,
        code: Option<serde_json::Value>,
    }
    let detail = serde_json::from_slice::<ApiErrorBody>(&response.body)
        .ok()
        .and_then(|body| {
            body.message.map(|message| {
                let message: String = message
                    .chars()
                    .filter(|character| !character.is_control())
                    .take(256)
                    .collect();
                match body.code {
                    Some(code) => format!("{message} (code {code})"),
                    None => message,
                }
            })
        })
        .unwrap_or_else(|| "request rejected without a valid error body".into());
    rejected(format!("Alpaca HTTP {}: {detail}", response.status))
}

fn rejected(message: impl Into<String>) -> BrokerError {
    BrokerError::Rejected(message.into())
}

fn micros_to_decimal(value: u64) -> String {
    let whole = value / 1_000_000;
    let fractional = value % 1_000_000;
    if fractional == 0 {
        return whole.to_string();
    }
    let fraction = format!("{fractional:06}").trim_end_matches('0').to_owned();
    format!("{whole}.{fraction}")
}

fn percent_encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push_str(&format!("{byte:02X}"));
        }
    }
    encoded
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum MicrosConversionError {
    #[error("decimal has more than six non-zero fractional digits")]
    PrecisionLoss,
    #[error("decimal does not fit the Helios fixed-point range")]
    Overflow,
    #[error("decimal is invalid")]
    Invalid,
}

pub fn broker_decimal_to_micros(value: &BrokerDecimal) -> Result<u64, MicrosConversionError> {
    unsigned_text_to_micros(value.as_str())
}

pub fn signed_decimal_to_micros(value: &SignedDecimal) -> Result<i128, MicrosConversionError> {
    let (negative, magnitude) = value
        .as_str()
        .strip_prefix('-')
        .map_or((false, value.as_str()), |magnitude| (true, magnitude));
    let magnitude = i128::from(unsigned_text_to_micros(magnitude)?);
    if negative {
        magnitude
            .checked_neg()
            .ok_or(MicrosConversionError::Overflow)
    } else {
        Ok(magnitude)
    }
}

fn unsigned_text_to_micros(value: &str) -> Result<u64, MicrosConversionError> {
    let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
    if whole.is_empty()
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(MicrosConversionError::Invalid);
    }
    if fraction.len() > 6 && fraction.as_bytes()[6..].iter().any(|byte| *byte != b'0') {
        return Err(MicrosConversionError::PrecisionLoss);
    }
    let whole = whole
        .parse::<u64>()
        .map_err(|_| MicrosConversionError::Overflow)?;
    let fractional = &fraction[..fraction.len().min(6)];
    let mut fractional_micros = if fractional.is_empty() {
        0
    } else {
        fractional
            .parse::<u64>()
            .map_err(|_| MicrosConversionError::Invalid)?
    };
    for _ in fractional.len()..6 {
        fractional_micros = fractional_micros
            .checked_mul(10)
            .ok_or(MicrosConversionError::Overflow)?;
    }
    whole
        .checked_mul(1_000_000)
        .and_then(|value| value.checked_add(fractional_micros))
        .ok_or(MicrosConversionError::Overflow)
}

pub fn execution_identity(order_id: &str, execution_id: &str, occurred_at: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(order_id.as_bytes());
    digest.update([0]);
    digest.update(execution_id.as_bytes());
    digest.update([0]);
    digest.update(occurred_at.as_bytes());
    hex::encode(digest.finalize())
}

#[cfg(feature = "native-http")]
mod native;

#[cfg(feature = "native-stream")]
mod native_stream;

#[cfg(feature = "native-http")]
pub use native::*;

#[cfg(feature = "native-stream")]
pub use native_stream::*;

#[cfg(test)]
mod tests;
