//! Step / emit / snapshot pressure points.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use helio_scan::{
    CheckpointKeyFn, FlushReason, FlushableScan, HashMapStore, Persisted, Scan, ScanExt,
    SnapshottingScan, VecEmitter,
};

#[derive(Clone, Copy, Debug)]
struct MultiEmit {
    pub k: usize,
}

impl Scan for MultiEmit {
    type In = u64;
    type Out = u64;
    type State = ();

    fn init(&self) -> Self::State {}

    fn step<E>(&self, _st: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: helio_scan::Emit<Self::Out>,
    {
        for i in 0..self.k {
            emit.emit(input.wrapping_add(i as u64));
        }
    }
}

fn emit_fanout(c: &mut Criterion) {
    let mut group = c.benchmark_group("emit_fanout");
    for k in [0usize, 1, 4, 16] {
        let s = MultiEmit { k };
        group.bench_function(format!("k{k}_2048_steps"), |b| {
            b.iter(|| {
                let mut st = s.init();
                let mut out = VecEmitter::new();
                for i in 0..2048u64 {
                    s.step(&mut st, black_box(i), &mut out);
                }
                black_box(out.0.len())
            });
        });
    }
    group.finish();
}

#[derive(Clone, Copy, Debug)]
struct MapChain;

impl Scan for MapChain {
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

fn composed_map_depth(c: &mut Criterion) {
    let s = MapChain
        .map(|x| x + 1)
        .map(|x| x * 2)
        .map(|x| x ^ 0xAAAA)
        .filter_map(|x| if x % 3 == 0 { Some(x) } else { None });
    let mut group = c.benchmark_group("composed_scan");
    group.bench_function("map_map_map_filter_8192", |b| {
        b.iter(|| {
            let mut st = s.init();
            let mut out = VecEmitter::new();
            for i in 0..8192u64 {
                s.step(&mut st, black_box(i), &mut out);
            }
            black_box(out.0.len())
        });
    });
    group.finish();
}

#[derive(Clone)]
struct Key;
impl CheckpointKeyFn<u64> for Key {
    type Key = &'static str;
    fn key_for_offset(&self, _o: &u64) -> Self::Key {
        "p"
    }
}

#[derive(Clone, Copy, Debug)]
struct Inc;

impl Scan for Inc {
    type In = u64;
    type Out = u64;
    type State = u64;

    fn init(&self) -> Self::State {
        0
    }

    fn step<E>(&self, st: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: helio_scan::Emit<Self::Out>,
    {
        *st += input;
        emit.emit(*st);
    }
}

impl FlushableScan for Inc {
    type Offset = u64;

    fn flush<E>(&self, _st: &mut Self::State, _sig: FlushReason<Self::Offset>, _emit: &mut E)
    where
        E: helio_scan::Emit<Self::Out>,
    {
    }
}

impl SnapshottingScan for Inc {
    type Snapshot = u64;

    fn snapshot(&self, st: &Self::State) -> Self::Snapshot {
        *st
    }

    fn restore(&self, snap: Self::Snapshot) -> Self::State {
        snap
    }
}

fn persisted_checkpoint_step(c: &mut Criterion) {
    let mut group = c.benchmark_group("persisted_checkpoint");
    group.bench_function("checkpoint_every_64_2048_steps", |b| {
        b.iter(|| {
            let p = Persisted::new(Inc, HashMapStore::default(), Key);
            let mut st = p.init();
            let mut out = VecEmitter::new();
            for i in 0..2048u64 {
                p.step(&mut st, black_box(i), &mut out);
                if i % 64 == 63 {
                    p.flush(&mut st, FlushReason::Checkpoint(i), &mut out);
                }
            }
            black_box(out.0.len())
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    emit_fanout,
    composed_map_depth,
    persisted_checkpoint_step
);
criterion_main!(benches);
