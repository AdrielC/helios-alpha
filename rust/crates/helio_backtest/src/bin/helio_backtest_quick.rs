//! One-shot demo run (stdout): bars, cumulative toy return, annualized daily Sharpe, wall seconds.

fn main() {
    let spec = helio_backtest::demo_run_spec();
    let r = helio_backtest::BacktestHarness::fixed(0)
        .run(&spec)
        .expect("demo run");
    println!(
        "bars={} pnl_sum={:.8} sharpe_daily_ann={:?} run_wall_secs={:.6}",
        r.bars_processed, r.pnl_simple, r.sharpe_daily_annualized, r.run_wall_secs
    );

    // ~10y toy range for a rough throughput check (same engine as unit test).
    let mut big = spec.clone();
    big.range = helio_backtest::EpochRange::new(0, 10 * 365 * 86_400).expect("range");
    let r2 = helio_backtest::BacktestHarness::fixed(0)
        .run(&big)
        .expect("10y");
    println!(
        "10y_toy bars={} sharpe_daily_ann={:?} run_wall_secs={:.6}",
        r2.bars_processed, r2.sharpe_daily_annualized, r2.run_wall_secs
    );
}
