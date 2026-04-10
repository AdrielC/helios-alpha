//! Checkpoint snapshot frequency on the event-shock vertical (same workload as `event_shock_vertical`).

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use helio_event::{
    collect_vertical_trades_incremental, collect_vertical_trades_with_checkpoint_cadence,
    build_vertical_replay, candidate_entries_from_bars, DailyBar, EventId, EventKind, EventScope,
    EventShock, EventShockControlConfig, EventShockFilterConfig, EventShockVerticalRecord,
    EventShockVerticalScan,
    ExitPolicy, Exposure, Symbol,
};
use helio_scan::SessionDate;
use helio_time::{AvailableAt, SimpleWeekdayCalendar};

fn bench_events() -> Vec<EventShock> {
    (0..256)
        .map(|i| {
            let d = 20 + (i as i32 % 40);
            EventShock {
                id: EventId(i),
                kind: EventKind::Other,
                tags: String::new(),
                observed_at: None,
                available_at: AvailableAt((d as i64) * 86_400 + 100),
                impact_start: (d as i64 + 5) * 86_400,
                impact_end: (d as i64 + 10) * 86_400,
                severity: 1.0,
                confidence: 1.0,
                scope: EventScope::Global,
            }
        })
        .collect()
}

fn bench_bars() -> Vec<DailyBar> {
    let mut v = Vec::new();
    for d in 10..100i32 {
        for sym in ["XLU", "SPY"] {
            v.push(DailyBar {
                session: SessionDate(d),
                symbol: Symbol(sym.into()),
                open: 100.0 + d as f64,
                high: 101.0 + d as f64,
                low: 99.0 + d as f64,
                close: 100.5 + d as f64,
            });
        }
    }
    v
}

fn replay_stream() -> Vec<EventShockVerticalRecord> {
    build_vertical_replay(bench_events(), bench_bars())
}

fn vertical_machine() -> EventShockVerticalScan<SimpleWeekdayCalendar> {
    let cal = SimpleWeekdayCalendar;
    let bars = bench_bars();
    let cand = candidate_entries_from_bars(&bars);
    EventShockVerticalScan::new(
        None,
        EventShockFilterConfig::default(),
        cal,
        ExitPolicy::FixedHorizonSessions { n: 4 },
        Exposure::Pair {
            long: Symbol("XLU".into()),
            short: Symbol("SPY".into()),
        },
        EventShockControlConfig {
            seed: 11,
            controls_per_treatment: 1,
            strategy_name: "bench".into(),
            horizon_sessions: 4,
            exposure: Exposure::Pair {
                long: Symbol("XLU".into()),
                short: Symbol("SPY".into()),
            },
            vol_epsilon: None,
        },
        cand,
        helio_event::ExecutionEntryTiming::EntrySessionOpen,
        "bench",
    )
}

fn bench_checkpoint_every(c: &mut Criterion) {
    let stream = replay_stream();
    let vertical = vertical_machine();
    let full = collect_vertical_trades_incremental(&vertical, &stream);

    let mut group = c.benchmark_group("event_shock_checkpoint_cadence");
    for every in [1usize, 64, 256, 1024] {
        group.bench_function(format!("every_{every}"), |b| {
            b.iter(|| {
                let out = collect_vertical_trades_with_checkpoint_cadence(
                    black_box(&vertical),
                    black_box(stream.as_slice()),
                    black_box(every),
                );
                assert_eq!(out, full);
                black_box(out.len())
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_checkpoint_every);
criterion_main!(benches);
