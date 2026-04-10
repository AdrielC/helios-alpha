//! Composable **tick → time bucket → smooth → change** pipelines on [`Scan`](helio_scan::Scan).
//!
//! - **[`TimeBucketAggregatorScan`]** — fixed wall-clock buckets (e.g. 10 minutes in nanoseconds);
//!   emits one [`BucketBarClose`] **when the first tick of a new bucket arrives** (previous bucket
//!   “saturated” by time boundary). Ticks in an empty stream’s first bucket accumulate until rollover.
//! - **[`EmaScan`]** — exponential moving average over scalar inputs (e.g. bucket mean); emits every step.
//! - **[`SequentialDiffScan`]** — `out[k] = in[k] - in[k-1]`; **no emit** on the first value.
//!
//! Typical composition (same logical stream, sequential `Then`):
//! `ticks → TimeBucketAggregator → Map(mean) → EmaScan → SequentialDiffScan`.

use helio_scan::{
    Emit, FlushReason, FlushableScan, Scan, SnapshottingScan, VersionedSnapshot,
};
use serde::{Deserialize, Serialize};

/// One trade or quote tick in **nanoseconds since Unix epoch** (same unit as `i64` wall times).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PriceTick {
    pub t_ns: i64,
    pub price: f64,
}

/// Completed bucket summary (mean price over all ticks whose `t_ns` fell in the half-open bucket).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BucketBarClose {
    pub bucket_start_ns: i64,
    pub bucket_end_ns: i64,
    pub mean_price: f64,
    pub tick_count: u64,
}

impl BucketBarClose {
    pub fn mean_price(&self) -> f64 {
        self.mean_price
    }
}

#[inline]
fn floor_bucket_start(t_ns: i64, bucket_dur_ns: i64) -> i64 {
    if bucket_dur_ns <= 0 {
        return 0;
    }
    t_ns.div_euclid(bucket_dur_ns) * bucket_dur_ns
}

/// Aggregate ticks into fixed-duration wall-clock buckets; emit when the bucket **changes**
/// (first tick of the next bucket closes the previous one).
#[derive(Debug, Clone, Copy)]
pub struct TimeBucketAggregatorScan {
    /// Bucket width in nanoseconds (e.g. `10 * 60 * 1_000_000_000` for 10 minutes).
    pub bucket_dur_ns: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimeBucketAggregatorState {
    /// Start of the bucket currently accumulating (`None` before first tick).
    pub open_bucket_start: Option<i64>,
    pub sum_price: f64,
    pub tick_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeBucketAggregatorSnapshot {
    pub open_bucket_start: Option<i64>,
    pub sum_price_bits: u64,
    pub tick_count: u64,
}

fn f64_to_bits(x: f64) -> u64 {
    x.to_bits()
}

fn bits_to_f64(b: u64) -> f64 {
    f64::from_bits(b)
}

impl TimeBucketAggregatorScan {
    pub fn new(bucket_dur_ns: i64) -> Self {
        Self { bucket_dur_ns }
    }

    /// Convenience: 10-minute buckets.
    pub fn ten_minute_buckets() -> Self {
        Self {
            bucket_dur_ns: 10 * 60 * 1_000_000_000,
        }
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
        let mean = state.sum_price / state.tick_count as f64;
        emit.emit(BucketBarClose {
            bucket_start_ns: bucket_start,
            bucket_end_ns: bucket_start.saturating_add(self.bucket_dur_ns),
            mean_price: mean,
            tick_count: state.tick_count,
        });
        state.sum_price = 0.0;
        state.tick_count = 0;
    }
}

impl Scan for TimeBucketAggregatorScan {
    type In = PriceTick;
    type Out = BucketBarClose;
    type State = TimeBucketAggregatorState;

    fn init(&self) -> Self::State {
        TimeBucketAggregatorState {
            open_bucket_start: None,
            sum_price: 0.0,
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
        let b = floor_bucket_start(input.t_ns, self.bucket_dur_ns);
        match state.open_bucket_start {
            None => {
                state.open_bucket_start = Some(b);
                state.sum_price = input.price;
                state.tick_count = 1;
            }
            Some(cur) if cur == b => {
                state.sum_price += input.price;
                state.tick_count += 1;
            }
            Some(cur) => {
                self.flush_open_bucket(state, cur, emit);
                state.open_bucket_start = Some(b);
                state.sum_price = input.price;
                state.tick_count = 1;
            }
        }
    }
}

impl FlushableScan for TimeBucketAggregatorScan {
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

impl SnapshottingScan for TimeBucketAggregatorScan {
    type Snapshot = TimeBucketAggregatorSnapshot;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        TimeBucketAggregatorSnapshot {
            open_bucket_start: state.open_bucket_start,
            sum_price_bits: f64_to_bits(state.sum_price),
            tick_count: state.tick_count,
        }
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        TimeBucketAggregatorState {
            open_bucket_start: snapshot.open_bucket_start,
            sum_price: bits_to_f64(snapshot.sum_price_bits),
            tick_count: snapshot.tick_count,
        }
    }
}

impl VersionedSnapshot for TimeBucketAggregatorSnapshot {
    const VERSION: u32 = 1;
}

/// Exponential moving average: `ema = alpha * x + (1 - alpha) * ema_prev`; first sample passes through.
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

/// Emits `current - previous` for each value after the first (first value updates state only).
#[derive(Debug, Clone, Copy, Default)]
pub struct SequentialDiffScan;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SequentialDiffState {
    pub prev: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SequentialDiffSnapshot {
    pub prev_bits: Option<u64>,
}

impl Scan for SequentialDiffScan {
    type In = f64;
    type Out = f64;
    type State = SequentialDiffState;

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

impl FlushableScan for SequentialDiffScan {
    type Offset = u64;

    fn flush<E>(&self, _state: &mut Self::State, _signal: FlushReason<Self::Offset>, _emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
    }
}

impl SnapshottingScan for SequentialDiffScan {
    type Snapshot = SequentialDiffSnapshot;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        SequentialDiffSnapshot {
            prev_bits: state.prev.map(f64_to_bits),
        }
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        SequentialDiffState {
            prev: snapshot.prev_bits.map(bits_to_f64),
        }
    }
}

impl VersionedSnapshot for SequentialDiffSnapshot {
    const VERSION: u32 = 1;
}

#[cfg(test)]
mod tests {
    use super::*;
    use helio_scan::VecEmitter;

    const MIN: i64 = 60_000_000_000;
    const BUCKET: i64 = 10 * MIN;

    #[test]
    fn bucket_emits_on_rollover_mean_correct() {
        let s = TimeBucketAggregatorScan::new(BUCKET);
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
        assert!((b.mean_price - 105.0).abs() < 1e-9);
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

        let d = SequentialDiffScan;
        let mut st2 = d.init();
        let mut o2 = VecEmitter::new();
        d.step(&mut st2, 10.0, &mut o2);
        d.step(&mut st2, 13.0, &mut o2);
        d.step(&mut st2, 11.0, &mut o2);
        assert_eq!(o2.0, vec![3.0, -2.0]);
    }
}
