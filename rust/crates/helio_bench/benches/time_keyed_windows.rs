//! Time-keyed and session-keyed rolling buffers vs sample-count baseline.

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use helio_scan::{Scan, SessionDate, VecEmitter};
use helio_time::{
    Bounds, FixedStep, FixedUnit, Frequency, SimpleWeekdayCalendar, TradingCalendar, WindowSpec,
};
use helio_window::{
    rolling_mean_scan, time_keyed_rolling_mean_scan, TimeKey, TimeKeyedRollingAggregatorScan,
    TimeKeyedSampleIn, TimeKeyedWindowState,
};
use helio_window::{SessionKeyedRollingState, SumCountMeanAggregator};

const BATCH: u64 = 4096;

fn bench_time_keyed_state(c: &mut Criterion) {
    let spec = WindowSpec::Trailing {
        size: Frequency::Fixed(FixedStep {
            n: 3600,
            unit: FixedUnit::Second,
        }),
        bounds: Bounds::LEFT_CLOSED_RIGHT_OPEN,
    };
    let mut group = c.benchmark_group("time_keyed_window_state");
    group.throughput(Throughput::Elements(BATCH));
    group.bench_function("push_f64_span_1h_batch4096", |b| {
        b.iter(|| {
            let mut w =
                TimeKeyedWindowState::new(spec, SumCountMeanAggregator::default()).unwrap();
            for i in 0..BATCH {
                let t = (i as i64) * 30;
                w.push(TimeKey(t), black_box(i as f64 * 0.0001));
            }
            black_box(w.summary().count)
        });
    });
    group.finish();
}

fn bench_time_keyed_scan(c: &mut Criterion) {
    let spec = WindowSpec::Trailing {
        size: Frequency::Fixed(FixedStep {
            n: 600,
            unit: FixedUnit::Second,
        }),
        bounds: Bounds::LEFT_CLOSED_RIGHT_OPEN,
    };
    let scan: TimeKeyedRollingAggregatorScan<f64, SumCountMeanAggregator> =
        time_keyed_rolling_mean_scan(spec);
    let mut group = c.benchmark_group("time_keyed_rolling_scan");
    group.throughput(Throughput::Elements(BATCH));
    group.bench_function("emit_summary_batch4096", |b| {
        b.iter(|| {
            let mut st = scan.init();
            let mut e = VecEmitter::new();
            for i in 0..BATCH {
                scan.step(
                    &mut st,
                    TimeKeyedSampleIn {
                        key_secs: (i as i64) * 15,
                        value: black_box(i as f64 * 0.0001),
                    },
                    &mut e,
                );
            }
            black_box(e.0.len())
        });
    });
    group.finish();
}

fn bench_session_keyed_state(c: &mut Criterion) {
    let cal = SimpleWeekdayCalendar;
    let mut group = c.benchmark_group("session_keyed_window_state");
    group.throughput(Throughput::Elements(BATCH));
    group.bench_function("trailing_5_sessions_batch4096", |b| {
        b.iter(|| {
            let mut w =
                SessionKeyedRollingState::new(cal, 5, SumCountMeanAggregator::default()).unwrap();
            let mut d = SessionDate(10_000);
            for i in 0..BATCH {
                w.push(d, black_box(i as f64 * 0.0001));
                d = cal.next_session_after(d);
            }
            black_box(w.summary().count)
        });
    });
    group.finish();
}

fn bench_sample_count_baseline(c: &mut Criterion) {
    let scan = rolling_mean_scan(64);
    let mut group = c.benchmark_group("sample_count_baseline");
    group.throughput(Throughput::Elements(BATCH));
    group.bench_function("rolling_mean_64_batch4096", |b| {
        b.iter(|| {
            let mut st = scan.init();
            let mut e = VecEmitter::new();
            for i in 0..BATCH {
                scan.step(&mut st, black_box(i as f64 * 0.0001), &mut e);
            }
            black_box(e.0.len())
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_time_keyed_state,
    bench_time_keyed_scan,
    bench_session_keyed_state,
    bench_sample_count_baseline
);
criterion_main!(benches);
