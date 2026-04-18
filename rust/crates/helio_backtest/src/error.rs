use thiserror::Error;

#[derive(Debug, Error)]
pub enum BacktestError {
    #[error("epoch range invalid: start {0} must be <= end {1}")]
    InvalidEpochRange(i64, i64),
}

pub type Result<T> = std::result::Result<T, BacktestError>;
