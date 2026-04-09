//! Event-shock vertical: ingest → align → signal → execution; checkpoint slice.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use helio_event::*;
use helio_scan::{
    run_slice, FlushReason, FlushableScan, HashMapStore, Persisted, Runner, Scan, ScanBatchExt,
    SessionDate, VecEmitter,
};
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
            horizon_sessions: 4,
            exposure: Exposure::Pair {
                long: Symbol("XLU".into()),
                short: Symbol("SPY".into()),
            },
            vol_epsilon: None,
        },
        cand,
        helio_event::ExecutionEntryTiming::EntrySessionOpen,
    )
}

fn bench_align_to_signal(c: &mut Criterion) {
    let cal = SimpleWeekdayCalendar;
    let stream = replay_stream();
    let shocks: Vec<_> = stream
        .iter()
        .filter_map(|r| match r {
            EventShockVerticalRecord::Shock(_, t) => Some(t.clone()),
            _ => None,
        })
        .collect();
    c.bench_function("event_shock_align_pipeline", |b| {
        let pipe = EventShockAlignPipelineScan::new(None, EventShockFilterConfig::default(), cal);
        b.iter(|| {
            let mut st = black_box(pipe.init());
            let mut e = VecEmitter::new();
            for s in &shocks {
                pipe.step(&mut st, s.clone(), &mut e);
            }
            black_box(e.into_inner().len())
        });
    });

    let aligned: Vec<_> = {
        let pipe = EventShockAlignPipelineScan::new(None, EventShockFilterConfig::default(), cal);
        let mut st = pipe.init();
        let mut e = VecEmitter::new();
        for s in &shocks {
            pipe.step(&mut st, s.clone(), &mut e);
        }
        e.into_inner()
    };
    let to_sig = EventShockToSignalScan {
        exit_policy: ExitPolicy::FixedHorizonSessions { n: 4 },
        exposure: Exposure::Pair {
            long: Symbol("XLU".into()),
            short: Symbol("SPY".into()),
        },
        calendar: cal,
    };
    c.bench_function("event_shock_aligned_to_signal", |b| {
        b.iter(|| {
            let mut st = black_box(to_sig.init());
            let mut e = VecEmitter::new();
            for a in &aligned {
                to_sig.step(&mut st, a.clone(), &mut e);
            }
            black_box(e.into_inner().len())
        });
    });
}

fn bench_execution_e2e(c: &mut Criterion) {
    let stream = replay_stream();
    let vertical = vertical_machine();
    c.bench_function("event_shock_e2e_replay", |b| {
        b.iter(|| {
            let mut st = black_box(vertical.init());
            let mut e = VecEmitter::new();
            for r in &stream {
                vertical.step(&mut st, r.clone(), &mut e);
            }
            vertical.flush(&mut st, FlushReason::EndOfInput, &mut e);
            black_box(e.into_inner().len())
        });
    });

    c.bench_function("event_shock_e2e_step_batch_slice", |b| {
        b.iter(|| {
            let mut st = black_box(vertical.init());
            let mut e = VecEmitter::new();
            vertical.step_batch(&mut st, black_box(stream.iter().cloned()), &mut e);
            vertical.flush(&mut st, FlushReason::EndOfInput, &mut e);
            black_box(e.into_inner().len())
        });
    });

    c.bench_function("event_shock_e2e_run_slice", |b| {
        b.iter(|| {
            let mut st = black_box(vertical.init());
            let mut e = VecEmitter::new();
            run_slice(&vertical, &mut st, black_box(stream.as_slice()), &mut e);
            vertical.flush(&mut st, FlushReason::EndOfInput, &mut e);
            black_box(e.into_inner().len())
        });
    });
}

#[derive(Clone)]
struct CpKey;
impl helio_scan::CheckpointKeyFn<u64> for CpKey {
    type Key = &'static str;
    fn key_for_offset(&self, _offset: &u64) -> Self::Key {
        "cp"
    }
}

fn bench_checkpoint_restart(c: &mut Criterion) {
    let stream = replay_stream();
    c.bench_function("event_shock_checkpoint_restart", |b| {
        b.iter(|| {
            let inner = vertical_machine();
            let persisted = Persisted::new(inner, HashMapStore::default(), CpKey);
            let mut r = Runner::new(persisted);
            let mut e = VecEmitter::new();
            let mid = stream.len() / 2;
            for (i, rec) in stream.iter().enumerate() {
                r.step(rec.clone(), &mut e);
                if i + 1 == mid {
                    r.flush(FlushReason::Checkpoint(1u64), &mut e);
                }
            }
            r.flush(FlushReason::EndOfInput, &mut e);
            black_box(e.into_inner().len())
        });
    });
}

criterion_group!(
    benches,
    bench_align_to_signal,
    bench_execution_e2e,
    bench_checkpoint_restart,
);
criterion_main!(benches);
