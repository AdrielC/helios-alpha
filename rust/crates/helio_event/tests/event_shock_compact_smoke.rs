//! Compact CSV → `EventShock` → vertical pipeline (deterministic smoke).

use helio_event::*;
use helio_scan::{FlushReason, FlushableScan, Scan, VecEmitter};
use helio_time::SimpleWeekdayCalendar;

const COMPACT_CSV: &str = include_str!("../../../fixtures/event_shock/compact_events.csv");
const BARS_CSV: &str = include_str!("../../../fixtures/event_shock/bars.csv");

#[test]
fn compact_csv_loads_and_produces_treatment_trades() {
    let shocks = load_compact_event_shocks_csv(COMPACT_CSV).expect("compact csv");
    assert_eq!(shocks.len(), 2);
    assert!(matches!(shocks[0].scope, EventScope::Global));

    let bars = load_daily_bars_csv(BARS_CSV).expect("bars");
    let cand = candidate_entries_from_bars(&bars);
    let replay = build_vertical_replay(shocks, bars);

    let cal = SimpleWeekdayCalendar;
    let vertical = EventShockVerticalScan::new(
        None,
        EventShockFilterConfig::default(),
        cal,
        ExitPolicy::FixedHorizonSessions { n: 5 },
        Exposure::Pair {
            long: Symbol("XLU".into()),
            short: Symbol("SPY".into()),
        },
        EventShockControlConfig {
            seed: 42,
            controls_per_treatment: 1,
            strategy_name: "smoke".into(),
            horizon_sessions: 5,
            exposure: Exposure::Pair {
                long: Symbol("XLU".into()),
                short: Symbol("SPY".into()),
            },
            vol_epsilon: None,
        },
        cand,
        ExecutionEntryTiming::EntrySessionOpen,
        "smoke",
    );

    let mut st = vertical.init();
    let mut e = VecEmitter::new();
    for r in &replay {
        vertical.step(&mut st, r.clone(), &mut e);
    }
    vertical.flush(&mut st, FlushReason::EndOfInput, &mut e);
    let trades = e.into_inner();
    let n_treat = trades
        .iter()
        .filter(|t| t.matched_treatment.is_none())
        .count();
    assert_eq!(n_treat, 2, "expected one treatment trade per shock");
    assert!(
        trades.iter().any(|t| t.matched_treatment.is_some()),
        "expected matched control rows"
    );
}
