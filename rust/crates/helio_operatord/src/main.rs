use helio_alpaca::{
    AlpacaBroker, AlpacaConfig, AlpacaCredentials, AlpacaStockFeed, MarketStreamConfig,
    ReqwestAlpacaTransport,
};
use helio_execution::RiskPolicy;
use helio_operatord::fixtures::{default_catalog, default_forecast_bundles, empty_snapshot};
use helio_operatord::{
    connect_golem_paper_ports, router, run_alpaca_market_feed, run_alpaca_trade_updates,
    watch_projection_file, AlpacaPaperCommandExecutor, AlpacaTradeUpdatePort, AppState,
    CommandAuth, CommandExecutor, GolemClientSettings, GolemEndpoint, InMemoryMarketReferencePort,
    InMemoryTimeSeriesPort, MarketReference, OperatorStore, PaperServicePorts,
    ReadOnlyCommandExecutor, SystemExecutionClock,
};
use helio_time::VenueSchedule;
use std::env;
use std::path::PathBuf;
use std::sync::Arc;
use time::Duration;
use tokio::net::TcpListener;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

struct AlpacaPaperRuntime {
    executor: Arc<dyn CommandExecutor>,
    update_port: Arc<dyn AlpacaTradeUpdatePort>,
    market_credentials: AlpacaCredentials,
    trade_credentials: AlpacaCredentials,
    stream_config: MarketStreamConfig,
    market: Arc<InMemoryMarketReferencePort>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();
    let snapshot = match env::var("HELIOS_SNAPSHOT_PATH") {
        Ok(path) => serde_json::from_slice(&std::fs::read(path)?)?,
        Err(_) => empty_snapshot(),
    };
    let account_id = snapshot.context.account_id.clone();
    let store = OperatorStore::new(snapshot)?;
    let auth = command_auth();
    let time_series = Arc::new(InMemoryTimeSeriesPort::new(
        account_id.clone(),
        default_catalog(),
        default_forecast_bundles(),
    ));
    if let Some(path) = env::var_os("HELIOS_TIME_SERIES_PROJECTION_PATH").map(PathBuf::from) {
        let digest = helio_operatord::load_projection_file(&path, time_series.as_ref())?;
        let interval = projection_poll_interval()?;
        tokio::spawn(watch_projection_file(
            path,
            time_series.clone(),
            interval,
            digest,
        ));
    }
    let command_executor = command_executor(&account_id, &store, &time_series).await;
    let state = AppState::new(store, auth, command_executor, time_series);
    let static_dir = env::var_os("HELIOS_STATIC_DIR").map(PathBuf::from);
    let application = router(state, static_dir);
    let bind = env::var("HELIOS_BIND").unwrap_or_else(|_| "127.0.0.1:8080".into());
    let listener = TcpListener::bind(&bind).await?;
    info!(bind = %bind, "Helios operator gateway listening");
    axum::serve(listener, application)
        .with_graceful_shutdown(shutdown())
        .await?;
    Ok(())
}

fn projection_poll_interval() -> Result<std::time::Duration, Box<dyn std::error::Error>> {
    let millis = env::var("HELIOS_TIME_SERIES_PROJECTION_POLL_MS")
        .unwrap_or_else(|_| "1000".into())
        .parse::<u64>()?;
    if !(100..=60_000).contains(&millis) {
        return Err("HELIOS_TIME_SERIES_PROJECTION_POLL_MS must be between 100 and 60000".into());
    }
    Ok(std::time::Duration::from_millis(millis))
}

async fn command_executor(
    account_id: &str,
    store: &Arc<OperatorStore>,
    time_series: &Arc<InMemoryTimeSeriesPort>,
) -> Arc<dyn CommandExecutor> {
    if env::var("HELIOS_ALPACA_PAPER_ENABLED").as_deref() != Ok("1") {
        warn!("Alpaca paper execution is disabled; commands remain read-only");
        return Arc::new(ReadOnlyCommandExecutor);
    }
    match build_alpaca_paper_executor(account_id, store.clone()).await {
        Ok(runtime) => {
            info!("Alpaca paper command executor admitted");
            tokio::spawn(run_alpaca_market_feed(
                runtime.market_credentials,
                runtime.stream_config,
                runtime.market,
                store.clone(),
                time_series.clone(),
            ));
            tokio::spawn(run_alpaca_trade_updates(
                runtime.trade_credentials,
                runtime.update_port,
                store.clone(),
            ));
            runtime.executor
        }
        Err(error) => {
            warn!(error = %error, "Alpaca paper executor configuration rejected; commands remain read-only");
            Arc::new(ReadOnlyCommandExecutor)
        }
    }
}

async fn build_alpaca_paper_executor(
    account_id: &str,
    store: Arc<OperatorStore>,
) -> Result<AlpacaPaperRuntime, Box<dyn std::error::Error>> {
    let key_id = env::var("APCA_API_KEY_ID")?;
    let secret_key = env::var("APCA_API_SECRET_KEY")?;
    let risk_policy: RiskPolicy = read_json_env("HELIOS_RISK_POLICY_PATH")?;
    let schedule: VenueSchedule = read_json_env("HELIOS_VENUE_SCHEDULE_PATH")?;
    schedule.validate()?;
    let symbols = env::var("HELIOS_ALPACA_SYMBOLS")?
        .split(',')
        .map(str::trim)
        .filter(|symbol| !symbol.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let feed = match env::var("HELIOS_ALPACA_FEED").as_deref() {
        Ok("sip") => AlpacaStockFeed::Sip,
        Ok("delayed_sip") => AlpacaStockFeed::DelayedSip,
        Ok("iex") | Err(_) => AlpacaStockFeed::Iex,
        Ok(_) => return Err("HELIOS_ALPACA_FEED must be iex, sip, or delayed_sip".into()),
    };
    let stream_config = MarketStreamConfig::stocks(feed, symbols)?;

    let mut config = AlpacaConfig::paper();
    config.venue = schedule.metadata.venue.clone();
    let transport =
        ReqwestAlpacaTransport::try_new(config.environment, std::time::Duration::from_secs(5))?;
    let broker = AlpacaBroker::try_new(
        config,
        AlpacaCredentials::try_new(key_id.clone(), secret_key.clone())?,
        transport,
    )?;
    let market = Arc::new(InMemoryMarketReferencePort::default());
    if env::var_os("HELIOS_MARKET_REFERENCE_PATH").is_some() {
        let references: Vec<MarketReference> = read_json_env("HELIOS_MARKET_REFERENCE_PATH")?;
        for reference in references {
            market.update(reference)?;
        }
    }
    let clock = Arc::new(SystemExecutionClock);
    let concrete = if env::var("HELIOS_GOLEM_ENABLED").as_deref() == Ok("1") {
        let now_ns = helio_operatord::ExecutionClock::now_ns(clock.as_ref())?;
        let now_sec = i64::try_from(now_ns / 1_000_000_000)?;
        let trading_day = schedule
            .session_on_or_after(now_sec)?
            .ok_or("venue schedule does not cover Golem initialization time")?
            .label
            .0;
        let portfolio = helio_execution::PortfolioRiskSnapshot::empty(now_ns, trading_day);
        let settings = golem_settings()?;
        let (oms, risk) =
            connect_golem_paper_ports(&settings, account_id, &risk_policy, &schedule, &portfolio)
                .await?;
        Arc::new(AlpacaPaperCommandExecutor::try_new_with_ports(
            account_id,
            broker,
            risk_policy,
            schedule,
            market.clone(),
            clock,
            PaperServicePorts::new(oms, risk),
        )?)
    } else {
        if env::var("HELIOS_ALLOW_IN_MEMORY_EXECUTION_STATE").as_deref() != Ok("1") {
            return Err(
                "paper execution requires HELIOS_GOLEM_ENABLED=1; process-local execution state is allowed only with HELIOS_ALLOW_IN_MEMORY_EXECUTION_STATE=1"
                    .into(),
            );
        }
        warn!("paper execution is using explicitly enabled process-local OMS and risk state");
        Arc::new(AlpacaPaperCommandExecutor::try_new(
            account_id,
            broker,
            risk_policy,
            schedule,
            market.clone(),
            clock,
        )?)
    };
    concrete.startup_reconcile(store).await?;
    let executor: Arc<dyn CommandExecutor> = concrete.clone();
    let update_port: Arc<dyn AlpacaTradeUpdatePort> = concrete;
    Ok(AlpacaPaperRuntime {
        executor,
        update_port,
        market_credentials: AlpacaCredentials::try_new(key_id.clone(), secret_key.clone())?,
        trade_credentials: AlpacaCredentials::try_new(key_id, secret_key)?,
        stream_config,
        market,
    })
}

fn golem_settings() -> Result<GolemClientSettings, Box<dyn std::error::Error>> {
    let mode = env::var("HELIOS_GOLEM_MODE").unwrap_or_else(|_| "local".into());
    let endpoint = match mode.as_str() {
        "local" => GolemEndpoint::Local,
        "cloud" => GolemEndpoint::Cloud {
            token: env::var("GOLEM_TOKEN")?,
        },
        "custom" => GolemEndpoint::Custom {
            url: env::var("HELIOS_GOLEM_URL")?,
            token: env::var("GOLEM_TOKEN")?,
        },
        _ => return Err("HELIOS_GOLEM_MODE must be local, cloud, or custom".into()),
    };
    Ok(GolemClientSettings {
        endpoint,
        app_name: env::var("HELIOS_GOLEM_APP").unwrap_or_else(|_| "helios-alpha".into()),
        environment_name: env::var("HELIOS_GOLEM_ENVIRONMENT").unwrap_or(mode),
    })
}

fn read_json_env<T: serde::de::DeserializeOwned>(
    name: &str,
) -> Result<T, Box<dyn std::error::Error>> {
    let path = env::var(name)?;
    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
}

fn command_auth() -> CommandAuth {
    match (
        env::var("HELIOS_OPERATOR_SESSION_TOKEN"),
        env::var("HELIOS_COMMAND_CSRF_SECRET"),
    ) {
        (Ok(token), Ok(secret)) => CommandAuth::enabled(
            env::var("HELIOS_OPERATOR_NAME").unwrap_or_else(|_| "operator".into()),
            &token,
            secret.as_bytes(),
            Duration::minutes(15),
        )
        .unwrap_or_else(|error| {
            warn!(error = %error, "Command authentication configuration rejected");
            CommandAuth::disabled()
        }),
        _ => {
            warn!("Command authentication is disabled; all command requests fail closed");
            CommandAuth::disabled()
        }
    }
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    if env::var("HELIOS_LOG_FORMAT").as_deref() == Ok("json") {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(filter)
            .json()
            .try_init();
    } else {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(filter)
            .compact()
            .try_init();
    }
}

async fn shutdown() {
    let control_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            signal.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = control_c => {},
        _ = terminate => {},
    }
}
