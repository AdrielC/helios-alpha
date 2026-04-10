//! Rolling aggregator driven by [`TimeKeyedWindowState`](crate::time_keyed::TimeKeyedWindowState).

use helio_scan::{
    BatchOptimizedScan, Emit, FlushReason, FlushableScan, Scan, SnapshottingScan, VersionedSnapshot,
};
use helio_time::WindowSpec;
use serde::{Deserialize, Serialize};

use crate::agg::{EvictingWindowAggregator, SumCountMeanAggregator};
use crate::time_keyed::{TimeKey, TimeKeyedWindowState};

/// Emits a rolling summary whenever the time-keyed buffer is **non-empty** after each input.
///
/// **Requires** `spec` = `WindowSpec::Trailing { size: Frequency::Fixed(..), .. }` with positive span;
/// otherwise `inner` is `None` and the scan is a no-op.
#[derive(Debug, Clone)]
pub struct TimeKeyedRollingAggregatorScan<T, A> {
    pub spec: WindowSpec,
    _p: std::marker::PhantomData<(T, A)>,
}

#[derive(Debug, Clone)]
pub struct TimeKeyedRollingAggregatorState<T, A> {
    pub inner: Option<TimeKeyedWindowState<T, A>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimeKeyedRollingAggregatorSnapshot<T, AS> {
    pub entries: Vec<(i64, T)>,
    pub agg_summary: AS,
}

impl<
        T: Clone + Serialize + for<'de> Deserialize<'de>,
        A: EvictingWindowAggregator<T> + Default,
    > TimeKeyedRollingAggregatorScan<T, A>
{
    pub fn new(spec: WindowSpec) -> Self {
        Self {
            spec,
            _p: std::marker::PhantomData,
        }
    }
}

impl<
        T: Clone + Serialize + for<'de> Deserialize<'de>,
        A: EvictingWindowAggregator<T> + Default,
    > BatchOptimizedScan for TimeKeyedRollingAggregatorScan<T, A>
where
    A::Summary: Clone + Serialize + for<'de> Deserialize<'de>,
{
    fn step_batch_optimized<E>(&self, state: &mut Self::State, batch: &[Self::In], emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        for item in batch {
            self.step(state, item.clone(), emit);
        }
    }
}

/// Input: wall-time key + payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimeKeyedSampleIn<T> {
    pub key_secs: i64,
    pub value: T,
}

impl<
        T: Clone + Serialize + for<'de> Deserialize<'de>,
        A: EvictingWindowAggregator<T> + Default,
    > Scan for TimeKeyedRollingAggregatorScan<T, A>
where
    A::Summary: Clone + Serialize + for<'de> Deserialize<'de>,
{
    type In = TimeKeyedSampleIn<T>;
    type Out = A::Summary;
    type State = TimeKeyedRollingAggregatorState<T, A>;

    fn init(&self) -> Self::State {
        TimeKeyedRollingAggregatorState {
            inner: TimeKeyedWindowState::new(self.spec, A::default()),
        }
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        let Some(w) = state.inner.as_mut() else {
            return;
        };
        w.push(TimeKey(input.key_secs), input.value);
        if w.len() > 0 {
            emit.emit(w.summary());
        }
    }
}

impl<
        T: Clone + Serialize + for<'de> Deserialize<'de>,
        A: EvictingWindowAggregator<T> + Default,
    > FlushableScan for TimeKeyedRollingAggregatorScan<T, A>
where
    A::Summary: Clone + Serialize + for<'de> Deserialize<'de>,
{
    type Offset = u64;

    fn flush<E>(&self, _state: &mut Self::State, _signal: FlushReason<Self::Offset>, _emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
    }
}

impl<
        T: Clone + Serialize + for<'de> Deserialize<'de>,
        A: EvictingWindowAggregator<T> + Default,
    > SnapshottingScan for TimeKeyedRollingAggregatorScan<T, A>
where
    A::Summary: Clone + Serialize + for<'de> Deserialize<'de>,
{
    type Snapshot = TimeKeyedRollingAggregatorSnapshot<T, A::Summary>;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        let entries = state
            .inner
            .as_ref()
            .map(|w| w.entries())
            .unwrap_or_default();
        let agg_summary = state
            .inner
            .as_ref()
            .map(|w| w.summary())
            .unwrap_or_else(|| A::default().snapshot());
        TimeKeyedRollingAggregatorSnapshot {
            entries,
            agg_summary,
        }
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        let mut inner = TimeKeyedWindowState::new(self.spec, A::default());
        if let Some(ref mut w) = inner {
            for (k, v) in snapshot.entries {
                w.push(TimeKey(k), v);
            }
        }
        TimeKeyedRollingAggregatorState { inner }
    }
}

impl<T: Serialize + for<'de> Deserialize<'de>, AS: Serialize + for<'de> Deserialize<'de>>
    VersionedSnapshot for TimeKeyedRollingAggregatorSnapshot<T, AS>
{
    const VERSION: u32 = 1;
}

/// Trailing fixed-time window rolling mean on `f64` (wall seconds key).
pub fn time_keyed_rolling_mean_scan(
    spec: WindowSpec,
) -> TimeKeyedRollingAggregatorScan<f64, SumCountMeanAggregator> {
    TimeKeyedRollingAggregatorScan::new(spec)
}
