//! Bounded exact decimal arithmetic. No binary floating point or intermediate rounded quantity.

use helio_execution::BrokerDecimal;

use crate::CryptoError;

pub(crate) const DECIMAL_SCALE: u128 = 1_000_000_000_000_000_000;

fn parts(value: &BrokerDecimal) -> Result<(u128, u128), CryptoError> {
    let (whole, fraction) = value
        .as_str()
        .split_once('.')
        .unwrap_or((value.as_str(), ""));
    if whole.len() > 18 || fraction.len() > 18 {
        return Err(CryptoError::Overflow);
    }
    let whole = whole.parse().map_err(|_| CryptoError::Overflow)?;
    let fraction = if fraction.is_empty() {
        0
    } else {
        fraction
            .parse::<u128>()
            .map_err(|_| CryptoError::Overflow)?
            * 10_u128.pow(18 - fraction.len() as u32)
    };
    Ok((whole, fraction))
}

pub(crate) fn decimal_units(value: &BrokerDecimal) -> Result<u128, CryptoError> {
    let (whole, fraction) = parts(value)?;
    Ok(whole * DECIMAL_SCALE + fraction)
}

pub(crate) fn decimal_from_units(value: u128) -> Result<BrokerDecimal, CryptoError> {
    BrokerDecimal::try_new(format!(
        "{}.{:018}",
        value / DECIMAL_SCALE,
        value % DECIMAL_SCALE
    ))
    .map_err(|_| CryptoError::Overflow)
}

/// Whole and fractional money micros before the final, explicit rounding operation.
fn notional(price: &BrokerDecimal, quantity: &BrokerDecimal) -> Result<(u128, bool), CryptoError> {
    let (pw, pf) = parts(price)?;
    let (qw, qf) = parts(quantity)?;
    // Multiplying two scaled-18 values directly overflows u128 even for some small
    // dollar orders. Split whole/fractional terms first; each cross product fits.
    const CROSS_DIVISOR: u128 = DECIMAL_SCALE / 1_000_000;
    const FRACTION_DIVISOR: u128 = DECIMAL_SCALE * CROSS_DIVISOR;
    let whole = pw
        .checked_mul(qw)
        .and_then(|v| v.checked_mul(1_000_000))
        .ok_or(CryptoError::Overflow)?;
    let cross = pw * qf + qw * pf;
    let fractional = pf * qf;
    let remainder = (cross % CROSS_DIVISOR) * DECIMAL_SCALE + fractional % FRACTION_DIVISOR;
    let micros = whole
        .checked_add(cross / CROSS_DIVISOR)
        .and_then(|v| v.checked_add(fractional / FRACTION_DIVISOR))
        .and_then(|v| v.checked_add(remainder / FRACTION_DIVISOR))
        .ok_or(CryptoError::Overflow)?;
    Ok((micros, !remainder.is_multiple_of(FRACTION_DIVISOR)))
}

/// Upper bound in USD micros, including a final fractional micro. Use for spending/exposure.
pub fn ceil_crypto_notional_micros(
    price: &BrokerDecimal,
    quantity: &BrokerDecimal,
) -> Result<u64, CryptoError> {
    let (micros, remainder) = notional(price, quantity)?;
    u64::try_from(
        micros
            .checked_add(u128::from(remainder))
            .ok_or(CryptoError::Overflow)?,
    )
    .map_err(|_| CryptoError::Overflow)
}

/// Lower bound in USD micros. Use when checking a broker's minimum order amount.
pub fn floor_crypto_notional_micros(
    price: &BrokerDecimal,
    quantity: &BrokerDecimal,
) -> Result<u64, CryptoError> {
    u64::try_from(notional(price, quantity)?.0).map_err(|_| CryptoError::Overflow)
}
