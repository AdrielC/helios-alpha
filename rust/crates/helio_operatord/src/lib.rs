pub mod alpaca_feed;
pub mod alpaca_paper;
pub mod alpaca_trade_stream;
pub mod auth;
pub mod fixtures;
pub mod forecast;
#[cfg(feature = "golem")]
pub mod golem_ports;
pub mod paper_ports;
pub mod projection_file;
pub mod server;
pub mod store;
pub mod time_series;
pub mod types;

pub use alpaca_feed::run_alpaca_market_feed;
pub use alpaca_paper::{
    AlpacaPaperCommandExecutor, AlpacaTradeUpdatePort, ExecutionClock, InMemoryMarketReferencePort,
    MarketReference, MarketReferencePort, PaperExecutorError, PaperServicePorts,
    SystemExecutionClock,
};
pub use alpaca_trade_stream::run_alpaca_trade_updates;
pub use auth::CommandAuth;
#[cfg(feature = "golem")]
pub use golem_ports::{
    connect_golem_paper_ports, GolemClientSettings, GolemEndpoint, GolemPortError,
};
pub use paper_ports::{LocalRiskPort, PaperOmsPort, PaperRiskPort, RiskPortStatus};
pub use projection_file::{load_projection_file, watch_projection_file, ProjectionFileError};
pub use server::{router, AppState};
pub use store::{CommandExecutor, OperatorStore, ReadOnlyCommandExecutor};
pub use time_series::{InMemoryTimeSeriesPort, TimeSeriesPort};
