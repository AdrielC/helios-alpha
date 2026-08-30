//! Online update and deterministic parallel-merge costs.

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use helio_stats::{
    merge_moments_balanced, CompensatedSum, GammaPoisson, NormalInverseGamma, OnlineCovariance,
    OnlineMoments, ScalarPosterior, ScaledSumSquares, StrategyFingerprint, ThompsonKey,
};

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

fn guarded_accumulators(c: &mut Criterion) {
    let values = values();
    let mut group = c.benchmark_group("guarded_accumulators");
    group.throughput(Throughput::Elements(N as u64));
    group.bench_function("neumaier_sum_65536", |b| {
        b.iter(|| {
            let mut state = CompensatedSum::new();
            for &value in &values {
                state.try_push(black_box(value)).unwrap();
            }
            black_box(state.try_total().unwrap())
        });
    });
    group.bench_function("scaled_norm_65536", |b| {
        b.iter(|| {
            let mut state = ScaledSumSquares::new();
            for &value in &values {
                state.try_push(black_box(value)).unwrap();
            }
            black_box(state.try_norm().unwrap())
        });
    });
    group.finish();
}

fn bayesian_updates(c: &mut Criterion) {
    let effect = NormalInverseGamma::try_new(0.0, 1.0, 2.0, 1.0).unwrap();
    let observations: Vec<f64> = (0..N)
        .map(|index| (index as f64 * 0.013).sin() * 0.02)
        .collect();
    let mut group = c.benchmark_group("bayesian_streams");
    group.throughput(Throughput::Elements(N as u64));
    group.bench_function("normal_inverse_gamma_push_65536", |b| {
        b.iter(|| {
            let mut state = effect.init();
            for &value in &observations {
                state.try_observe(black_box(value)).unwrap();
            }
            black_box(effect.try_posterior(&state).unwrap())
        });
    });

    let arrivals = GammaPoisson::try_new(1.0, 60.0).unwrap();
    let mut state = arrivals.init();
    state.try_observe(37, 3_600.0).unwrap();
    let posterior = arrivals.try_posterior(&state).unwrap();
    group.throughput(Throughput::Elements(1));
    group.bench_function("keyed_gamma_poisson_draw", |b| {
        let mut decision = 0_u64;
        b.iter(|| {
            decision = decision.wrapping_add(1);
            black_box(
                posterior
                    .try_draw(ThompsonKey::new(
                        StrategyFingerprint::from_bytes([7; 32]),
                        black_box(decision),
                        11,
                    ))
                    .unwrap(),
            )
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    online_moments,
    balanced_merge,
    online_covariance,
    guarded_accumulators,
    bayesian_updates
);
criterion_main!(benches);
