//! Online update and deterministic parallel-merge costs.

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use helio_stats::{merge_moments_balanced, OnlineCovariance, OnlineMoments};

const N: usize = 65_536;

fn values() -> Vec<f64> {
    (0..N)
        .map(|i| 1e9 + (i as f64 * 0.017).sin() * 0.25)
        .collect()
}

fn online_moments(c: &mut Criterion) {
    let values = values();
    let mut group = c.benchmark_group("online_moments");
    group.throughput(Throughput::Elements(N as u64));
    group.bench_function("welford_push_65536", |b| {
        b.iter(|| {
            let mut state = OnlineMoments::new();
            for &value in &values {
                state.try_push(black_box(value)).unwrap();
            }
            black_box(state.sample_variance())
        });
    });
    group.finish();
}

fn balanced_merge(c: &mut Criterion) {
    let partials: Vec<OnlineMoments> = values()
        .chunks(256)
        .map(|chunk| {
            let mut state = OnlineMoments::new();
            for &value in chunk {
                state.try_push(value).unwrap();
            }
            state
        })
        .collect();
    let mut group = c.benchmark_group("online_moments_merge");
    group.throughput(Throughput::Elements(N as u64));
    group.bench_function("balanced_256_partitions", |b| {
        b.iter(|| {
            let merged = merge_moments_balanced(black_box(partials.clone())).unwrap();
            black_box(merged.sample_variance())
        });
    });
    group.finish();
}

fn online_covariance(c: &mut Criterion) {
    let values = values();
    let mut group = c.benchmark_group("online_covariance");
    group.throughput(Throughput::Elements(N as u64));
    group.bench_function("push_pair_65536", |b| {
        b.iter(|| {
            let mut state = OnlineCovariance::new();
            for &x in &values {
                state
                    .try_push(black_box(x), black_box(x * 1.5 - 7.0))
                    .unwrap();
            }
            black_box(state.sample_correlation())
        });
    });
    group.finish();
}

criterion_group!(benches, online_moments, balanced_merge, online_covariance);
criterion_main!(benches);
