use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Decimal money represented in millionths of the settlement currency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MoneyMicros(pub u64);

/// Decimal price represented in millionths of the settlement currency per unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PriceMicros(pub u64);

/// Decimal quantity represented in millionths of one unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct QuantityMicros(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Side {
    Buy,
    Sell,
}

impl Side {
    pub const fn signed_quantity(self, quantity: QuantityMicros) -> i128 {
        match self {
            Self::Buy => quantity.0 as i128,
            Self::Sell => -(quantity.0 as i128),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionMode {
    Paper,
    Live,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderProposal {
    pub proposal_id: String,
    pub strategy_id: String,
    pub symbol: String,
    pub venue: String,
    pub currency: String,
    pub side: Side,
    pub quantity: QuantityMicros,
    pub limit_price: PriceMicros,
    pub mode: ExecutionMode,
    pub trading_day: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderIntent {
    pub client_order_id: String,
    pub proposal: OrderProposal,
    pub authorized_notional: MoneyMicros,
    pub risk_policy_version: String,
    pub authorized_at_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ArithmeticError {
    #[error("price and quantity must both be nonzero")]
    ZeroOrderValue,
    #[error("fixed-point arithmetic overflowed")]
    Overflow,
}

/// Conservative notional calculation. Fractional money micros are rounded upward.
pub fn checked_notional(
    price: PriceMicros,
    quantity: QuantityMicros,
) -> Result<MoneyMicros, ArithmeticError> {
    if price.0 == 0 || quantity.0 == 0 {
        return Err(ArithmeticError::ZeroOrderValue);
    }
    let product = u128::from(price.0)
        .checked_mul(u128::from(quantity.0))
        .ok_or(ArithmeticError::Overflow)?;
    let rounded = product
        .checked_add(999_999)
        .ok_or(ArithmeticError::Overflow)?
        / 1_000_000;
    u64::try_from(rounded)
        .map(MoneyMicros)
        .map_err(|_| ArithmeticError::Overflow)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notional_is_exact_or_rounds_against_understatement() {
        assert_eq!(
            checked_notional(PriceMicros(12_500_000), QuantityMicros(2_000_000)),
            Ok(MoneyMicros(25_000_000))
        );
        assert_eq!(
            checked_notional(PriceMicros(1), QuantityMicros(1)),
            Ok(MoneyMicros(1))
        );
    }

    #[test]
    fn notional_overflow_is_explicit() {
        assert_eq!(
            checked_notional(PriceMicros(u64::MAX), QuantityMicros(u64::MAX)),
            Err(ArithmeticError::Overflow)
        );
    }
}
