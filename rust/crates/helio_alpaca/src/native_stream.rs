use futures_util::{SinkExt, StreamExt};
use thiserror::Error;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

use crate::{
    market_data_auth_message, market_data_subscribe_message, trading_stream_auth_message,
    trading_stream_listen_message, AlpacaCredentials, AlpacaEnvironment,
    ALPACA_MARKET_DATA_STREAM_ORIGIN,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlpacaStockFeed {
    Iex,
    Sip,
    DelayedSip,
}

pub struct AlpacaTradingStream {
    socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

impl std::fmt::Debug for AlpacaTradingStream {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AlpacaTradingStream")
            .finish_non_exhaustive()
    }
}

impl AlpacaTradingStream {
    pub async fn connect(
        environment: AlpacaEnvironment,
        credentials: &AlpacaCredentials,
    ) -> Result<Self, StreamError> {
        let (mut socket, _) = connect_async(environment.trading_stream_url())
            .await
            .map_err(|_| StreamError::Connect)?;
        let auth = serde_json::to_string(&trading_stream_auth_message(credentials))
            .map_err(|_| StreamError::Serialization)?;
        socket
            .send(Message::Text(auth.into()))
            .await
            .map_err(|_| StreamError::Send)?;
        let listen = serde_json::to_string(&trading_stream_listen_message())
            .map_err(|_| StreamError::Serialization)?;
        socket
            .send(Message::Text(listen.into()))
            .await
            .map_err(|_| StreamError::Send)?;
        Ok(Self { socket })
    }

    pub async fn next_frame(&mut self) -> Result<Vec<u8>, StreamError> {
        next_data_frame(&mut self.socket).await
    }

    pub async fn close(mut self) -> Result<(), StreamError> {
        self.socket.close(None).await.map_err(|_| StreamError::Send)
    }
}

async fn next_data_frame(
    socket: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
) -> Result<Vec<u8>, StreamError> {
    loop {
        let message = socket
            .next()
            .await
            .ok_or(StreamError::Closed)?
            .map_err(|_| StreamError::Receive)?;
        match message {
            Message::Text(text) => return Ok(text.as_bytes().to_vec()),
            Message::Binary(bytes) => return Ok(bytes.to_vec()),
            Message::Ping(bytes) => socket
                .send(Message::Pong(bytes))
                .await
                .map_err(|_| StreamError::Send)?,
            Message::Close(_) => return Err(StreamError::Closed),
            Message::Pong(_) | Message::Frame(_) => {}
        }
    }
}

impl AlpacaStockFeed {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Iex => "iex",
            Self::Sip => "sip",
            Self::DelayedSip => "delayed_sip",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarketStreamConfig {
    pub feed: AlpacaStockFeed,
    pub trades: Vec<String>,
    pub quotes: Vec<String>,
    pub bars: Vec<String>,
    pub updated_bars: Vec<String>,
    pub statuses: Vec<String>,
}

impl MarketStreamConfig {
    pub fn stocks(feed: AlpacaStockFeed, symbols: Vec<String>) -> Result<Self, StreamError> {
        validate_symbols(&symbols)?;
        Ok(Self {
            feed,
            trades: symbols.clone(),
            quotes: symbols.clone(),
            bars: symbols.clone(),
            updated_bars: symbols.clone(),
            statuses: symbols,
        })
    }

    pub fn validate(&self) -> Result<(), StreamError> {
        if self.trades.is_empty()
            && self.quotes.is_empty()
            && self.bars.is_empty()
            && self.updated_bars.is_empty()
            && self.statuses.is_empty()
        {
            return Err(StreamError::EmptySubscription);
        }
        for symbols in [
            &self.trades,
            &self.quotes,
            &self.bars,
            &self.updated_bars,
            &self.statuses,
        ] {
            validate_symbols(symbols)?;
        }
        Ok(())
    }

    pub fn url(&self) -> String {
        format!(
            "{ALPACA_MARKET_DATA_STREAM_ORIGIN}/v2/{}",
            self.feed.as_str()
        )
    }
}

fn validate_symbols(symbols: &[String]) -> Result<(), StreamError> {
    if symbols.len() > 1_000
        || symbols.iter().any(|symbol| {
            symbol.is_empty()
                || symbol.len() > 32
                || !symbol
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'-')
        })
    {
        return Err(StreamError::InvalidSymbol);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum StreamError {
    #[error("Alpaca stream subscription must not be empty")]
    EmptySubscription,
    #[error("Alpaca stream contains an invalid symbol or too many symbols")]
    InvalidSymbol,
    #[error("Alpaca WebSocket connection failed")]
    Connect,
    #[error("Alpaca WebSocket authentication or subscription send failed")]
    Send,
    #[error("Alpaca WebSocket receive failed")]
    Receive,
    #[error("Alpaca WebSocket closed")]
    Closed,
    #[error("Alpaca WebSocket control serialization failed")]
    Serialization,
}

pub struct AlpacaMarketStream {
    socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

impl std::fmt::Debug for AlpacaMarketStream {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AlpacaMarketStream")
            .finish_non_exhaustive()
    }
}

impl AlpacaMarketStream {
    pub async fn connect(
        credentials: &AlpacaCredentials,
        config: &MarketStreamConfig,
    ) -> Result<Self, StreamError> {
        config.validate()?;
        let (mut socket, _) = connect_async(config.url())
            .await
            .map_err(|_| StreamError::Connect)?;
        let auth = serde_json::to_string(&market_data_auth_message(credentials))
            .map_err(|_| StreamError::Serialization)?;
        socket
            .send(Message::Text(auth.into()))
            .await
            .map_err(|_| StreamError::Send)?;
        let subscribe = serde_json::to_string(&market_data_subscribe_message(
            &config.trades,
            &config.quotes,
            &config.bars,
            &config.updated_bars,
            &config.statuses,
        ))
        .map_err(|_| StreamError::Serialization)?;
        socket
            .send(Message::Text(subscribe.into()))
            .await
            .map_err(|_| StreamError::Send)?;
        Ok(Self { socket })
    }

    pub async fn next_frame(&mut self) -> Result<Vec<u8>, StreamError> {
        next_data_frame(&mut self.socket).await
    }

    pub async fn close(mut self) -> Result<(), StreamError> {
        self.socket.close(None).await.map_err(|_| StreamError::Send)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_urls_and_symbols_are_closed_over_known_origins() {
        let config = MarketStreamConfig::stocks(AlpacaStockFeed::Iex, vec!["SPY".into()]).unwrap();
        assert_eq!(config.url(), "wss://stream.data.alpaca.markets/v2/iex");
        assert_eq!(
            MarketStreamConfig::stocks(AlpacaStockFeed::Iex, vec!["../SPY".into()]),
            Err(StreamError::InvalidSymbol)
        );
    }
}
