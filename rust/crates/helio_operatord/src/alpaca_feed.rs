use crate::alpaca_paper::{
    ExecutionClock, InMemoryMarketReferencePort, MarketReference, SystemExecutionClock,
};
use crate::store::OperatorStore;
use crate::time_series::InMemoryTimeSeriesPort;
use crate::types::{HealthState, SourceView, TimeSeriesPoint};
use helio_alpaca::{
    broker_decimal_to_micros, AlpacaCredentials, AlpacaMarketEvent, AlpacaMarketNormalizer,
    AlpacaMarketStream, MarketStreamConfig,
};
use std::sync::Arc;
use std::time::Duration;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use tracing::{info, warn};

const SOURCE_NAME: &str = "alpaca-market-data";
const PUBLISH_INTERVAL_NS: u64 = 250_000_000;
const RETAINED_CHART_POINTS: usize = 100_000;
const DISCONNECTED_LAG_MS: u64 = 86_400_000;

pub async fn run_alpaca_market_feed(
    credentials: AlpacaCredentials,
    config: MarketStreamConfig,
    market: Arc<InMemoryMarketReferencePort>,
    store: Arc<OperatorStore>,
    time_series: Arc<InMemoryTimeSeriesPort>,
) {
    let mut normalizer = AlpacaMarketNormalizer::new(format!("v2/{}", config.feed.as_str()));
    let clock = SystemExecutionClock;
    let mut backoff = Duration::from_secs(1);
    loop {
        let mut stream = match AlpacaMarketStream::connect(&credentials, &config).await {
            Ok(stream) => {
                info!(
                    feed = config.feed.as_str(),
                    "Alpaca market-data stream connected"
                );
                backoff = Duration::from_secs(1);
                stream
            }
            Err(error) => {
                warn!(error = %error, "Alpaca market-data connection failed");
                publish_source(
                    &store,
                    HealthState::Stale,
                    DISCONNECTED_LAG_MS,
                    "No authenticated market-data connection",
                )
                .await;
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(30));
                continue;
            }
        };

        let mut last_publish_ns = 0_u64;
        loop {
            let frame = match stream.next_frame().await {
                Ok(frame) => frame,
                Err(error) => {
                    warn!(error = %error, "Alpaca market-data stream interrupted");
                    publish_source(
                        &store,
                        HealthState::Degraded,
                        DISCONNECTED_LAG_MS,
                        "Stream interrupted; reconnecting",
                    )
                    .await;
                    break;
                }
            };
            let observed_at_ns = match clock.now_ns() {
                Ok(value) => value,
                Err(error) => {
                    warn!(error = %error, "Market-data observation clock failed");
                    break;
                }
            };
            let observed_at_i64 = match i64::try_from(observed_at_ns) {
                Ok(value) => value,
                Err(_) => {
                    warn!("Market-data observation timestamp exceeded source range");
                    break;
                }
            };
            let records = match normalizer.normalize_frame(&frame, observed_at_i64) {
                Ok(records) => records,
                Err(error) => {
                    warn!(error = %error, "Alpaca market-data frame rejected");
                    break;
                }
            };

            let mut max_lag_ms = 0_u64;
            let mut control_error = false;
            for record in records {
                let lag_ns = record.available_at.saturating_sub(record.event_time);
                max_lag_ms = max_lag_ms.max(u64::try_from(lag_ns).unwrap_or(u64::MAX) / 1_000_000);
                match apply_record(
                    &record.payload,
                    record.event_time,
                    observed_at_ns,
                    &market,
                    &time_series,
                ) {
                    Ok(RecordDisposition::Applied | RecordDisposition::Ignored) => {}
                    Ok(RecordDisposition::ControlError) => control_error = true,
                    Err(error) => {
                        warn!(error = %error, "Alpaca market-data record rejected");
                        control_error = true;
                    }
                }
            }
            if control_error {
                publish_source(
                    &store,
                    HealthState::Degraded,
                    max_lag_ms,
                    "Feed returned an error or an unrepresentable fixed-point value",
                )
                .await;
                break;
            }
            if observed_at_ns.saturating_sub(last_publish_ns) >= PUBLISH_INTERVAL_NS {
                publish_source(
                    &store,
                    HealthState::Healthy,
                    max_lag_ms,
                    "Authenticated Alpaca market-data stream",
                )
                .await;
                last_publish_ns = observed_at_ns;
            }
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(30));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecordDisposition {
    Applied,
    Ignored,
    ControlError,
}

fn apply_record(
    event: &AlpacaMarketEvent,
    event_time_ns: i64,
    observed_at_ns: u64,
    market: &InMemoryMarketReferencePort,
    time_series: &InMemoryTimeSeriesPort,
) -> Result<RecordDisposition, String> {
    match event {
        // Quotes are the admitted execution reference. A trade can later be corrected or
        // canceled, so it must not silently become the risk price.
        AlpacaMarketEvent::Trade { .. } => Ok(RecordDisposition::Ignored),
        AlpacaMarketEvent::Quote {
            symbol,
            bid_price,
            ask_price,
            ..
        } => {
            let bid = broker_decimal_to_micros(bid_price).map_err(|error| error.to_string())?;
            let ask = broker_decimal_to_micros(ask_price).map_err(|error| error.to_string())?;
            let conservative = bid.max(ask);
            market
                .update(MarketReference {
                    symbol: symbol.clone(),
                    price: helio_execution::PriceMicros(conservative),
                    observed_at_ns,
                })
                .map_err(|error| error.to_string())?;
            Ok(RecordDisposition::Applied)
        }
        AlpacaMarketEvent::Bar {
            symbol,
            open,
            high,
            low,
            close,
            volume,
            ..
        } => {
            update_reference(market, symbol, close, observed_at_ns)?;
            let timestamp = format_ns(event_time_ns)?;
            let available_at = format_ns(i64::try_from(observed_at_ns).map_err(|_| "timestamp")?)?;
            time_series
                .append_point(
                    "market-ohlc",
                    TimeSeriesPoint::Ohlc {
                        timestamp: timestamp.clone(),
                        available_at: available_at.clone(),
                        open: display_decimal(open)?,
                        high: display_decimal(high)?,
                        low: display_decimal(low)?,
                        close: display_decimal(close)?,
                    },
                    RETAINED_CHART_POINTS,
                )
                .map_err(|error| error.to_string())?;
            time_series
                .append_point(
                    "market-volume",
                    TimeSeriesPoint::Scalar {
                        timestamp,
                        available_at,
                        value: display_decimal(volume)?,
                        color: None,
                    },
                    RETAINED_CHART_POINTS,
                )
                .map_err(|error| error.to_string())?;
            Ok(RecordDisposition::Applied)
        }
        AlpacaMarketEvent::Control { event_type, .. } if event_type == "error" => {
            Ok(RecordDisposition::ControlError)
        }
        AlpacaMarketEvent::Control { .. }
        | AlpacaMarketEvent::Correction { .. }
        | AlpacaMarketEvent::Cancel { .. }
        | AlpacaMarketEvent::TradingStatus { .. } => Ok(RecordDisposition::Ignored),
    }
}

fn update_reference(
    market: &InMemoryMarketReferencePort,
    symbol: &str,
    price: &helio_execution::BrokerDecimal,
    observed_at_ns: u64,
) -> Result<(), String> {
    let price = broker_decimal_to_micros(price).map_err(|error| error.to_string())?;
    market
        .update(MarketReference {
            symbol: symbol.into(),
            price: helio_execution::PriceMicros(price),
            observed_at_ns,
        })
        .map_err(|error| error.to_string())
}

fn display_decimal(value: &helio_execution::BrokerDecimal) -> Result<f64, String> {
    let parsed = value
        .as_str()
        .parse::<f64>()
        .map_err(|error| error.to_string())?;
    if parsed.is_finite() {
        Ok(parsed)
    } else {
        Err("display decimal is not finite".into())
    }
}

fn format_ns(value: i64) -> Result<String, String> {
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(value))
        .map_err(|error| error.to_string())?
        .format(&Rfc3339)
        .map_err(|error| error.to_string())
}

async fn publish_source(store: &OperatorStore, health: HealthState, lag_ms: u64, detail: &str) {
    let detail = detail.to_owned();
    let result = store
        .mutate_snapshot(|snapshot| {
            let source = SourceView {
                name: SOURCE_NAME.into(),
                channel: "wss:v2/iex".into(),
                health,
                lag_ms,
                watermark: snapshot.observed_at.clone(),
                detail,
            };
            if let Some(existing) = snapshot
                .sources
                .iter_mut()
                .find(|source| source.name == SOURCE_NAME)
            {
                *existing = source;
            } else {
                snapshot.sources.push(source);
            }
            snapshot.risk.source_lag_ms = lag_ms;
            Ok(())
        })
        .await;
    if let Err(error) = result {
        warn!(error = %error, "Failed to publish Alpaca source health");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alpaca_paper::MarketReferencePort;
    use helio_execution::BrokerDecimal;

    #[test]
    fn quote_reference_is_conservative_and_exact() {
        let market = InMemoryMarketReferencePort::default();
        let time_series = InMemoryTimeSeriesPort::new("paper", Vec::new(), Vec::new());
        apply_record(
            &AlpacaMarketEvent::Quote {
                symbol: "SPY".into(),
                bid_exchange: "V".into(),
                bid_price: BrokerDecimal::try_new("499.12").unwrap(),
                bid_size: BrokerDecimal::try_new("10").unwrap(),
                ask_exchange: "V".into(),
                ask_price: BrokerDecimal::try_new("499.13").unwrap(),
                ask_size: BrokerDecimal::try_new("10").unwrap(),
                conditions: Vec::new(),
                tape: "C".into(),
            },
            10,
            20,
            &market,
            &time_series,
        )
        .unwrap();
        assert_eq!(market.latest("SPY").unwrap().price.0, 499_130_000);
    }
}
