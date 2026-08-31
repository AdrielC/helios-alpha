use helio_execution::BrokerDecimal;
use helio_scan::{SourceCheckpoint, SourceEnvelope, SourceId, SourceOffset, SourcePhase};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;
const MAX_EVENTS_PER_FRAME: usize = 10_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum AlpacaMarketEvent {
    Trade {
        symbol: String,
        trade_id: String,
        exchange: String,
        price: BrokerDecimal,
        size: BrokerDecimal,
        conditions: Vec<String>,
        tape: String,
    },
    Quote {
        symbol: String,
        bid_exchange: String,
        bid_price: BrokerDecimal,
        bid_size: BrokerDecimal,
        ask_exchange: String,
        ask_price: BrokerDecimal,
        ask_size: BrokerDecimal,
        conditions: Vec<String>,
        tape: String,
    },
    Bar {
        symbol: String,
        interval: String,
        open: BrokerDecimal,
        high: BrokerDecimal,
        low: BrokerDecimal,
        close: BrokerDecimal,
        volume: BrokerDecimal,
        trade_count: Option<u64>,
        vwap: Option<BrokerDecimal>,
    },
    Correction {
        symbol: String,
        original_trade_id: String,
        corrected_trade_id: String,
        original_price: BrokerDecimal,
        corrected_price: BrokerDecimal,
        original_size: BrokerDecimal,
        corrected_size: BrokerDecimal,
    },
    Cancel {
        symbol: String,
        trade_id: String,
        exchange: String,
        price: BrokerDecimal,
        size: BrokerDecimal,
        action: String,
        tape: String,
    },
    TradingStatus {
        symbol: String,
        status_code: String,
        reason_code: String,
        reason_message: String,
        tape: String,
    },
    Control {
        event_type: String,
        code: Option<u64>,
        message: Option<String>,
    },
}

impl AlpacaMarketEvent {
    fn partition(&self) -> &str {
        match self {
            Self::Trade { symbol, .. }
            | Self::Quote { symbol, .. }
            | Self::Bar { symbol, .. }
            | Self::Correction { symbol, .. }
            | Self::Cancel { symbol, .. }
            | Self::TradingStatus { symbol, .. } => symbol,
            Self::Control { .. } => "$control",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MarketDataError {
    #[error("Alpaca market-data frame exceeded the configured byte limit")]
    FrameTooLarge,
    #[error("Alpaca market-data frame was not valid JSON")]
    InvalidJson,
    #[error("Alpaca market-data frame exceeded the event-count limit")]
    TooManyEvents,
    #[error("Alpaca market-data event is missing or has an invalid {0} field")]
    InvalidField(&'static str),
    #[error("Alpaca market-data event type {0} is unsupported")]
    UnsupportedEvent(String),
    #[error("Alpaca market-data observation time regressed for partition {0}")]
    ObservationTimeRegression(String),
    #[error("Alpaca market-data partition sequence overflowed")]
    SequenceOverflow,
    #[error("market-data checkpoint belongs to a different source")]
    ForeignCheckpoint,
}

#[derive(Debug, Clone)]
pub struct AlpacaMarketNormalizer {
    source: SourceId,
    checkpoint: SourceCheckpoint,
}

impl AlpacaMarketNormalizer {
    pub fn new(feed: impl Into<String>) -> Self {
        let source = SourceId::new(format!("alpaca-market-data:{}", feed.into()));
        Self {
            checkpoint: SourceCheckpoint::empty(source.clone()),
            source,
        }
    }

    pub fn resume(
        feed: impl Into<String>,
        checkpoint: SourceCheckpoint,
    ) -> Result<Self, MarketDataError> {
        let source = SourceId::new(format!("alpaca-market-data:{}", feed.into()));
        if checkpoint.source != source {
            return Err(MarketDataError::ForeignCheckpoint);
        }
        Ok(Self { source, checkpoint })
    }

    pub fn checkpoint(&self) -> SourceCheckpoint {
        self.checkpoint.clone()
    }

    pub fn normalize_frame(
        &mut self,
        frame: &[u8],
        observed_at_ns: i64,
    ) -> Result<Vec<SourceEnvelope<AlpacaMarketEvent>>, MarketDataError> {
        if frame.len() > MAX_FRAME_BYTES {
            return Err(MarketDataError::FrameTooLarge);
        }
        let value: Value =
            serde_json::from_slice(frame).map_err(|_| MarketDataError::InvalidJson)?;
        let messages = match value {
            Value::Array(messages) => messages,
            Value::Object(_) => vec![value],
            _ => return Err(MarketDataError::InvalidJson),
        };
        if messages.len() > MAX_EVENTS_PER_FRAME {
            return Err(MarketDataError::TooManyEvents);
        }

        let mut staged = self.checkpoint.clone();
        let mut records = Vec::with_capacity(messages.len());
        for message in messages {
            let object = message.as_object().ok_or(MarketDataError::InvalidJson)?;
            let (event_time, event) = parse_market_event(object, observed_at_ns)?;
            let partition = event.partition().to_owned();
            if staged
                .available_at
                .get(&partition)
                .is_some_and(|previous| observed_at_ns < *previous)
            {
                return Err(MarketDataError::ObservationTimeRegression(partition));
            }
            let sequence = staged
                .positions
                .get(&partition)
                .copied()
                .unwrap_or(0)
                .checked_add(1)
                .ok_or(MarketDataError::SequenceOverflow)?;
            staged.positions.insert(partition.clone(), sequence);
            staged
                .available_at
                .insert(partition.clone(), observed_at_ns);
            records.push(SourceEnvelope {
                source: self.source.clone(),
                offset: SourceOffset::new(partition, sequence),
                event_time,
                available_at: observed_at_ns,
                observed_at: observed_at_ns,
                phase: SourcePhase::Live,
                payload: event,
            });
        }
        self.checkpoint = staged;
        Ok(records)
    }
}

fn parse_market_event(
    object: &Map<String, Value>,
    observed_at_ns: i64,
) -> Result<(i64, AlpacaMarketEvent), MarketDataError> {
    let event_type = text(object, "T")?;
    let event_time = object
        .get("t")
        .map(timestamp)
        .transpose()?
        .unwrap_or(observed_at_ns);
    let event = match event_type.as_str() {
        "t" => AlpacaMarketEvent::Trade {
            symbol: text(object, "S")?,
            trade_id: scalar_text(object, "i")?,
            exchange: scalar_text(object, "x")?,
            price: decimal(object, "p")?,
            size: decimal(object, "s")?,
            conditions: string_array(object, "c")?,
            tape: optional_scalar_text(object, "z").unwrap_or_default(),
        },
        "q" => AlpacaMarketEvent::Quote {
            symbol: text(object, "S")?,
            bid_exchange: scalar_text(object, "bx")?,
            bid_price: decimal(object, "bp")?,
            bid_size: decimal(object, "bs")?,
            ask_exchange: scalar_text(object, "ax")?,
            ask_price: decimal(object, "ap")?,
            ask_size: decimal(object, "as")?,
            conditions: string_array(object, "c")?,
            tape: optional_scalar_text(object, "z").unwrap_or_default(),
        },
        "b" | "u" | "d" => AlpacaMarketEvent::Bar {
            symbol: text(object, "S")?,
            interval: match event_type.as_str() {
                "b" => "minute",
                "u" => "updated_minute",
                _ => "daily",
            }
            .into(),
            open: decimal(object, "o")?,
            high: decimal(object, "h")?,
            low: decimal(object, "l")?,
            close: decimal(object, "c")?,
            volume: decimal(object, "v")?,
            trade_count: optional_u64(object, "n")?,
            vwap: optional_decimal(object, "vw")?,
        },
        "c" => AlpacaMarketEvent::Correction {
            symbol: text(object, "S")?,
            original_trade_id: scalar_text(object, "oi")?,
            corrected_trade_id: scalar_text(object, "ci")?,
            original_price: decimal(object, "op")?,
            corrected_price: decimal(object, "cp")?,
            original_size: decimal(object, "os")?,
            corrected_size: decimal(object, "cs")?,
        },
        "x" => AlpacaMarketEvent::Cancel {
            symbol: text(object, "S")?,
            trade_id: scalar_text(object, "i")?,
            exchange: scalar_text(object, "x")?,
            price: decimal(object, "p")?,
            size: decimal(object, "s")?,
            action: optional_scalar_text(object, "a").unwrap_or_default(),
            tape: optional_scalar_text(object, "z").unwrap_or_default(),
        },
        "s" => AlpacaMarketEvent::TradingStatus {
            symbol: text(object, "S")?,
            status_code: scalar_text(object, "sc")?,
            reason_code: optional_scalar_text(object, "rc").unwrap_or_default(),
            reason_message: optional_scalar_text(object, "rm").unwrap_or_default(),
            tape: optional_scalar_text(object, "z").unwrap_or_default(),
        },
        "success" | "subscription" | "error" => AlpacaMarketEvent::Control {
            event_type,
            code: optional_u64(object, "code")?,
            message: object
                .get("msg")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
        },
        other => return Err(MarketDataError::UnsupportedEvent(other.into())),
    };
    Ok((event_time, event))
}

fn text(object: &Map<String, Value>, field: &'static str) -> Result<String, MarketDataError> {
    object
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or(MarketDataError::InvalidField(field))
}

fn scalar_text(
    object: &Map<String, Value>,
    field: &'static str,
) -> Result<String, MarketDataError> {
    optional_scalar_text(object, field).ok_or(MarketDataError::InvalidField(field))
}

fn optional_scalar_text(object: &Map<String, Value>, field: &str) -> Option<String> {
    match object.get(field)? {
        Value::String(value) if !value.is_empty() => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn decimal(
    object: &Map<String, Value>,
    field: &'static str,
) -> Result<BrokerDecimal, MarketDataError> {
    let value = scalar_text(object, field)?;
    BrokerDecimal::try_new(value).map_err(|_| MarketDataError::InvalidField(field))
}

fn optional_decimal(
    object: &Map<String, Value>,
    field: &'static str,
) -> Result<Option<BrokerDecimal>, MarketDataError> {
    object
        .get(field)
        .filter(|value| !value.is_null())
        .map(|_| decimal(object, field))
        .transpose()
}

fn optional_u64(
    object: &Map<String, Value>,
    field: &'static str,
) -> Result<Option<u64>, MarketDataError> {
    let Some(value) = object.get(field) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    value
        .as_u64()
        .or_else(|| value.as_str()?.parse().ok())
        .map(Some)
        .ok_or(MarketDataError::InvalidField(field))
}

fn string_array(
    object: &Map<String, Value>,
    field: &'static str,
) -> Result<Vec<String>, MarketDataError> {
    let Some(value) = object.get(field) else {
        return Ok(Vec::new());
    };
    value
        .as_array()
        .ok_or(MarketDataError::InvalidField(field))?
        .iter()
        .map(|value| match value {
            Value::String(value) => Ok(value.clone()),
            Value::Number(value) => Ok(value.to_string()),
            _ => Err(MarketDataError::InvalidField(field)),
        })
        .collect()
}

fn timestamp(value: &Value) -> Result<i64, MarketDataError> {
    let raw = value.as_str().ok_or(MarketDataError::InvalidField("t"))?;
    let timestamp =
        OffsetDateTime::parse(raw, &Rfc3339).map_err(|_| MarketDataError::InvalidField("t"))?;
    i64::try_from(timestamp.unix_timestamp_nanos()).map_err(|_| MarketDataError::InvalidField("t"))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlpacaTradeUpdate {
    pub event: String,
    pub execution_id: Option<String>,
    pub timestamp: String,
    pub order_id: String,
    pub client_order_id: String,
    pub symbol: String,
    pub side: String,
    pub status: String,
    pub quantity: BrokerDecimal,
    pub filled_quantity: BrokerDecimal,
    pub filled_average_price: Option<BrokerDecimal>,
    pub execution_price: Option<BrokerDecimal>,
    pub execution_quantity: Option<BrokerDecimal>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum AlpacaTradingFrame {
    Authorization { status: String },
    Listening { streams: Vec<String> },
    TradeUpdate { update: Box<AlpacaTradeUpdate> },
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TradingFrameError {
    #[error("Alpaca trading frame exceeded the configured byte limit")]
    FrameTooLarge,
    #[error("Alpaca trading frame was invalid")]
    InvalidFrame,
    #[error("Alpaca trading frame used an unsupported stream {0}")]
    UnsupportedStream(String),
}

pub fn parse_trading_frame(frame: &[u8]) -> Result<AlpacaTradingFrame, TradingFrameError> {
    if frame.len() > MAX_FRAME_BYTES {
        return Err(TradingFrameError::FrameTooLarge);
    }
    let value: Value =
        serde_json::from_slice(frame).map_err(|_| TradingFrameError::InvalidFrame)?;
    let object = value.as_object().ok_or(TradingFrameError::InvalidFrame)?;
    let stream = object
        .get("stream")
        .and_then(Value::as_str)
        .ok_or(TradingFrameError::InvalidFrame)?;
    let data = object
        .get("data")
        .and_then(Value::as_object)
        .ok_or(TradingFrameError::InvalidFrame)?;
    match stream {
        "authorization" => Ok(AlpacaTradingFrame::Authorization {
            status: data
                .get("status")
                .and_then(Value::as_str)
                .ok_or(TradingFrameError::InvalidFrame)?
                .into(),
        }),
        "listening" => Ok(AlpacaTradingFrame::Listening {
            streams: data
                .get("streams")
                .and_then(Value::as_array)
                .ok_or(TradingFrameError::InvalidFrame)?
                .iter()
                .map(|stream| {
                    stream
                        .as_str()
                        .map(ToOwned::to_owned)
                        .ok_or(TradingFrameError::InvalidFrame)
                })
                .collect::<Result<_, _>>()?,
        }),
        "trade_updates" => Ok(AlpacaTradingFrame::TradeUpdate {
            update: Box::new(parse_trade_update(data)?),
        }),
        other => Err(TradingFrameError::UnsupportedStream(other.into())),
    }
}

fn parse_trade_update(data: &Map<String, Value>) -> Result<AlpacaTradeUpdate, TradingFrameError> {
    let order = data
        .get("order")
        .and_then(Value::as_object)
        .ok_or(TradingFrameError::InvalidFrame)?;
    let required_text = |object: &Map<String, Value>, field: &str| {
        object
            .get(field)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .ok_or(TradingFrameError::InvalidFrame)
    };
    let parse_decimal =
        |value: Option<&Value>| -> Result<Option<BrokerDecimal>, TradingFrameError> {
            let Some(value) = value.filter(|value| !value.is_null()) else {
                return Ok(None);
            };
            let text = match value {
                Value::String(value) => value.clone(),
                Value::Number(value) => value.to_string(),
                _ => return Err(TradingFrameError::InvalidFrame),
            };
            BrokerDecimal::try_new(text)
                .map(Some)
                .map_err(|_| TradingFrameError::InvalidFrame)
        };
    Ok(AlpacaTradeUpdate {
        event: required_text(data, "event")?,
        execution_id: data
            .get("execution_id")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        timestamp: required_text(data, "timestamp")?,
        order_id: required_text(order, "id")?,
        client_order_id: required_text(order, "client_order_id")?,
        symbol: required_text(order, "symbol")?,
        side: required_text(order, "side")?,
        status: required_text(order, "status")?,
        quantity: parse_decimal(order.get("qty"))?.ok_or(TradingFrameError::InvalidFrame)?,
        filled_quantity: parse_decimal(order.get("filled_qty"))?
            .ok_or(TradingFrameError::InvalidFrame)?,
        filled_average_price: parse_decimal(order.get("filled_avg_price"))?,
        execution_price: parse_decimal(data.get("price"))?,
        execution_quantity: parse_decimal(data.get("qty"))?,
    })
}

pub fn market_data_auth_message(credentials: &super::AlpacaCredentials) -> serde_json::Value {
    credentials.websocket_auth_message()
}

pub fn market_data_subscribe_message(
    trades: &[String],
    quotes: &[String],
    bars: &[String],
    updated_bars: &[String],
    statuses: &[String],
) -> serde_json::Value {
    serde_json::json!({
        "action": "subscribe",
        "trades": trades,
        "quotes": quotes,
        "bars": bars,
        "updatedBars": updated_bars,
        "statuses": statuses,
    })
}

pub fn trading_stream_auth_message(credentials: &super::AlpacaCredentials) -> serde_json::Value {
    serde_json::json!({
        "action": "authenticate",
        "data": {
            "key_id": credentials.key_id,
            "secret_key": credentials.secret_key,
        }
    })
}

pub fn trading_stream_listen_message() -> serde_json::Value {
    serde_json::json!({
        "action": "listen",
        "data": { "streams": ["trade_updates"] }
    })
}
