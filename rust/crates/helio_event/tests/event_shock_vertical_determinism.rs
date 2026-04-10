//! Determinism and causality guarantees for the event-shock vertical.

use helio_event::*;
use helio_scan::{
    CheckpointKeyFn, FlushReason, FlushableScan, HashMapStore, Persisted, Runner, Scan,
    SessionDate, SnapshottingScan, VecEmitter,
};
use helio_window::{SessionSample, SessionWindowScan};
use helio_time::{AvailableAt, SimpleWeekdayCalendar};

fn day(d: i32) -> i64 {
    (d as i64) * 86_400
}

fn sample_shocks() -> Vec<EventShock> {
    vec![
        EventShock {
            id: EventId(1),
            kind: EventKind::Other,
            tags: String::new(),
            observed_at: None,
            available_at: AvailableAt(day(15) + 100),
            impact_start: day(22),
            impact_end: day(28),
            severity: 1.0,
            confidence: 1.0,
            scope: EventScope::Global,
        },
        EventShock {
            id: EventId(2),
            kind: EventKind::Other,
            tags: String::new(),
            observed_at: None,
            available_at: AvailableAt(day(40) + 100),
            impact_start: day(45),
            impact_end: day(50),
            severity: 1.0,
            confidence: 1.0,
            scope: EventScope::Global,
        },
    ]
}

fn sample_bars() -> Vec<DailyBar> {
    let mut v = Vec::new();
    for d in 10..80i32 {
        for (sym, base) in [("XLU", 50.0f64), ("SPY", 400.0f64)] {
            let t = d as f64;
            v.push(DailyBar {
                session: SessionDate(d),
                symbol: Symbol(sym.into()),
                open: base + t * 0.01,
                high: base + t * 0.02,
                low: base,
                close: base + t * 0.015,
            });
        }
    }
    v
}

fn make_vertical() -> EventShockVerticalScan<SimpleWeekdayCalendar> {
    let cal = SimpleWeekdayCalendar;
    let bars = sample_bars();
    let cand = candidate_entries_from_bars(&bars);
    EventShockVerticalScan::new(
        None,
        EventShockFilterConfig::default(),
        cal,
        ExitPolicy::FixedHorizonSessions { n: 3 },
        Exposure::Pair {
            long: Symbol("XLU".into()),
            short: Symbol("SPY".into()),
        },
        EventShockControlConfig {
            seed: 99,
            controls_per_treatment: 1,
            strategy_name: "test-vertical".into(),
            horizon_sessions: 3,
            exposure: Exposure::Pair {
                long: Symbol("XLU".into()),
                short: Symbol("SPY".into()),
            },
            vol_epsilon: None,
        },
        cand,
        ExecutionEntryTiming::EntrySessionOpen,
        "test-vertical",
    )
}

fn collect_trades(
    pipe: &EventShockVerticalScan<SimpleWeekdayCalendar>,
    recs: &[EventShockVerticalRecord],
) -> Vec<TradeResult> {
    let mut st = pipe.init();
    let mut e = VecEmitter::new();
    for r in recs {
        pipe.step(&mut st, r.clone(), &mut e);
    }
    pipe.flush(&mut st, FlushReason::EndOfInput, &mut e);
    e.into_inner()
}

#[test]
fn slice_replay_matches_iterator_replay() {
    let pipe = make_vertical();
    let replay = build_vertical_replay(sample_shocks(), sample_bars());

    let mut st1 = pipe.init();
    let mut e1 = VecEmitter::new();
    helio_scan::run_slice(&pipe, &mut st1, &replay, &mut e1);
    pipe.flush(&mut st1, FlushReason::EndOfInput, &mut e1);
    let a = e1.into_inner();

    let mut st2 = pipe.init();
    let mut e2 = VecEmitter::new();
    for r in replay.iter().cloned() {
        pipe.step(&mut st2, r, &mut e2);
    }
    pipe.flush(&mut st2, FlushReason::EndOfInput, &mut e2);
    let b = e2.into_inner();

    assert_eq!(a, b);
}

#[test]
fn checkpoint_resume_matches_uninterrupted() {
    let pipe = make_vertical();
    let replay = build_vertical_replay(sample_shocks(), sample_bars());
    let full = collect_trades(&pipe, &replay);

    let split = replay.len() / 2;
    let mut e_first = VecEmitter::new();
    let mut st = pipe.init();
    for r in replay.iter().take(split) {
        pipe.step(&mut st, r.clone(), &mut e_first);
    }
    let snap = pipe.snapshot(&st);
    let mut e_rest = VecEmitter::new();
    let mut st2 = pipe.restore(snap);
    for r in replay.iter().skip(split) {
        pipe.step(&mut st2, r.clone(), &mut e_rest);
    }
    pipe.flush(&mut st2, FlushReason::EndOfInput, &mut e_rest);
    let mut combined = e_first.into_inner();
    combined.extend(e_rest.into_inner());
    assert_eq!(full, combined);
}

#[test]
fn receiver_runner_matches_iterator_for_vertical() {
    let pipe = make_vertical();
    let replay = build_vertical_replay(sample_shocks(), sample_bars());
    let a = collect_vertical_trades_incremental(&pipe, &replay);
    let b = collect_vertical_trades_receiver(&pipe, &replay);
    assert_eq!(a, b);
}

#[test]
fn checkpoint_cadence_matches_uninterrupted() {
    let pipe = make_vertical();
    let replay = build_vertical_replay(sample_shocks(), sample_bars());
    let full = collect_trades(&pipe, &replay);
    for every in [1usize, 2, 7, 64, 256, 1024] {
        let c = collect_vertical_trades_with_checkpoint_cadence(&pipe, &replay, every);
        assert_eq!(
            full, c,
            "checkpoint every {every} should match uninterrupted"
        );
    }
}

#[test]
fn session_window_flush_emits_once_without_double_emit() {
    let scan = SessionWindowScan::<i32>::new();
    let mut st = scan.init();
    let mut e = VecEmitter::new();
    scan.step(
        &mut st,
        SessionSample {
            session: SessionDate(1),
            value: 10,
        },
        &mut e,
    );
    assert!(e.0.is_empty());
    scan.flush(&mut st, FlushReason::SessionClose(SessionDate(1)), &mut e);
    assert_eq!(e.0, vec![vec![10]]);
    scan.flush(&mut st, FlushReason::SessionClose(SessionDate(1)), &mut e);
    assert_eq!(e.0.len(), 1);
}

#[test]
fn persisted_checkpoint_matches_uninterrupted() {
    #[derive(Clone)]
    struct Key;
    impl CheckpointKeyFn<u64> for Key {
        type Key = &'static str;
        fn key_for_offset(&self, _offset: &u64) -> Self::Key {
            "cp"
        }
    }

    let pipe = make_vertical();
    let replay = build_vertical_replay(sample_shocks(), sample_bars());
    let full = collect_trades(&pipe, &replay);

    let inner = make_vertical();
    let persisted = Persisted::new(inner, HashMapStore::default(), Key);
    let mut r = Runner::new(persisted);
    let mut e = VecEmitter::new();
    for (i, rec) in replay.iter().enumerate() {
        r.step(rec.clone(), &mut e);
        if i + 1 == replay.len() / 2 {
            r.flush(FlushReason::Checkpoint(3u64), &mut e);
        }
    }
    r.flush(FlushReason::EndOfInput, &mut e);
    assert_eq!(full, e.into_inner());
}

#[test]
fn future_available_events_emit_no_trades_under_gate() {
    let cal = SimpleWeekdayCalendar;
    let bars = sample_bars();
    let cand = candidate_entries_from_bars(&bars);
    let vertical = EventShockVerticalScan::new(
        Some(AvailableAt(day(12))),
        EventShockFilterConfig::default(),
        cal,
        ExitPolicy::FixedHorizonSessions { n: 2 },
        Exposure::Long(Symbol("SPY".into())),
        EventShockControlConfig {
            seed: 1,
            controls_per_treatment: 0,
            strategy_name: "gate-test".into(),
            horizon_sessions: 2,
            exposure: Exposure::Long(Symbol("SPY".into())),
            vol_epsilon: None,
        },
        cand,
        ExecutionEntryTiming::EntrySessionOpen,
        "gate-test",
    );
    let shocks = vec![EventShock {
        id: EventId(100),
        kind: EventKind::Other,
        tags: "macro".into(),
        observed_at: None,
        available_at: AvailableAt(day(20)),
        impact_start: day(25),
        impact_end: day(30),
        severity: 1.0,
        confidence: 1.0,
        scope: EventScope::Global,
    }];
    let replay = build_vertical_replay(shocks, bars);
    let trades = collect_trades(&vertical, &replay);
    assert!(
        trades.is_empty(),
        "as_of before available_at must not emit trades"
    );
}

#[test]
fn shock_stream_order_is_preserved_in_outputs() {
    let cal = SimpleWeekdayCalendar;
    let bars = sample_bars();
    let cand = candidate_entries_from_bars(&bars);
    let mk = |seed: u64| {
        EventShockVerticalScan::new(
            None,
            EventShockFilterConfig::default(),
            cal,
            ExitPolicy::FixedHorizonSessions { n: 2 },
            Exposure::Long(Symbol("SPY".into())),
            EventShockControlConfig {
                seed,
                controls_per_treatment: 0,
                strategy_name: "order-test".into(),
                horizon_sessions: 2,
                exposure: Exposure::Long(Symbol("SPY".into())),
                vol_epsilon: None,
            },
            cand.clone(),
            ExecutionEntryTiming::EntrySessionOpen,
            "order-test",
        )
    };
    let s1 = EventShock {
        id: EventId(10),
        kind: EventKind::Other,
        tags: String::new(),
        observed_at: None,
        available_at: AvailableAt(day(18) + 10),
        impact_start: day(24),
        impact_end: day(26),
        severity: 1.0,
        confidence: 1.0,
        scope: EventScope::Global,
    };
    let s2 = EventShock {
        id: EventId(11),
        kind: EventKind::Other,
        tags: String::new(),
        observed_at: None,
        available_at: AvailableAt(day(18) + 20),
        impact_start: day(27),
        impact_end: day(29),
        severity: 1.0,
        confidence: 1.0,
        scope: EventScope::Global,
    };
    let shocks_ab = vec![s1.clone(), s2.clone()];
    let shocks_ba = vec![s2, s1];
    let r_ab = build_vertical_replay(shocks_ab, bars.clone());
    let r_ba = build_vertical_replay(shocks_ba, bars);
    let t_ab: Vec<EventId> = collect_trades(&mk(1), &r_ab)
        .into_iter()
        .filter(|t| t.matched_treatment.is_none())
        .map(|t| t.event_id)
        .collect();
    let t_ba: Vec<EventId> = collect_trades(&mk(1), &r_ba)
        .into_iter()
        .filter(|t| t.matched_treatment.is_none())
        .map(|t| t.event_id)
        .collect();
    // Same merge bucket: stream order follows shock ingest order (seq), not id sort.
    assert_eq!(t_ab, vec![EventId(10), EventId(11)]);
    assert_eq!(t_ba, vec![EventId(11), EventId(10)]);
}

#[test]
fn matched_control_sampling_deterministic_under_seed() {
    let cal = SimpleWeekdayCalendar;
    let bars = sample_bars();
    let cand = candidate_entries_from_bars(&bars);
    let shocks = vec![sample_shocks()[0].clone()];
    let replay = build_vertical_replay(shocks, bars);

    let mk = |seed: u64| {
        EventShockVerticalScan::new(
            None,
            EventShockFilterConfig::default(),
            cal,
            ExitPolicy::FixedHorizonSessions { n: 3 },
            Exposure::Pair {
                long: Symbol("XLU".into()),
                short: Symbol("SPY".into()),
            },
            EventShockControlConfig {
                seed,
                controls_per_treatment: 2,
                strategy_name: "ctrl-seed".into(),
                horizon_sessions: 3,
                exposure: Exposure::Pair {
                    long: Symbol("XLU".into()),
                    short: Symbol("SPY".into()),
                },
                vol_epsilon: None,
            },
            cand.clone(),
            ExecutionEntryTiming::EntrySessionOpen,
            "ctrl-seed",
        )
    };

    let a = collect_trades(&mk(12345), &replay);
    let b = collect_trades(&mk(12345), &replay);
    assert_eq!(a, b);

    let c = collect_trades(&mk(7), &replay);
    let ctrl_a: Vec<_> = a.iter().filter(|t| t.matched_treatment.is_some()).collect();
    let ctrl_c: Vec<_> = c.iter().filter(|t| t.matched_treatment.is_some()).collect();
    assert_ne!(
        ctrl_a.first().map(|t| t.entry_session),
        ctrl_c.first().map(|t| t.entry_session),
        "different seeds should pick different control entries (fixture expectation)"
    );
}

#[test]
fn next_session_open_execution_changes_returns_vs_entry_open() {
    let cal = SimpleWeekdayCalendar;
    let bars = sample_bars();
    let cand = candidate_entries_from_bars(&bars);
    let mk = |timing: ExecutionEntryTiming| {
        EventShockVerticalScan::new(
            None,
            EventShockFilterConfig::default(),
            cal,
            ExitPolicy::FixedHorizonSessions { n: 2 },
            Exposure::Long(Symbol("SPY".into())),
            EventShockControlConfig {
                seed: 1,
                controls_per_treatment: 0,
                strategy_name: "timing".into(),
                horizon_sessions: 2,
                exposure: Exposure::Long(Symbol("SPY".into())),
                vol_epsilon: None,
            },
            cand.clone(),
            timing,
            "timing",
        )
    };
    let shocks = vec![sample_shocks()[0].clone()];
    let replay = build_vertical_replay(shocks, bars);
    let a = collect_trades(&mk(ExecutionEntryTiming::EntrySessionOpen), &replay);
    let b = collect_trades(&mk(ExecutionEntryTiming::NextSessionOpen), &replay);
    assert_eq!(a.len(), 1);
    assert_eq!(b.len(), 1);
    assert_ne!(a[0].gross_return, b[0].gross_return);
}
