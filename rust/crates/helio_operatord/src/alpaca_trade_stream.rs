use crate::alpaca_paper::AlpacaTradeUpdatePort;
use crate::store::OperatorStore;
use crate::types::{HealthState, SourceView};
use helio_alpaca::{
    parse_trading_frame, AlpacaCredentials, AlpacaEnvironment, AlpacaTradingFrame,
    AlpacaTradingStream,
};
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

const SOURCE_NAME: &str = "alpaca-trade-updates";
const DISCONNECTED_LAG_MS: u64 = 86_400_000;

pub async fn run_alpaca_trade_updates(
    credentials: AlpacaCredentials,
    port: Arc<dyn AlpacaTradeUpdatePort>,
    store: Arc<OperatorStore>,
) {
    let mut backoff = Duration::from_secs(1);
    loop {
        let mut stream =
            match AlpacaTradingStream::connect(AlpacaEnvironment::Paper, &credentials).await {
                Ok(stream) => {
                    info!("Alpaca paper trade-update stream connected");
                    backoff = Duration::from_secs(1);
                    stream
                }
                Err(error) => {
                    warn!(error = %error, "Alpaca trade-update connection failed");
                    publish_source(
                        &store,
                        HealthState::Stale,
                        "No authenticated trade-update connection",
                    )
                    .await;
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(Duration::from_secs(30));
                    continue;
                }
            };
        loop {
            let frame = match stream.next_frame().await {
                Ok(frame) => frame,
                Err(error) => {
                    warn!(error = %error, "Alpaca trade-update stream interrupted");
                    publish_source(
                        &store,
                        HealthState::Degraded,
                        "Trade-update stream interrupted; reconnecting",
                    )
                    .await;
                    break;
                }
            };
            match parse_trading_frame(&frame) {
                Ok(AlpacaTradingFrame::Authorization { status }) if status == "authorized" => {
                    publish_source(
                        &store,
                        HealthState::Healthy,
                        "Authenticated paper order lifecycle stream",
                    )
                    .await;
                }
                Ok(AlpacaTradingFrame::Authorization { status }) => {
                    warn!(status, "Alpaca trade-update authorization rejected");
                    publish_source(
                        &store,
                        HealthState::Stale,
                        "Trade-update authorization rejected",
                    )
                    .await;
                    break;
                }
                Ok(AlpacaTradingFrame::Listening { streams }) => {
                    if !streams.iter().any(|stream| stream == "trade_updates") {
                        warn!("Alpaca did not admit the trade_updates subscription");
                        break;
                    }
                }
                Ok(AlpacaTradingFrame::TradeUpdate { update }) => {
                    if let Err(error) = port.reconcile_trade_update(*update, store.clone()).await {
                        warn!(error = %error, "Alpaca trade update failed OMS reconciliation");
                        publish_source(
                            &store,
                            HealthState::Degraded,
                            "A trade update failed fail-closed OMS reconciliation",
                        )
                        .await;
                    }
                }
                Err(error) => {
                    warn!(error = %error, "Alpaca trade-update frame rejected");
                    break;
                }
            }
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(30));
    }
}

async fn publish_source(store: &OperatorStore, health: HealthState, detail: &str) {
    let detail = detail.to_owned();
    if let Err(error) = store
        .mutate_snapshot(|snapshot| {
            let is_healthy = health == HealthState::Healthy;
            let source = SourceView {
                name: SOURCE_NAME.into(),
                channel: "wss:trade_updates".into(),
                health,
                lag_ms: if is_healthy { 0 } else { DISCONNECTED_LAG_MS },
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
            Ok(())
        })
        .await
    {
        warn!(error = %error, "Failed to publish Alpaca trade-update health");
    }
}
