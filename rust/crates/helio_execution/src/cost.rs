use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{checked_notional, ArithmeticError, MoneyMicros, PriceMicros, QuantityMicros};

const PARTS_PER_MILLION: u128 = 1_000_000;
const MICROS_PER_BASIS_POINT: u128 = 1_000_000;
const BASIS_POINTS_PER_UNIT: u128 = 10_000;

/// Cost rate in millionths of one basis point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MicroBasisPoints(pub u64);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostModel {
    pub version: String,
    pub half_spread: MicroBasisPoints,
    pub fees: MicroBasisPoints,
    pub latency_slippage: MicroBasisPoints,
    /// Square-root impact at 100 percent participation.
    pub impact_at_full_participation: MicroBasisPoints,
    pub max_participation_ppm: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostEstimate {
    pub model_version: String,
    pub notional: MoneyMicros,
    pub participation_ppm: u32,
    pub spread_cost: MoneyMicros,
    pub fee_cost: MoneyMicros,
    pub latency_cost: MoneyMicros,
    pub market_impact_cost: MoneyMicros,
    pub total_cost: MoneyMicros,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CostModelError {
    #[error("average daily quantity must be nonzero")]
    MissingDailyVolume,
    #[error("order participation {actual_ppm} ppm exceeds limit {limit_ppm} ppm")]
    CapacityExceeded { actual_ppm: u32, limit_ppm: u32 },
    #[error("fixed-point arithmetic overflowed")]
    Overflow,
    #[error(transparent)]
    InvalidOrder(#[from] ArithmeticError),
}

impl CostModel {
    pub fn estimate(
        &self,
        price: PriceMicros,
        quantity: QuantityMicros,
        average_daily_quantity: QuantityMicros,
    ) -> Result<CostEstimate, CostModelError> {
        if average_daily_quantity.0 == 0 {
            return Err(CostModelError::MissingDailyVolume);
        }
        let notional = checked_notional(price, quantity)?;
        let participation_raw = u128::from(quantity.0)
            .checked_mul(PARTS_PER_MILLION)
            .ok_or(CostModelError::Overflow)?
            .checked_add(u128::from(average_daily_quantity.0) - 1)
            .ok_or(CostModelError::Overflow)?
            / u128::from(average_daily_quantity.0);
        let participation_ppm =
            u32::try_from(participation_raw).map_err(|_| CostModelError::CapacityExceeded {
                actual_ppm: u32::MAX,
                limit_ppm: self.max_participation_ppm,
            })?;
        if participation_ppm > self.max_participation_ppm {
            return Err(CostModelError::CapacityExceeded {
                actual_ppm: participation_ppm,
                limit_ppm: self.max_participation_ppm,
            });
        }

        // sqrt(ppm / 1e6) = sqrt(ppm) / 1000. Keep six extra decimal places.
        let impact_scale_millionths = integer_sqrt_ceil(
            u128::from(participation_ppm)
                .checked_mul(PARTS_PER_MILLION)
                .ok_or(CostModelError::Overflow)?,
        );
        let impact_rate = u128::from(self.impact_at_full_participation.0)
            .checked_mul(impact_scale_millionths)
            .ok_or(CostModelError::Overflow)?
            / PARTS_PER_MILLION;

        let spread_cost = cost_for_rate(notional, u128::from(self.half_spread.0))?;
        let fee_cost = cost_for_rate(notional, u128::from(self.fees.0))?;
        let latency_cost = cost_for_rate(notional, u128::from(self.latency_slippage.0))?;
        let market_impact_cost = cost_for_rate(notional, impact_rate)?;
        let total_cost = [spread_cost, fee_cost, latency_cost, market_impact_cost]
            .into_iter()
            .try_fold(0_u64, |sum, component| sum.checked_add(component.0))
            .map(MoneyMicros)
            .ok_or(CostModelError::Overflow)?;

        Ok(CostEstimate {
            model_version: self.version.clone(),
            notional,
            participation_ppm,
            spread_cost,
            fee_cost,
            latency_cost,
            market_impact_cost,
            total_cost,
        })
    }
}

fn cost_for_rate(
    notional: MoneyMicros,
    rate_micro_bps: u128,
) -> Result<MoneyMicros, CostModelError> {
    let denominator = BASIS_POINTS_PER_UNIT * MICROS_PER_BASIS_POINT;
    let numerator = u128::from(notional.0)
        .checked_mul(rate_micro_bps)
        .ok_or(CostModelError::Overflow)?;
    let rounded = numerator
        .checked_add(denominator - 1)
        .ok_or(CostModelError::Overflow)?
        / denominator;
    u64::try_from(rounded)
        .map(MoneyMicros)
        .map_err(|_| CostModelError::Overflow)
}

fn integer_sqrt(value: u128) -> u128 {
    if value < 2 {
        return value;
    }
    let mut low = 1;
    let mut high = value / 2 + 1;
    while low <= high {
        let middle = low + (high - low) / 2;
        if middle <= value / middle {
            low = middle + 1;
        } else {
            high = middle - 1;
        }
    }
    high
}

fn integer_sqrt_ceil(value: u128) -> u128 {
    let floor = integer_sqrt(value);
    if floor * floor == value {
        floor
    } else {
        floor + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(max_participation_ppm: u32) -> CostModel {
        CostModel {
            version: "tca-1".into(),
            half_spread: MicroBasisPoints(2_000_000),
            fees: MicroBasisPoints(500_000),
            latency_slippage: MicroBasisPoints(1_000_000),
            impact_at_full_participation: MicroBasisPoints(100_000_000),
            max_participation_ppm,
        }
    }

    #[test]
    fn impact_and_total_cost_are_monotonic_with_size() {
        let model = model(200_000);
        let small = model
            .estimate(
                PriceMicros(100_000_000),
                QuantityMicros(1_000_000),
                QuantityMicros(100_000_000),
            )
            .unwrap();
        let large = model
            .estimate(
                PriceMicros(100_000_000),
                QuantityMicros(4_000_000),
                QuantityMicros(100_000_000),
            )
            .unwrap();
        assert!(large.market_impact_cost > small.market_impact_cost);
        assert!(large.total_cost > small.total_cost);
    }

    #[test]
    fn capacity_is_a_hard_limit() {
        assert_eq!(
            model(10_000).estimate(
                PriceMicros(100_000_000),
                QuantityMicros(2_000_000),
                QuantityMicros(100_000_000),
            ),
            Err(CostModelError::CapacityExceeded {
                actual_ppm: 20_000,
                limit_ppm: 10_000,
            })
        );
    }

    #[test]
    fn all_components_round_up_without_floating_point() {
        let estimate = model(1_000_000)
            .estimate(PriceMicros(1), QuantityMicros(1), QuantityMicros(1))
            .unwrap();
        assert!(estimate.spread_cost.0 >= 1);
        assert!(estimate.total_cost.0 >= 4);
    }

    #[test]
    fn integer_square_root_is_floor_exact() {
        assert_eq!(integer_sqrt(0), 0);
        assert_eq!(integer_sqrt(15), 3);
        assert_eq!(integer_sqrt(16), 4);
        assert_eq!(integer_sqrt(u128::MAX), u64::MAX as u128);
        assert_eq!(integer_sqrt_ceil(15), 4);
        assert_eq!(integer_sqrt_ceil(16), 4);
    }
}
