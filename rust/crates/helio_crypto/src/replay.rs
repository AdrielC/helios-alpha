use serde::{Deserialize, Serialize};

use crate::{mul_div, CryptoError, CryptoRiskLimits};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Candle {
    pub start_ms: u64,
    pub duration_ms: u64,
    pub open_micros: u64,
    pub high_micros: u64,
    pub low_micros: u64,
    pub close_micros: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub available_ms: u64,
    /// Target share of the initial cash allocation, 0..10,000. Never leverage or shorts.
    pub allocation_bps: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayInput {
    pub symbol: String,
    pub bars: Vec<Candle>,
    pub targets: Vec<Target>,
    pub limits: CryptoRiskLimits,
    pub initial_cash_micros: u64,
    pub fee_bps: u32,
    /// Per-side spread + slippage; an explicit scenario, not a broker fee estimate.
    pub friction_bps: u32,
    #[serde(default)]
    pub minimum_order_micros: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimulatedFill {
    pub at_ms: u64,
    pub signal_available_ms: u64,
    pub side: String,
    pub quantity_micros: u64,
    pub price_micros: u64,
    pub fee_micros: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EquityPoint {
    pub at_ms: u64,
    pub equity_micros: u64,
    pub cash_micros: u64,
    pub quantity_micros: u64,
    pub halted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayReport {
    pub schema_version: u32,
    pub simulation: bool,
    pub initial_cash_micros: u64,
    /// All remaining holdings are marked at conservative modeled liquidation value.
    pub final_equity_micros: u64,
    pub net_pnl_micros: i128,
    pub max_drawdown_micros: u64,
    pub fees_paid_micros: u64,
    pub turnover_micros: u64,
    pub fills: Vec<SimulatedFill>,
    pub equity: Vec<EquityPoint>,
}

fn checked_add(a: u64, b: u64) -> Result<u64, CryptoError> {
    a.checked_add(b).ok_or(CryptoError::Overflow)
}

fn liquidation(price: u64, quantity: u64, input: &ReplayInput) -> Result<u64, CryptoError> {
    let price = mul_div(price, 10_000 - u64::from(input.friction_bps), 10_000, false)?;
    let gross = mul_div(price, quantity, 1_000_000, false)?;
    let fee = mul_div(gross, u64::from(input.fee_bps), 10_000, true)?;
    Ok(gross.saturating_sub(fee))
}

pub fn replay(input: &ReplayInput) -> Result<ReplayReport, CryptoError> {
    input.limits.validate()?;
    if input.fee_bps > 1000 || input.friction_bps > 1000 {
        return Err(CryptoError::InvalidCosts);
    }
    if input.initial_cash_micros == 0
        || input.initial_cash_micros > input.limits.capital_micros
        || !input.limits.symbols.contains(&input.symbol)
        || input.minimum_order_micros > input.limits.order_micros
    {
        return Err(CryptoError::InvalidLimits);
    }
    if input.bars.is_empty() || input.bars.len() > 200_000 {
        return Err(CryptoError::InvalidCandles);
    }
    let mut previous_end = None;
    let duration = input.bars[0].duration_ms;
    for bar in &input.bars {
        if duration == 0
            || bar.duration_ms != duration
            || bar.low_micros == 0
            || bar.high_micros < bar.low_micros
            || !(bar.low_micros..=bar.high_micros).contains(&bar.open_micros)
            || !(bar.low_micros..=bar.high_micros).contains(&bar.close_micros)
            || previous_end.is_some_and(|end| end != bar.start_ms)
        {
            return Err(CryptoError::InvalidCandles);
        }
        previous_end = Some(checked_add(bar.start_ms, duration)?);
    }
    let mut previous_target = None;
    for target in &input.targets {
        if target.allocation_bps > 10_000
            || previous_target.is_some_and(|at| target.available_ms <= at)
        {
            return Err(CryptoError::InvalidTargets);
        }
        previous_target = Some(target.available_ms);
    }
    let mut report = ReplayReport {
        schema_version: 1,
        simulation: true,
        initial_cash_micros: input.initial_cash_micros,
        final_equity_micros: input.initial_cash_micros,
        net_pnl_micros: 0,
        max_drawdown_micros: 0,
        fees_paid_micros: 0,
        turnover_micros: 0,
        fills: vec![],
        equity: vec![],
    };
    let mut cash = input.initial_cash_micros;
    let mut quantity = 0;
    let mut target_index = 0;
    let mut target: Option<&Target> = None;
    let mut peak = cash;
    let mut day = input.bars[0].start_ms / 86_400_000;
    let mut day_open = cash;
    let mut daily_orders = 0;
    let mut day_halted = false;
    let mut drawdown_halted = false;
    for bar in &input.bars {
        // Strictly earlier availability. A signal at this open cannot fill at that same open.
        while target_index < input.targets.len()
            && input.targets[target_index].available_ms < bar.start_ms
        {
            target = Some(&input.targets[target_index]);
            target_index += 1;
        }
        let opening_equity = checked_add(cash, liquidation(bar.open_micros, quantity, input)?)?;
        peak = peak.max(opening_equity);
        let opening_drawdown = peak.saturating_sub(opening_equity);
        if bar.start_ms / 86_400_000 != day {
            day = bar.start_ms / 86_400_000;
            day_open = opening_equity;
            daily_orders = 0;
            day_halted = false;
        }
        day_halted |= day_open.saturating_sub(opening_equity) >= input.limits.daily_loss_micros;
        drawdown_halted |= peak.saturating_sub(opening_equity) >= input.limits.drawdown_micros;
        if let Some(signal) = target {
            let allocation = if day_halted || drawdown_halted {
                0
            } else {
                signal.allocation_bps
            };
            let desired_value = mul_div(
                input.initial_cash_micros,
                u64::from(allocation),
                10_000,
                false,
            )?
            .min(input.limits.symbol_exposure_micros);
            let held_value = mul_div(bar.open_micros, quantity, 1_000_000, false)?;
            if desired_value > held_value && daily_orders < input.limits.max_daily_orders {
                let price = mul_div(
                    bar.open_micros,
                    10_000 + u64::from(input.friction_bps),
                    10_000,
                    true,
                )?;
                let max_cash_notional =
                    mul_div(cash, 10_000, 10_000 + u64::from(input.fee_bps), false)?;
                let order_notional = mul_div(
                    input.limits.order_micros,
                    10_000,
                    10_000 + u64::from(input.fee_bps),
                    false,
                )?;
                let notional = (desired_value - held_value)
                    .min(order_notional)
                    .min(max_cash_notional);
                let units = mul_div(notional, 1_000_000, price, false)?;
                let gross = mul_div(price, units, 1_000_000, true)?;
                let fee = mul_div(gross, u64::from(input.fee_bps), 10_000, true)?;
                let debit = checked_add(gross, fee)?;
                if units > 0
                    && gross >= input.minimum_order_micros
                    && debit <= cash
                    && debit <= input.limits.order_micros
                {
                    cash -= debit;
                    quantity = checked_add(quantity, units)?;
                    record_fill(&mut report, bar, signal, "buy", units, price, fee, gross)?;
                    daily_orders += 1;
                }
            } else if held_value > desired_value {
                let notional = (held_value - desired_value).min(input.limits.order_micros);
                let units = mul_div(notional, 1_000_000, bar.open_micros, false)?.min(quantity);
                let price = mul_div(
                    bar.open_micros,
                    10_000 - u64::from(input.friction_bps),
                    10_000,
                    false,
                )?;
                let gross = mul_div(price, units, 1_000_000, false)?;
                let fee = mul_div(gross, u64::from(input.fee_bps), 10_000, true)?;
                if units > 0 && gross >= input.minimum_order_micros {
                    cash = checked_add(cash, gross.saturating_sub(fee))?;
                    quantity -= units;
                    record_fill(&mut report, bar, signal, "sell", units, price, fee, gross)?;
                    daily_orders += 1;
                }
            }
        }
        let equity = checked_add(cash, liquidation(bar.close_micros, quantity, input)?)?;
        peak = peak.max(equity);
        let drawdown = peak.saturating_sub(equity);
        report.max_drawdown_micros = report
            .max_drawdown_micros
            .max(drawdown)
            .max(opening_drawdown);
        day_halted |= day_open.saturating_sub(equity) >= input.limits.daily_loss_micros;
        drawdown_halted |= drawdown >= input.limits.drawdown_micros;
        report.equity.push(EquityPoint {
            at_ms: checked_add(bar.start_ms, duration)?,
            equity_micros: equity,
            cash_micros: cash,
            quantity_micros: quantity,
            halted: day_halted || drawdown_halted,
        });
        report.final_equity_micros = equity;
    }
    report.net_pnl_micros =
        i128::from(report.final_equity_micros) - i128::from(input.initial_cash_micros);
    Ok(report)
}

#[allow(clippy::too_many_arguments)]
fn record_fill(
    report: &mut ReplayReport,
    bar: &Candle,
    signal: &Target,
    side: &str,
    quantity: u64,
    price: u64,
    fee: u64,
    gross: u64,
) -> Result<(), CryptoError> {
    report.fees_paid_micros = checked_add(report.fees_paid_micros, fee)?;
    report.turnover_micros = checked_add(report.turnover_micros, gross)?;
    report.fills.push(SimulatedFill {
        at_ms: bar.start_ms,
        signal_available_ms: signal.available_ms,
        side: side.to_owned(),
        quantity_micros: quantity,
        price_micros: price,
        fee_micros: fee,
    });
    Ok(())
}
