//! Single-item vs batched stepping vs iterator runner (same semantics).

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use helio_scan::{run_iter, BatchOptimizedScan, Scan, ScanBatchExt, Then, VecEmitter, ZipInput};

const N: u64 = 4096;

#[derive(Clone, Copy, Debug)]
struct PassThrough;

impl Scan for PassThrough {
    type In = u64;
    type Out = u64;
    type State = ();

    fn init(&self) -> Self::State {}

    fn step<E>(&self, _st: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: helio_scan::Emit<Self::Out>,
    {
        emit.emit(input);
    }
}

fn batch_vs_single_then(c: &mut Criterion) {
    let pipe = Then {
        left: PassThrough,
        right: PassThrough,
    };
    let batch: Vec<u64> = (0..N).collect();

    let mut group = c.benchmark_group("execution_then");
    group.throughput(Throughput::Elements(N));

    group.bench_function("step_each", |b| {
        b.iter(|| {
            let mut st = pipe.init();
            let mut out = VecEmitter::new();
            for x in 0..N {
                pipe.step(&mut st, black_box(x), &mut out);
            }
            black_box(out.0.len())
        });
    });

    group.bench_function("step_batch_slice", |b| {
        b.iter(|| {
            let mut st = pipe.init();
            let mut out = VecEmitter::new();
            pipe.step_batch(&mut st, black_box(batch.iter().copied()), &mut out);
            black_box(out.0.len())
        });
    });

    group.bench_function("run_iter", |b| {
        b.iter(|| {
            let mut st = pipe.init();
            let mut out = VecEmitter::new();
            run_iter(&pipe, &mut st, black_box(batch.iter().copied()), &mut out);
            black_box(out.0.len())
        });
    });

    group.finish();
}

fn zip_input_batch(c: &mut Criterion) {
    let z = ZipInput {
        a: PassThrough,
        b: PassThrough,
    };
    let batch: Vec<u64> = (0..N).collect();

    let mut group = c.benchmark_group("execution_zip");
    group.throughput(Throughput::Elements(N));

    group.bench_function("step_each", |b| {
        b.iter(|| {
            let mut st = z.init();
            let mut out = VecEmitter::new();
            for x in 0..N {
                z.step(&mut st, black_box(x), &mut out);
            }
            black_box(out.0.len())
        });
    });

    group.bench_function("step_batch", |b| {
        b.iter(|| {
            let mut st = z.init();
            let mut out = VecEmitter::new();
            z.step_batch(&mut st, black_box(batch.iter().copied()), &mut out);
            black_box(out.0.len())
        });
    });

    group.finish();
}

fn rolling_window_batch_opt(c: &mut Criterion) {
    use helio_window::RollingWindowScan;

    let scan = RollingWindowScan::new(32);
    let batch: Vec<i64> = (0..N as i64).collect();

    let mut group = c.benchmark_group("rolling_window_batch");
    group.throughput(Throughput::Elements(N));

    group.bench_function("step_each", |b| {
        b.iter(|| {
            let mut st = scan.init();
            let mut out = VecEmitter::new();
            for x in &batch {
                scan.step(&mut st, black_box(*x), &mut out);
            }
            black_box(out.0.len())
        });
    });

    group.bench_function("step_batch_optimized", |b| {
        b.iter(|| {
            let mut st = scan.init();
            let mut out = VecEmitter::new();
            scan.step_batch_optimized(&mut st, black_box(batch.as_slice()), &mut out);
            black_box(out.0.len())
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    batch_vs_single_then,
    zip_input_batch,
    rolling_window_batch_opt
);
criterion_main!(benches);
