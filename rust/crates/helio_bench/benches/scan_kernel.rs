//! Combinator and small-scan overhead (helio_scan kernel).

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use helio_scan::{Emit, FlushReason, FlushableScan, Scan, ScanExt, Then, VecEmitter, ZipInput};

const N: u64 = 8192;

#[derive(Clone, Copy, Debug)]
struct PassThrough;

impl Scan for PassThrough {
    type In = u64;
    type Out = u64;
    type State = ();

    fn init(&self) -> Self::State {}

    fn step<E>(&self, _st: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        emit.emit(input);
    }
}

impl FlushableScan for PassThrough {
    type Offset = u64;

    fn flush<E>(&self, _st: &mut Self::State, _sig: FlushReason<Self::Offset>, _emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
    }
}

fn then_chain(c: &mut Criterion) {
    let pipe = Then {
        left: PassThrough,
        right: PassThrough,
    };
    let mut group = c.benchmark_group("scan_then");
    group.throughput(Throughput::Elements(N));
    group.bench_function("pass_through_x2_u64", |b| {
        b.iter(|| {
            let mut st = pipe.init();
            let mut out = VecEmitter::new();
            for i in 0..N {
                pipe.step(&mut st, black_box(i), &mut out);
            }
            black_box(out.0.len())
        });
    });
    group.finish();
}

fn map_overhead(c: &mut Criterion) {
    let pipe = PassThrough.map(|x| x.wrapping_mul(7));
    let mut group = c.benchmark_group("scan_map");
    group.throughput(Throughput::Elements(N));
    group.bench_function("mul7_u64", |b| {
        b.iter(|| {
            let mut st = pipe.init();
            let mut out = VecEmitter::new();
            for i in 0..N {
                pipe.step(&mut st, black_box(i), &mut out);
            }
            black_box(out.0.last().copied())
        });
    });
    group.finish();
}

fn zip_input(c: &mut Criterion) {
    let z = ZipInput {
        a: PassThrough,
        b: PassThrough,
    };
    let mut group = c.benchmark_group("scan_zip_input");
    group.throughput(Throughput::Elements(N));
    group.bench_function("duplicate_emit_u64", |b| {
        b.iter(|| {
            let mut st = z.init();
            let mut out = VecEmitter::new();
            for i in 0..N {
                z.step(&mut st, black_box(i), &mut out);
            }
            black_box(out.0.len())
        });
    });
    group.finish();
}

criterion_group!(benches, then_chain, map_overhead, zip_input);
criterion_main!(benches);
