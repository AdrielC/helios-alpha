//! Generic mechanics only. Strategy logic and deployment parameters belong outside Git.
//! Replay has no network, broker port, credentials, or authority to submit an order.

mod broker_journal;
mod control;
mod exact;
mod paper;
mod preparation;
mod replay;
mod risk;

pub use broker_journal::*;
pub use control::*;
pub use exact::{ceil_crypto_notional_micros, floor_crypto_notional_micros};
pub use paper::*;
pub use preparation::*;
pub use replay::*;
pub use risk::*;

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CryptoError {
    #[error("invalid crypto risk limits")]
    InvalidLimits,
    #[error("invalid or noncontiguous candles")]
    InvalidCandles,
    #[error("invalid or out-of-order targets")]
    InvalidTargets,
    #[error("invalid cost assumptions")]
    InvalidCosts,
    #[error("fixed-point arithmetic overflow")]
    Overflow,
}

pub(crate) fn mul_div(a: u64, b: u64, divisor: u64, ceil: bool) -> Result<u64, CryptoError> {
    if divisor == 0 {
        return Err(CryptoError::Overflow);
    }
    let value = u128::from(a) * u128::from(b);
    let value = if ceil {
        value.div_ceil(u128::from(divisor))
    } else {
        value / u128::from(divisor)
    };
    u64::try_from(value).map_err(|_| CryptoError::Overflow)
}
