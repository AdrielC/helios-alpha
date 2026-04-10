//! Composable **timed event → time bucket → smooth → change** on [`Scan`](helio_scan::Scan).
//!
//! ## Two generic parameters
//!
//! - **`G: WallBucketGrid`** ([`helio_time::WallBucketGrid`]) — **what a bucket is**: width + `floor(t)` on
//!   timeline coordinate `G::T` (e.g. [`NanosecondWallBucket`](helio_time::NanosecondWallBucket) or
//!   [`SecondWallBucket`](helio_time::SecondWallBucket)).
//! - **`V: TimeBucketEvent<G>`** — **payload**: [`TimeBucketEvent::bucket_time`] picks the coordinate in
//!   the **same unit** as `G` (ns vs sec is a contract of your grid + event), plus [`mean_sample`]
//!   for the running mean.
//!
//! Use [`helio_time::Timed`] with a small wrapper (e.g. [`TimedPriceEvent`]) when the wall clock lives
//! in `available_at` / `effective_at` rather than a dedicated `t_ns` field.
//!
//! Composition: `TimeBucketAggregatorScan::<G,V>::new(grid) → Arr(mean) → EmaScan → SequentialDiffScan::<f64>`.

use std::marker::PhantomData;
use std::ops::Sub;

use helio_scan::{
    Emit, FlushReason, FlushableScan, Scan, SnapshottingScan, VersionedSnapshot,
};
use helio_time::{AvailableAt, NanosecondWallBucket, Timed, WallBucketGrid};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

// --- Event side: generic over bucket grid ---

/// A sample that can be placed on grid **`G`** and contributes **`mean_sample`** to the bar average.
pub trait TimeBucketEvent<G: WallBucketGrid>: Clone {
    /// Timeline coordinate in the **same unit** as `G::T` (ns for [`NanosecondWallBucket`], seconds for [`SecondWallBucket`](helio_time::SecondWallBucket), etc.).
    fn bucket_time(&self, grid: &G) -> G::T;
    /// Summand for arithmetic mean over the bucket (`sum / count`).
    fn mean_sample(&self) -> f64;
}

/// Trade tick: **nanoseconds** since epoch + price. Use with [`NanosecondWallBucket`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PriceTick {
    pub t_ns: i64,
    pub price: f64,
}

impl TimeBucketEvent<NanosecondWallBucket> for PriceTick {
    fn bucket_time(&self, _grid: &NanosecondWallBucket) -> i64 {
        self.t_ns
    }

    fn mean_sample(&self) -> f64 {
        self.price
    }
}

/// [`Timed`] payload where **`available_at`** is interpreted as **epoch nanoseconds** for bucketing
/// (same unit as [`NanosecondWallBucket`]); `value` is the scalar averaged in the bar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimedPriceEvent {
    pub inner: Timed<f64>,
}

impl TimedPriceEvent {
    pub fn new(price: f64, available_at_ns: i64) -> Self {
        Self {
            inner: Timed::new(price, AvailableAt(available_at_ns)),
        }
    }
}

impl TimeBucketEvent<NanosecondWallBucket> for TimedPriceEvent {
    fn bucket_time(&self, _grid: &NanosecondWallBucket) -> i64 {
        self.inner.available_at.0
    }

    fn mean_sample(&self) -> f64 {
        self.inner.value
    }
}

/// Closed bucket on grid **`G`**: half-open `[bucket_start, bucket_end)` in `G::T` space.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BucketBarClose<G: WallBucketGrid> {
    pub bucket_start: G::T,
    pub bucket_end: G::T,
    pub mean: f64,
    pub tick_count: u64,
}

impl<G: WallBucketGrid> BucketBarClose<G> {
    #[inline]
    pub fn mean_price(&self) -> f64 {
        self.mean
    }
}

/// Aggregate **`V`** into buckets defined by **`G`**; emit when the bucket key **changes** or on `flush`.
#[derive(Debug, Clone)]
pub struct TimeBucketAggregatorScan<G: WallBucketGrid, V: TimeBucketEvent<G>> {
    pub grid: G,
    _p: PhantomData<V>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimeBucketAggregatorState<G: WallBucketGrid> {
    pub open_bucket_start: Option<G::T>,
    pub sum: f64,
    pub tick_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeBucketAggregatorSnapshot<G: WallBucketGrid> {
    pub open_bucket_start: Option<G::T>,
    pub sum_bits: u64,
    pub tick_count: u64,
}

fn f64_to_bits(x: f64) -> u64 {
    x.to_bits()
}

fn bits_to_f64(b: u64) -> f64 {
    f64::from_bits(b)
}

impl<G: WallBucketGrid, V: TimeBucketEvent<G>> TimeBucketAggregatorScan<G, V> {
    pub fn new(grid: G) -> Self {
        Self {
            grid,
            _p: PhantomData,
        }
    }

    fn flush_open_bucket<E: Emit<BucketBarClose<G>>>(
        &self,
        state: &mut TimeBucketAggregatorState<G>,
        bucket_start: G::T,
        emit: &mut E,
    ) {
        if state.tick_count == 0 {
            return;
        }
        let mean = state.sum / state.tick_count as f64;
        let end = self.grid.bucket_end_exclusive(bucket_start);
        emit.emit(BucketBarClose {
            bucket_start,
            bucket_end: end,
            mean,
            tick_count: state.tick_count,
        });
        state.sum = 0.0;
        state.tick_count = 0;
    }
}

impl<V: TimeBucketEvent<NanosecondWallBucket>> TimeBucketAggregatorScan<NanosecondWallBucket, V> {
    pub fn nanoseconds_width(width_ns: i64) -> Self {
        Self::new(NanosecondWallBucket { width_ns })
    }

    pub fn ten_minute_ns() -> Self {
        Self::new(NanosecondWallBucket::ten_minutes())
    }
}

impl<G: WallBucketGrid, V: TimeBucketEvent<G>> Scan for TimeBucketAggregatorScan<G, V> {
    type In = V;
    type Out = BucketBarClose<G>;
    type State = TimeBucketAggregatorState<G>;

    fn init(&self) -> Self::State {
        TimeBucketAggregatorState {
            open_bucket_start: None,
            sum: 0.0,
            tick_count: 0,
        }
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        if !self.grid.is_valid() {
            return;
        }
        let t = input.bucket_time(&self.grid);
        let b = self.grid.bucket_start(t);
        let v = input.mean_sample();
        match state.open_bucket_start {
            None => {
                state.open_bucket_start = Some(b);
                state.sum = v;
                state.tick_count = 1;
            }
            Some(cur) if cur == b => {
                state.sum += v;
                state.tick_count += 1;
            }
            Some(cur) => {
                self.flush_open_bucket(state, cur, emit);
                state.open_bucket_start = Some(b);
                state.sum = v;
                state.tick_count = 1;
            }
        }
    }
}

impl<G: WallBucketGrid, V: TimeBucketEvent<G>> FlushableScan for TimeBucketAggregatorScan<G, V> {
    type Offset = u64;

    fn flush<E>(&self, state: &mut Self::State, _signal: FlushReason<Self::Offset>, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        if let Some(cur) = state.open_bucket_start.take() {
            self.flush_open_bucket(state, cur, emit);
        }
    }
}

impl<G: WallBucketGrid, V: TimeBucketEvent<G>> SnapshottingScan for TimeBucketAggregatorScan<G, V> {
    type Snapshot = TimeBucketAggregatorSnapshot<G>;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        TimeBucketAggregatorSnapshot {
            open_bucket_start: state.open_bucket_start,
            sum_bits: f64_to_bits(state.sum),
            tick_count: state.tick_count,
        }
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        TimeBucketAggregatorState {
            open_bucket_start: snapshot.open_bucket_start,
            sum: bits_to_f64(snapshot.sum_bits),
            tick_count: snapshot.tick_count,
        }
    }
}

impl<G: WallBucketGrid> VersionedSnapshot for TimeBucketAggregatorSnapshot<G> {
    const VERSION: u32 = 1;
}

/// Exponential moving average on `f64`.
#[derive(Debug, Clone, Copy)]
pub struct EmaScan {
    pub alpha: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmaState {
    pub ema: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmaSnapshot {
    pub ema: Option<f64>,
}

impl EmaScan {
    pub fn new(alpha: f64) -> Self {
        Self { alpha }
    }
}

impl Scan for EmaScan {
    type In = f64;
    type Out = f64;
    type State = EmaState;

    fn init(&self) -> Self::State {
        EmaState { ema: None }
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        let a = self.alpha.clamp(0.0, 1.0);
        let y = match state.ema {
            None => input,
            Some(prev) => a * input + (1.0 - a) * prev,
        };
        state.ema = Some(y);
        emit.emit(y);
    }
}

impl FlushableScan for EmaScan {
    type Offset = u64;

    fn flush<E>(&self, _state: &mut Self::State, _signal: FlushReason<Self::Offset>, _emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
    }
}

impl SnapshottingScan for EmaScan {
    type Snapshot = EmaSnapshot;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        EmaSnapshot { ema: state.ema }
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        EmaState {
            ema: snapshot.ema,
        }
    }
}

impl VersionedSnapshot for EmaSnapshot {
    const VERSION: u32 = 1;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SequentialDiffScan<T> {
    _p: PhantomData<T>,
}

impl<T> SequentialDiffScan<T> {
    pub fn new() -> Self {
        Self { _p: PhantomData }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SequentialDiffState<T> {
    pub prev: Option<T>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SequentialDiffSnapshot<T> {
    pub prev: Option<T>,
}

impl<T: Copy + Sub<Output = T> + Serialize + DeserializeOwned> Scan for SequentialDiffScan<T> {
    type In = T;
    type Out = T;
    type State = SequentialDiffState<T>;

    fn init(&self) -> Self::State {
        SequentialDiffState { prev: None }
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        if let Some(p) = state.prev {
            emit.emit(input - p);
        }
        state.prev = Some(input);
    }
}

impl<T: Copy + Sub<Output = T> + Serialize + DeserializeOwned> FlushableScan for SequentialDiffScan<T> {
    type Offset = u64;

    fn flush<E>(&self, _state: &mut Self::State, _signal: FlushReason<Self::Offset>, _emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
    }
}

impl<T: Copy + Sub<Output = T> + Serialize + DeserializeOwned> SnapshottingScan
    for SequentialDiffScan<T>
{
    type Snapshot = SequentialDiffSnapshot<T>;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        SequentialDiffSnapshot {
            prev: state.prev,
        }
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        SequentialDiffState {
            prev: snapshot.prev,
        }
    }
}

impl<T: Serialize + DeserializeOwned> VersionedSnapshot for SequentialDiffSnapshot<T> {
    const VERSION: u32 = 1;
}

pub type SequentialDiffF64 = SequentialDiffScan<f64>;

#[cfg(test)]
mod tests {
    use super::*;
    use helio_scan::VecEmitter;
    use helio_time::SecondWallBucket;

    const MIN_NS: i64 = 60_000_000_000;
    const BUCKET_NS: i64 = 10 * MIN_NS;

    #[derive(Debug, Clone, Copy)]
    struct VolTickNs {
        t_ns: i64,
        vol: u32,
    }

    impl TimeBucketEvent<NanosecondWallBucket> for VolTickNs {
        fn bucket_time(&self, _g: &NanosecondWallBucket) -> i64 {
            self.t_ns
        }

        fn mean_sample(&self) -> f64 {
            self.vol as f64
        }
    }

    #[test]
    fn bucket_generic_vol_tick() {
        let s = TimeBucketAggregatorScan::new(NanosecondWallBucket {
            width_ns: BUCKET_NS,
        });
        let mut st = s.init();
        let mut e = VecEmitter::new();
        s.step(
            &mut st,
            VolTickNs {
                t_ns: 0,
                vol: 10,
            },
            &mut e,
        );
        s.step(
            &mut st,
            VolTickNs {
                t_ns: BUCKET_NS,
                vol: 0,
            },
            &mut e,
        );
        assert_eq!(e.0.len(), 1);
        assert!((e.0[0].mean - 10.0).abs() < 1e-9);
    }

    #[test]
    fn bucket_second_grid_price_tick_seconds() {
        /// Tick with **epoch seconds** (use with [`SecondWallBucket`]).
        #[derive(Debug, Clone, Copy)]
        struct PriceTickSec {
            t_sec: i64,
            price: f64,
        }

        impl TimeBucketEvent<SecondWallBucket> for PriceTickSec {
            fn bucket_time(&self, _g: &SecondWallBucket) -> i64 {
                self.t_sec
            }

            fn mean_sample(&self) -> f64 {
                self.price
            }
        }

        let grid = SecondWallBucket { width_sec: 60 };
        let s = TimeBucketAggregatorScan::<SecondWallBucket, PriceTickSec>::new(grid);
        let mut st = s.init();
        let mut e = VecEmitter::new();
        s.step(
            &mut st,
            PriceTickSec {
                t_sec: 0,
                price: 1.0,
            },
            &mut e,
        );
        s.step(
            &mut st,
            PriceTickSec {
                t_sec: 60,
                price: 2.0,
            },
            &mut e,
        );
        assert_eq!(e.0.len(), 1);
        assert_eq!(e.0[0].bucket_start, 0);
        assert_eq!(e.0[0].bucket_end, 60);
    }

    #[test]
    fn timed_price_event_buckets_by_available_at() {
        let grid = NanosecondWallBucket {
            width_ns: BUCKET_NS,
        };
        let s = TimeBucketAggregatorScan::<NanosecondWallBucket, TimedPriceEvent>::new(grid);
        let mut st = s.init();
        let mut e = VecEmitter::new();
        s.step(&mut st, TimedPriceEvent::new(100.0, 0), &mut e);
        s.step(&mut st, TimedPriceEvent::new(110.0, 5 * MIN_NS), &mut e);
        assert!(e.0.is_empty());
        s.step(&mut st, TimedPriceEvent::new(50.0, BUCKET_NS), &mut e);
        assert_eq!(e.0.len(), 1);
        assert!((e.0[0].mean - 105.0).abs() < 1e-9);
    }

    #[test]
    fn bucket_emits_on_rollover_mean_correct() {
        let s = TimeBucketAggregatorScan::new(NanosecondWallBucket {
            width_ns: BUCKET_NS,
        });
        let mut st = s.init();
        let mut e = VecEmitter::new();
        s.step(
            &mut st,
            PriceTick {
                t_ns: 0,
                price: 100.0,
            },
            &mut e,
        );
        s.step(
            &mut st,
            PriceTick {
                t_ns: 5 * MIN_NS,
                price: 110.0,
            },
            &mut e,
        );
        assert!(e.0.is_empty());
        s.step(
            &mut st,
            PriceTick {
                t_ns: BUCKET_NS,
                price: 50.0,
            },
            &mut e,
        );
        assert_eq!(e.0.len(), 1);
        assert!((e.0[0].mean - 105.0).abs() < 1e-9);
        assert_eq!(e.0[0].tick_count, 2);
    }

    #[test]
    fn ema_and_diff_compose() {
        let e = EmaScan::new(0.5);
        let mut st = e.init();
        let mut out = VecEmitter::new();
        e.step(&mut st, 100.0, &mut out);
        e.step(&mut st, 200.0, &mut out);
        assert!((out.0[0] - 100.0).abs() < 1e-9);
        assert!((out.0[1] - 150.0).abs() < 1e-9);

        let d = SequentialDiffScan::<f64>::new();
        let mut st2 = d.init();
        let mut o2 = VecEmitter::new();
        d.step(&mut st2, 10.0, &mut o2);
        d.step(&mut st2, 13.0, &mut o2);
        d.step(&mut st2, 11.0, &mut o2);
        assert_eq!(o2.0, vec![3.0, -2.0]);
    }

    #[test]
    fn sequential_diff_i64() {
        let d = SequentialDiffScan::<i64>::new();
        let mut st = d.init();
        let mut o = VecEmitter::new();
        d.step(&mut st, 100, &mut o);
        d.step(&mut st, 107, &mut o);
        assert_eq!(o.0, vec![7]);
    }
}
