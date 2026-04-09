//! End-to-end-ish hot path: causal pipeline steps (helio_event).

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use helio_event::{
    AvailabilityTagged, CausalEventStudyConfig, CausalEventStudyPipeline, ReplayRecord,
    TreatmentEvent,
};
use helio_scan::{Scan, VecEmitter};
use helio_time::AvailableAt;

const EVENTS: usize = 512;

fn build_stream(n: usize) -> Vec<ReplayRecord> {
    let mut v = Vec::with_capacity(n * 2);
    for i in 0..n {
        v.push(ReplayRecord::Bar {
            session_day: i as i32 * 2,
            close: 100.0 + i as f64 * 0.01,
        });
        v.push(ReplayRecord::Treatment(AvailabilityTagged {
            value: TreatmentEvent {
                id: i as u32,
                day: (i * 2) as i64,
                strength: 1.0,
                horizon_trading_days: 5,
            },
            observed_at: None,
            available_at: AvailableAt(0),
            effective_at: None,
            session_date: None,
        }));
    }
    v
}

fn causal_pipeline_step(c: &mut Criterion) {
    let cfg = CausalEventStudyConfig {
        decision_available: AvailableAt(10_000),
        overlap: helio_event::OverlapConfig { max_gap_days: 3 },
    };
    let pipe = CausalEventStudyPipeline::new(cfg);
    let stream = build_stream(EVENTS);
    let total = stream.len() as u64;

    let mut group = c.benchmark_group("event_study_pipeline");
    group.throughput(Throughput::Elements(total));
    group.bench_function("replay_records_512pairs", |b| {
        b.iter(|| {
            let mut st = pipe.init();
            let mut out = VecEmitter::new();
            for r in stream.iter() {
                pipe.step(&mut st, black_box(r.clone()), &mut out);
            }
            black_box(out.0.len())
        });
    });
    group.finish();
}

criterion_group!(benches, causal_pipeline_step);
criterion_main!(benches);
