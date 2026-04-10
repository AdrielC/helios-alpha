//! Integration: **ticks → time buckets → mean → EMA → sequential diff** (`scan_then!`).

use helio_scan::{scan_then, Arr, FlushReason, FlushableScan, Scan, VecEmitter};
use helio_time::NanosecondWallBucket;
use helio_window::{
    BucketBarClose, EmaScan, PriceTick, SequentialDiffScan, TimeBucketAggregatorScan,
};

const NS_PER_SEC: i64 = 1_000_000_000;
const BUCKET_NS: i64 = 60 * NS_PER_SEC;

fn pipeline(alpha: f64) -> impl Scan<In = PriceTick, Out = f64> {
    scan_then!(
        TimeBucketAggregatorScan::<NanosecondWallBucket, PriceTick>::new(NanosecondWallBucket {
            width_ns: BUCKET_NS,
        }),
        Arr::<_, BucketBarClose<NanosecondWallBucket>, f64>::new(
            |b: BucketBarClose<NanosecondWallBucket>| b.mean,
        ),
        EmaScan::new(alpha),
        SequentialDiffScan::<f64>::new(),
    )
}

#[test]
fn composed_pipeline_bucket_then_ema_then_diff() {
    let p = pipeline(1.0);
    let mut st = p.init();
    let mut out = VecEmitter::new();

    p.step(
        &mut st,
        PriceTick {
            t_ns: 0,
            price: 100.0,
        },
        &mut out,
    );
    p.step(
        &mut st,
        PriceTick {
            t_ns: 30 * NS_PER_SEC,
            price: 120.0,
        },
        &mut out,
    );
    assert!(out.0.is_empty(), "no bar until bucket rolls");

    p.step(
        &mut st,
        PriceTick {
            t_ns: BUCKET_NS,
            price: 200.0,
        },
        &mut out,
    );
    assert!(
        out.0.is_empty(),
        "first bar updates EMA; SequentialDiff waits for second EMA"
    );

    p.step(
        &mut st,
        PriceTick {
            t_ns: 2 * BUCKET_NS,
            price: 300.0,
        },
        &mut out,
    );
    assert_eq!(out.0.len(), 1);
    assert!((out.0[0] - 90.0).abs() < 1e-6, "got {}", out.0[0]);
}

#[test]
fn flush_emits_partial_bucket() {
    let s = TimeBucketAggregatorScan::<NanosecondWallBucket, PriceTick>::new(NanosecondWallBucket {
        width_ns: BUCKET_NS,
    });
    let mut st = s.init();
    let mut e = VecEmitter::new();
    s.step(
        &mut st,
        PriceTick {
            t_ns: 0,
            price: 10.0,
        },
        &mut e,
    );
    s.flush(&mut st, FlushReason::EndOfInput, &mut e);
    assert_eq!(e.0.len(), 1);
    assert!((e.0[0].mean - 10.0).abs() < 1e-9);
}
