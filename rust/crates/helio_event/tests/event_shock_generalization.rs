//! Second dataset (weather), incremental vs batch replay, lead-time summary.

use helio_event::*;
use helio_scan::SessionDate;
use helio_time::SimpleWeekdayCalendar;

const WEATHER_CSV: &str = include_str!("../../../fixtures/event_shock/weather_events.csv");
const BARS_CSV: &str = include_str!("../../../fixtures/event_shock/bars.csv");

fn vertical_for_strategy(
    preset: EventShockStrategyPreset,
    filter: EventShockFilterConfig,
) -> EventShockVerticalScan<SimpleWeekdayCalendar> {
    let bars = load_daily_bars_csv(BARS_CSV).expect("bars");
    let cand = candidate_entries_from_bars(&bars);
    EventShockVerticalScan::new(
        None,
        filter,
        SimpleWeekdayCalendar,
        preset.exit_policy(),
        preset.treatment_exposure(),
        EventShockControlConfig {
            seed: 77,
            controls_per_treatment: 1,
            horizon_sessions: preset.control_horizon_sessions(),
            exposure: preset.control_exposure_clone(),
            vol_epsilon: None,
        },
        cand,
    )
}

#[test]
fn weather_adapter_maps_to_event_shock() {
    let shocks = load_weather_event_shocks_csv(WEATHER_CSV).expect("parse");
    assert_eq!(shocks.len(), 2);
    assert!(matches!(shocks[0].kind, EventKind::Weather));
    assert!(matches!(shocks[0].scope, EventScope::Region(12)));
    assert!(matches!(shocks[1].scope, EventScope::Global));
}

#[test]
fn defense_spy_strategy_runs_on_weather_events() {
    let shocks = load_weather_event_shocks_csv(WEATHER_CSV).expect("weather");
    let bars = load_daily_bars_csv(BARS_CSV).expect("bars");
    let replay = build_vertical_replay(shocks, bars);
    let vertical = vertical_for_strategy(
        EventShockStrategyPreset::DefenseSpyPairMidWindow,
        EventShockFilterConfig::default(),
    );
    let trades = collect_vertical_trades_incremental(&vertical, &replay);
    let n_treat = trades
        .iter()
        .filter(|t| t.matched_treatment.is_none())
        .count();
    assert_eq!(n_treat, 2);
}

#[test]
fn incremental_batch_and_checkpoint_match() {
    let shocks = load_solar_event_shocks_csv(include_str!(
        "../../../fixtures/event_shock/solar_events.csv"
    ))
    .expect("solar");
    let bars = load_daily_bars_csv(BARS_CSV).expect("bars");
    let replay = build_vertical_replay(shocks, bars);
    let vertical = vertical_for_strategy(
        EventShockStrategyPreset::XluSpyPairFiveSession,
        EventShockFilterConfig::default(),
    );
    let a = collect_vertical_trades_batch(&vertical, &replay);
    let b = collect_vertical_trades_incremental(&vertical, &replay);
    let c = collect_vertical_trades_with_checkpoint_resume(&vertical, &replay, replay.len() / 2);
    assert_eq!(a, b);
    assert_eq!(a, c);
}

#[test]
fn lead_time_summary_counts_tradable_band() {
    let shocks = load_weather_event_shocks_csv(WEATHER_CSV).expect("w");
    let r = summarize_lead_times(&shocks, 100_000, 10_000_000);
    assert_eq!(r.n_events, 2);
    assert!(r.n_tradable_under_band >= 1);
}

#[test]
fn second_strategy_changes_exit_sessions_vs_default() {
    let shocks = load_solar_event_shocks_csv(include_str!(
        "../../../fixtures/event_shock/solar_events.csv"
    ))
    .expect("solar");
    let bars = load_daily_bars_csv(BARS_CSV).expect("bars");
    let replay = build_vertical_replay(shocks, bars);
    let v1 = vertical_for_strategy(
        EventShockStrategyPreset::XluSpyPairFiveSession,
        EventShockFilterConfig::default(),
    );
    let v2 = vertical_for_strategy(
        EventShockStrategyPreset::DefenseSpyPairMidWindow,
        EventShockFilterConfig::default(),
    );
    let t1: Vec<SessionDate> = collect_vertical_trades_incremental(&v1, &replay)
        .into_iter()
        .filter(|t| t.matched_treatment.is_none())
        .map(|t| t.exit_session)
        .collect();
    let t2: Vec<SessionDate> = collect_vertical_trades_incremental(&v2, &replay)
        .into_iter()
        .filter(|t| t.matched_treatment.is_none())
        .map(|t| t.exit_session)
        .collect();
    assert_ne!(t1, t2, "mid-window exit should differ from fixed horizon");
}
