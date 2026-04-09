//! Throughput of ring-buffer rolling paths (sample-count windows).

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use helio_scan::{Scan, VecEmitter};
use helio_time::WindowSpec;
use helio_window::{rolling_mean_scan, RollingFoldScan, RollingWindowScan};

const BATCH: u64 = 4096;

fn rolling_mean_window64(c: &mut Criterion) {
    let scan = rolling_mean_scan(64);
    let mut group = c.benchmark_group("rolling_mean");
    group.throughput(Throughput::Elements(BATCH));
    group.bench_function("window64_f64_batch4096", |b| {
        b.iter(|| {
            let mut st = scan.init();
            let mut out = VecEmitter::new();
            for i in 0..BATCH {
                scan.step(&mut st, black_box(i as f64 * 0.0001), &mut out);
            }
            black_box(out.0.len())
        });
    });
    group.finish();
}

fn rolling_vec_snapshot(c: &mut Criterion) {
    let scan = RollingWindowScan::new(64);
    let mut group = c.benchmark_group("rolling_vec_snapshot");
    group.throughput(Throughput::Elements(BATCH));
    group.bench_function("window64_batch4096", |b| {
        b.iter(|| {
            let mut st = scan.init();
            let mut out = VecEmitter::new();
            for i in 0..BATCH {
                scan.step(&mut st, black_box(i as i64), &mut out);
            }
            black_box(out.0.len())
        });
    });
    group.finish();
}

fn rolling_fold_max(c: &mut Criterion) {
    let scan = RollingFoldScan::new(WindowSpec::trailing_samples(128), 0i64, |xs: &[i64]| {
        *xs.iter().max().unwrap_or(&0)
    });
    let mut group = c.benchmark_group("rolling_fold");
    group.throughput(Throughput::Elements(BATCH));
    group.bench_function("max_window128_batch4096", |b| {
        b.iter(|| {
            let mut st = scan.init();
            let mut out = VecEmitter::new();
            for i in 0..BATCH {
                scan.step(&mut st, black_box(i as i64), &mut out);
            }
            black_box(out.0.len())
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    rolling_mean_window64,
    rolling_vec_snapshot,
    rolling_fold_max
);
criterion_main!(benches);
