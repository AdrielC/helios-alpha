//! Composable **tick → time bucket → smooth → change** pipelines on [`Scan`](helio_scan::Scan).
//!
//! ## Generic inputs
//!
//! - **[`TimeBucketAggregatorScan<T>`]** — `T` must implement [`TimeBucketSample`]: wall-clock
//!   `time_ns()` plus a scalar [`mean_sample`](TimeBucketSample::mean_sample) summed for the bar mean.
//!   Built-in: [`PriceTick`]. Define your own tick type and impl the trait.
//! - **[`SequentialDiffScan<T>`]** — `T: Copy + Sub<Output = T>` (e.g. `f64`, `i64`); first element
//!   seeds state only; emits `current - previous` thereafter. Use `SequentialDiffScan::<f64>::default()`.
//! - **[`EmaScan`]** — `f64` in/out today (smooth the scalar series after `Map` from your bar type).
//!
//! Typical composition:
//! `TimeBucketAggregatorScan::<MyTick>::new(ns) → Arr(mean) → EmaScan → SequentialDiffScan::<f64>`.

use std::marker::PhantomData;
use std::ops::Sub;

use helio_scan::{
    Emit, FlushReason, FlushableScan, Scan, SnapshottingScan, VersionedSnapshot,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

// --- Time bucket sample (generic input to bucket scan) ---

/// Anything that can be **bucketed by wall time** and contributes one **f64** per sample to the
/// bar’s arithmetic mean (`sum(mean_sample) / count`).
pub trait TimeBucketSample: Clone {
    fn time_ns(&self) -> i64;
    fn mean_sample(&self) -> f64;
}

/// Trade / quote tick: **nanoseconds since Unix epoch** + price.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PriceTick {
    pub t_ns: i64,
    pub price: f64,
}

impl TimeBucketSample for PriceTick {
    fn time_ns(&self) -> i64 {
        self.t_ns
    }

    fn mean_sample(&self) -> f64 {
        self.price
    }
}

/// Closed bucket: `[bucket_start_ns, bucket_end_ns)` half-open in time; **mean** is over
/// [`TimeBucketSample::mean_sample`] (for [`PriceTick`], mean price).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BucketBarClose {
    pub bucket_start_ns: i64,
    pub bucket_end_ns: i64,
    /// `sum(mean_sample) / tick_count` for samples in this bucket.
    pub mean: f64,
    pub tick_count: u64,
}

impl BucketBarClose {
    #[inline]
    pub fn mean_price(&self) -> f64 {
        self.mean
    }
}

#[inline]
fn floor_bucket_start(t_ns: i64, bucket_dur_ns: i64) -> i64 {
    if bucket_dur_ns <= 0 {
        return 0;
    }
    t_ns.div_euclid(bucket_dur_ns) * bucket_dur_ns
}

/// Aggregate **generic** time-stamped samples into fixed-duration wall-clock buckets; emit when the
/// bucket **changes** (first sample of the next bucket closes the previous one).
#[derive(Debug, Clone)]
pub struct TimeBucketAggregatorScan<T: TimeBucketSample> {
    pub bucket_dur_ns: i64,
    _p: PhantomData<T>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimeBucketAggregatorState {
    pub open_bucket_start: Option<i64>,
    pub sum: f64,
    pub tick_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeBucketAggregatorSnapshot {
    pub open_bucket_start: Option<i64>,
    pub sum_bits: u64,
    pub tick_count: u64,
}

fn f64_to_bits(x: f64) -> u64 {
    x.to_bits()
}

fn bits_to_f64(b: u64) -> f64 {
    f64::from_bits(b)
}

impl<T: TimeBucketSample> TimeBucketAggregatorScan<T> {
    pub fn new(bucket_dur_ns: i64) -> Self {
        Self {
            bucket_dur_ns,
            _p: PhantomData,
        }
    }

    pub fn ten_minute_buckets() -> Self {
        Self::new(10 * 60 * 1_000_000_000)
    }

    fn flush_open_bucket<E: Emit<BucketBarClose>>(
        &self,
        state: &mut TimeBucketAggregatorState,
        bucket_start: i64,
        emit: &mut E,
    ) {
        if state.tick_count == 0 {
            return;
        }
        let mean = state.sum / state.tick_count as f64;
        emit.emit(BucketBarClose {
            bucket_start_ns: bucket_start,
            bucket_end_ns: bucket_start.saturating_add(self.bucket_dur_ns),
            mean,
            tick_count: state.tick_count,
        });
        state.sum = 0.0;
        state.tick_count = 0;
    }
}

impl<T: TimeBucketSample> Scan for TimeBucketAggregatorScan<T> {
    type In = T;
    type Out = BucketBarClose;
    type State = TimeBucketAggregatorState;

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
        if self.bucket_dur_ns <= 0 {
            return;
        }
        let b = floor_bucket_start(input.time_ns(), self.bucket_dur_ns);
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

impl<T: TimeBucketSample> FlushableScan for TimeBucketAggregatorScan<T> {
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

impl<T: TimeBucketSample> SnapshottingScan for TimeBucketAggregatorScan<T> {
    type Snapshot = TimeBucketAggregatorSnapshot;

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

impl VersionedSnapshot for TimeBucketAggregatorSnapshot {
    const VERSION: u32 = 1;
}

/// Exponential moving average on `f64` (compose after mapping your bar to a scalar).
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

/// Sequential difference: `out = current - previous`; **no emit** until a second value arrives.
///
/// Type parameter `T` must support [`Sub`](std::ops::Sub) with output `T` (e.g. `f64`, `i64`).
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

/// Convenience alias for `f64` diffs.
pub type SequentialDiffF64 = SequentialDiffScan<f64>;

#[cfg(test)]
mod tests {
    use super::*;
    use helio_scan::VecEmitter;

    const MIN: i64 = 60_000_000_000;
    const BUCKET: i64 = 10 * MIN;

    /// Custom tick: volume-weighted style sample (mean of unsigned volume as f64).
    #[derive(Debug, Clone, Copy)]
    struct VolTick {
        t_ns: i64,
        vol: u32,
    }

    impl TimeBucketSample for VolTick {
        fn time_ns(&self) -> i64 {
            self.t_ns
        }

        fn mean_sample(&self) -> f64 {
            self.vol as f64
        }
    }

    #[test]
    fn bucket_generic_custom_tick() {
        let s = TimeBucketAggregatorScan::<VolTick>::new(BUCKET);
        let mut st = s.init();
        let mut e = VecEmitter::new();
        s.step(
            &mut st,
            VolTick {
                t_ns: 0,
                vol: 10,
            },
            &mut e,
        );
        s.step(
            &mut st,
            VolTick {
                t_ns: BUCKET,
                vol: 0,
            },
            &mut e,
        );
        assert_eq!(e.0.len(), 1);
        assert!((e.0[0].mean - 10.0).abs() < 1e-9);
    }

    #[test]
    fn bucket_emits_on_rollover_mean_correct() {
        let s = TimeBucketAggregatorScan::<PriceTick>::new(BUCKET);
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
                t_ns: 5 * MIN,
                price: 110.0,
            },
            &mut e,
        );
        assert!(e.0.is_empty());
        s.step(
            &mut st,
            PriceTick {
                t_ns: BUCKET,
                price: 50.0,
            },
            &mut e,
        );
        assert_eq!(e.0.len(), 1);
        let b = &e.0[0];
        assert!((b.mean - 105.0).abs() < 1e-9);
        assert_eq!(b.tick_count, 2);
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
