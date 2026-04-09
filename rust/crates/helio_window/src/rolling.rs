use std::collections::VecDeque;
use std::marker::PhantomData;

use helio_scan::{
    BatchOptimizedScan, Emit, FlushReason, FlushableScan, Scan, SnapshottingScan, VersionedSnapshot,
};
use helio_time::WindowSpec;
use serde::{Deserialize, Serialize};

use crate::agg::{EvictingWindowAggregator, SumCountMeanAggregator};
use crate::window_state::{FoldWindowState, WindowState};

/// Fixed-size FIFO window; emits **full snapshots** (oldest→newest) when length reaches `max_len`.
#[derive(Debug, Clone)]
pub struct RollingWindowScan<T> {
    pub max_len: usize,
    _p: PhantomData<T>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RollingWindowState<T> {
    buf: VecDeque<T>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RollingWindowSnapshot<T> {
    pub buf: Vec<T>,
}

impl<T: Clone> RollingWindowScan<T> {
    pub fn new(max_len: usize) -> Self {
        Self {
            max_len,
            _p: PhantomData,
        }
    }
}

impl<T: Clone> BatchOptimizedScan for RollingWindowScan<T> {
    /// Same as sequential [`Scan::step`] (opaque batching). Window fills can emit mid-batch in order.
    fn step_batch_optimized<E>(&self, state: &mut Self::State, batch: &[Self::In], emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        for item in batch {
            self.step(state, item.clone(), emit);
        }
    }
}

impl<T: Clone> Scan for RollingWindowScan<T> {
    type In = T;
    type Out = Vec<T>;
    type State = RollingWindowState<T>;

    fn init(&self) -> Self::State {
        RollingWindowState {
            buf: VecDeque::with_capacity(self.max_len),
        }
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        if self.max_len == 0 {
            return;
        }
        state.buf.push_back(input);
        while state.buf.len() > self.max_len {
            state.buf.pop_front();
        }
        if state.buf.len() == self.max_len {
            emit.emit(state.buf.iter().cloned().collect());
        }
    }
}

impl<T: Clone> FlushableScan for RollingWindowScan<T> {
    type Offset = u64;

    fn flush<E>(&self, _state: &mut Self::State, _signal: FlushReason<Self::Offset>, _emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
    }
}

impl<T: Clone + Serialize + for<'de> Deserialize<'de>> SnapshottingScan for RollingWindowScan<T> {
    type Snapshot = RollingWindowSnapshot<T>;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        RollingWindowSnapshot {
            buf: state.buf.iter().cloned().collect(),
        }
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        RollingWindowState {
            buf: snapshot.buf.into_iter().collect(),
        }
    }
}

impl<T> VersionedSnapshot for RollingWindowSnapshot<T> {
    const VERSION: u32 = 1;
}

// --- Spec-aware rolling with aggregation (sample-count `WindowSpec` only) ---

/// Rolling window using [`WindowState`] + an [`EvictingWindowAggregator`]. Emits a summary whenever
/// the buffer is **full** (per [`WindowSpec::sample_capacity`]).
///
/// **Sample-count only:** if [`WindowSpec::sample_capacity`] is `None`, the scan is a no-op (`inner` is `None`).
#[derive(Debug, Clone)]
pub struct RollingAggregatorScan<T, A> {
    pub spec: WindowSpec,
    _p: PhantomData<(T, A)>,
}

#[derive(Debug, Clone)]
pub struct RollingAggregatorState<T, A> {
    pub inner: Option<WindowState<T, A>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RollingAggregatorSnapshot<T, AS> {
    pub buf: Vec<T>,
    pub agg_summary: AS,
}

impl<
        T: Clone + Serialize + for<'de> Deserialize<'de>,
        A: EvictingWindowAggregator<T> + Default,
    > RollingAggregatorScan<T, A>
{
    pub fn new(spec: WindowSpec) -> Self {
        Self {
            spec,
            _p: PhantomData,
        }
    }
}

impl<
        T: Clone + Serialize + for<'de> Deserialize<'de>,
        A: EvictingWindowAggregator<T> + Default,
    > Scan for RollingAggregatorScan<T, A>
where
    A::Summary: Clone + Serialize + for<'de> Deserialize<'de>,
{
    type In = T;
    type Out = A::Summary;
    type State = RollingAggregatorState<T, A>;

    fn init(&self) -> Self::State {
        RollingAggregatorState {
            inner: WindowState::new(self.spec, A::default()),
        }
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        let Some(w) = state.inner.as_mut() else {
            return;
        };
        w.push(input);
        let cap = self.spec.sample_capacity().unwrap_or(0);
        if cap > 0 && w.buffer().len() == cap {
            emit.emit(w.summary());
        }
    }
}

impl<
        T: Clone + Serialize + for<'de> Deserialize<'de>,
        A: EvictingWindowAggregator<T> + Default,
    > FlushableScan for RollingAggregatorScan<T, A>
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
    > SnapshottingScan for RollingAggregatorScan<T, A>
where
    A::Summary: Clone + Serialize + for<'de> Deserialize<'de>,
{
    type Snapshot = RollingAggregatorSnapshot<T, A::Summary>;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        let buf = state
            .inner
            .as_ref()
            .map(|w| w.buffer().to_vec())
            .unwrap_or_default();
        let agg_summary = state
            .inner
            .as_ref()
            .map(|w| w.summary())
            .unwrap_or_else(|| A::default().snapshot());
        RollingAggregatorSnapshot { buf, agg_summary }
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        let mut inner = WindowState::new(self.spec, A::default());
        if let Some(ref mut w) = inner {
            for v in snapshot.buf {
                w.push(v);
            }
        }
        RollingAggregatorState { inner }
    }
}

impl<T: Serialize + for<'de> Deserialize<'de>, AS: Serialize + for<'de> Deserialize<'de>>
    VersionedSnapshot for RollingAggregatorSnapshot<T, AS>
{
    const VERSION: u32 = 1;
}

/// Trailing *n* samples, rolling sum/count/mean on `f64`.
#[inline]
pub fn rolling_mean_scan(n: u32) -> RollingAggregatorScan<f64, SumCountMeanAggregator> {
    RollingAggregatorScan::new(WindowSpec::trailing_samples(n))
}

/// Fold-on-snapshot rolling window (O(window) per emit).
#[derive(Debug, Clone)]
pub struct RollingFoldScan<T, S, F> {
    pub spec: WindowSpec,
    pub empty_summary: S,
    pub fold: F,
    _p: PhantomData<T>,
}

#[derive(Debug, Clone)]
pub struct RollingFoldState<T, S, F> {
    pub inner: Option<FoldWindowState<T, S, F>>,
}

impl<T: Clone, S: Clone, F: Clone + Fn(&[T]) -> S> RollingFoldScan<T, S, F> {
    pub fn new(spec: WindowSpec, empty_summary: S, fold: F) -> Self {
        Self {
            spec,
            empty_summary,
            fold,
            _p: PhantomData,
        }
    }
}

impl<T: Clone, S: Clone, F: Clone + Fn(&[T]) -> S> Scan for RollingFoldScan<T, S, F> {
    type In = T;
    type Out = S;
    type State = RollingFoldState<T, S, F>;

    fn init(&self) -> Self::State {
        RollingFoldState {
            inner: FoldWindowState::new(self.spec, self.empty_summary.clone(), self.fold.clone()),
        }
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        let Some(w) = state.inner.as_mut() else {
            return;
        };
        w.push(input);
        let cap = self.spec.sample_capacity().unwrap_or(0);
        if cap > 0 && w.len() == cap {
            emit.emit(w.summary());
        }
    }
}

impl<T: Clone, S: Clone, F: Clone + Fn(&[T]) -> S> FlushableScan for RollingFoldScan<T, S, F> {
    type Offset = u64;

    fn flush<E>(&self, _state: &mut Self::State, _signal: FlushReason<Self::Offset>, _emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use helio_scan::VecEmitter;

    #[test]
    fn rolling_mean_emits_when_full() {
        let s = rolling_mean_scan(3);
        let mut st = s.init();
        let mut e = VecEmitter::new();
        s.step(&mut st, 1.0, &mut e);
        s.step(&mut st, 2.0, &mut e);
        s.step(&mut st, 3.0, &mut e);
        assert_eq!(e.0.len(), 1);
        assert!((e.0[0].sum - 6.0).abs() < 1e-9);
        assert_eq!(e.0[0].count, 3);
        s.step(&mut st, 10.0, &mut e);
        assert_eq!(e.0.len(), 2);
        assert!((e.0[1].sum - 15.0).abs() < 1e-9);
    }

    #[test]
    fn rolling_fold_max() {
        let s = RollingFoldScan::new(WindowSpec::trailing_samples(2), 0i32, |xs: &[i32]| {
            *xs.iter().max().unwrap_or(&0)
        });
        let mut st = s.init();
        let mut e = VecEmitter::new();
        s.step(&mut st, 3, &mut e);
        s.step(&mut st, 7, &mut e);
        assert_eq!(e.0, vec![7]);
    }
}
