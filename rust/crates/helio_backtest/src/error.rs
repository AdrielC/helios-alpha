use thiserror::Error;

use helio_stats::NumericalError;

#[derive(Debug, Error)]
pub enum BacktestError {
    #[error("epoch range invalid: start {0} must be <= end {1}")]
    InvalidEpochRange(i64, i64),
    #[error("backtest numerical reduction failed: {0}")]
    Numerical(#[from] NumericalError),
    #[error("Kalman likelihood could not be represented")]
    InvalidKalmanLikelihood,
}

pub type Result<T> = std::result::Result<T, BacktestError>;
