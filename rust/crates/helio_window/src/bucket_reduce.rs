//! Generic, event-time bucket reduction with explicit ordering and watermark semantics.
//!
//! Unlike [`crate::TimeBucketAggregatorScan`], this scan does not bake in price or mean semantics.
//! A [`BucketReducer`] supplies the bucket-local state transition, while [`BucketTimed`] supplies
//! event time. Inputs must be nondecreasing in event time. Put a reorder/watermark stage ahead of
//! this scan when a source can arrive out of order.

use std::marker::PhantomData;

use helio_scan::{
    Emit, FallibleRestoreScan, FlushReason, FlushableScan, Scan, SnapshottingScan,
    VersionedSnapshot,
};
use helio_stats::{OnlineMoments, StatsError};
use helio_time::WallBucketGrid;
use serde::{Deserialize, Serialize};

use crate::TimeBucketEvent;

/// Extract an event-time coordinate in the same unit as `G`.
pub trait BucketTimed<G: WallBucketGrid> {
    fn bucket_time(&self, grid: &G) -> G::T;
}

/// Existing [`TimeBucketEvent`] implementations automatically work with the generic reducer scan.
impl<G, V> BucketTimed<G> for V
where
    G: WallBucketGrid,
    V: TimeBucketEvent<G>,
{
    fn bucket_time(&self, grid: &G) -> G::T {
        TimeBucketEvent::bucket_time(self, grid)
    }
}

/// Bucket-local reduction strategy.
///
/// `push` errors must leave `state` unchanged. Configuration belongs in the reducer value; mutable
/// and checkpointed data belongs in `State`.
pub trait BucketReducer<V> {
    type State;
    type Summary;
    type Error;

    fn init(&self) -> Self::State;
    fn push(&self, state: &mut Self::State, value: &V) -> Result<(), Self::Error>;
    fn finish(&self, state: &Self::State) -> Option<Self::Summary>;

    /// Validate reducer-owned state loaded from an external checkpoint.
    ///
    /// Reducers with no additional invariants may keep the default. Stateful numerical reducers
    /// should reject non-finite or otherwise impossible values here.
    fn validate_state(&self, _state: &Self::State) -> Result<(), Self::Error> {
        Ok(())
    }
}

/// Projection-backed stable moments reducer. The projection is compile-time DI on the hot path.
#[derive(Debug, Clone, Copy)]
pub struct F64MomentsReducer<F> {
    project: F,
}

impl<F> F64MomentsReducer<F> {
    pub const fn new(project: F) -> Self {
        Self { project }
    }
}

impl<V, F> BucketReducer<V> for F64MomentsReducer<F>
where
    F: Fn(&V) -> f64,
{
    type State = OnlineMoments;
    type Summary = OnlineMoments;
    type Error = StatsError;

    fn init(&self) -> Self::State {
        OnlineMoments::new()
    }

    fn push(&self, state: &mut Self::State, value: &V) -> Result<(), Self::Error> {
        state.try_push((self.project)(value))
    }

    fn finish(&self, state: &Self::State) -> Option<Self::Summary> {
        (!state.is_empty()).then_some(*state)
    }

    fn validate_state(&self, state: &Self::State) -> Result<(), Self::Error> {
        state.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReducedBucket<T, S> {
    pub bucket_start: T,
    pub bucket_end: T,
    pub summary: S,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BucketRejection<T, E> {
    InvalidGrid,
    AtOrBeforeWatermark { event_time: T, watermark: T },
    RegressedEventTime { event_time: T, previous: T },
    Reducer(E),
    RegressedWatermark { attempted: T, current: T },
}

/// A bucket close or an explicit rejected input/control transition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BucketReduceOutput<T, S, E> {
    Closed(ReducedBucket<T, S>),
    Rejected(BucketRejection<T, E>),
}

#[derive(Debug, Clone)]
pub struct BucketReduceScan<G, V, R> {
    pub grid: G,
    pub reducer: R,
    _input: PhantomData<V>,
}

impl<G, V, R> BucketReduceScan<G, V, R> {
    pub const fn new(grid: G, reducer: R) -> Self {
        Self {
            grid,
            reducer,
            _input: PhantomData,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BucketReduceState<T, RS> {
    pub open_bucket_start: Option<T>,
    pub reducer_state: RS,
    pub previous_event_time: Option<T>,
    pub watermark: Option<T>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BucketReduceSnapshot<T, RS> {
    pub open_bucket_start: Option<T>,
    pub reducer_state: RS,
    pub previous_event_time: Option<T>,
    pub watermark: Option<T>,
}

/// Structural or reducer-owned error in a bucket snapshot loaded from external storage.
#[derive(Debug, Clone, PartialEq)]
pub enum BucketRestoreError<T, E> {
    InvalidGrid,
    MisalignedOpenBucket {
        stored: T,
        expected: T,
    },
    MissingPreviousEventTime {
        open_bucket_start: T,
    },
    PreviousEventOutsideOpenBucket {
        previous: T,
        bucket_start: T,
        bucket_end: T,
    },
    OpenBucketAtOrBeforeWatermark {
        bucket_end: T,
        watermark: T,
    },
    PreviousEventAtOrBeforeWatermark {
        previous: T,
        watermark: T,
    },
    Reducer(E),
}

impl<G, V, R> BucketReduceScan<G, V, R>
where
    G: WallBucketGrid,
    V: BucketTimed<G>,
    R: BucketReducer<V>,
{
    fn reject<E>(&self, reason: BucketRejection<G::T, R::Error>, emit: &mut E)
    where
        E: Emit<BucketReduceOutput<G::T, R::Summary, R::Error>>,
    {
        emit.emit(BucketReduceOutput::Rejected(reason));
    }

    fn close_open<E>(&self, state: &mut BucketReduceState<G::T, R::State>, emit: &mut E)
    where
        E: Emit<BucketReduceOutput<G::T, R::Summary, R::Error>>,
    {
        let Some(start) = state.open_bucket_start.take() else {
            return;
        };
        if let Some(summary) = self.reducer.finish(&state.reducer_state) {
            emit.emit(BucketReduceOutput::Closed(ReducedBucket {
                bucket_start: start,
                bucket_end: self.grid.bucket_end_exclusive(start),
                summary,
            }));
        }
        state.reducer_state = self.reducer.init();
    }

    fn start_bucket<E>(
        &self,
        state: &mut BucketReduceState<G::T, R::State>,
        bucket_start: G::T,
        event_time: G::T,
        input: &V,
        emit: &mut E,
    ) where
        E: Emit<BucketReduceOutput<G::T, R::Summary, R::Error>>,
    {
        let mut next = self.reducer.init();
        match self.reducer.push(&mut next, input) {
            Ok(()) => {
                state.open_bucket_start = Some(bucket_start);
                state.reducer_state = next;
                state.previous_event_time = Some(event_time);
            }
            Err(error) => self.reject(BucketRejection::Reducer(error), emit),
        }
    }
}

impl<G, V, R> Scan for BucketReduceScan<G, V, R>
where
    G: WallBucketGrid,
    V: BucketTimed<G>,
    R: BucketReducer<V>,
{
    type In = V;
    type Out = BucketReduceOutput<G::T, R::Summary, R::Error>;
    type State = BucketReduceState<G::T, R::State>;

    fn init(&self) -> Self::State {
        BucketReduceState {
            open_bucket_start: None,
            reducer_state: self.reducer.init(),
            previous_event_time: None,
            watermark: None,
        }
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        if !self.grid.is_valid() {
            self.reject(BucketRejection::InvalidGrid, emit);
            return;
        }

        let event_time = input.bucket_time(&self.grid);
        if let Some(watermark) = state.watermark {
            if event_time <= watermark {
                self.reject(
                    BucketRejection::AtOrBeforeWatermark {
                        event_time,
                        watermark,
                    },
                    emit,
                );
                return;
            }
        }
        if let Some(previous) = state.previous_event_time {
            if event_time < previous {
                self.reject(
                    BucketRejection::RegressedEventTime {
                        event_time,
                        previous,
                    },
                    emit,
                );
                return;
            }
        }

        let bucket_start = self.grid.bucket_start(event_time);
        match state.open_bucket_start {
            None => self.start_bucket(state, bucket_start, event_time, &input, emit),
            Some(open) if open == bucket_start => {
                match self.reducer.push(&mut state.reducer_state, &input) {
                    Ok(()) => state.previous_event_time = Some(event_time),
                    Err(error) => self.reject(BucketRejection::Reducer(error), emit),
                }
            }
            Some(open) if bucket_start > open => {
                self.close_open(state, emit);
                self.start_bucket(state, bucket_start, event_time, &input, emit);
            }
            Some(_) => {
                let previous = state.previous_event_time.unwrap_or(event_time);
                self.reject(
                    BucketRejection::RegressedEventTime {
                        event_time,
                        previous,
                    },
                    emit,
                );
            }
        }
    }
}

impl<G, V, R> FlushableScan for BucketReduceScan<G, V, R>
where
    G: WallBucketGrid,
    V: BucketTimed<G>,
    R: BucketReducer<V>,
{
    type Offset = G::T;

    fn flush<E>(&self, state: &mut Self::State, signal: FlushReason<Self::Offset>, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        match signal {
            FlushReason::Watermark(next) => {
                if let Some(current) = state.watermark {
                    if next < current {
                        self.reject(
                            BucketRejection::RegressedWatermark {
                                attempted: next,
                                current,
                            },
                            emit,
                        );
                        return;
                    }
                }
                state.watermark = Some(next);
                if state
                    .open_bucket_start
                    .is_some_and(|start| self.grid.bucket_end_exclusive(start) <= next)
                {
                    self.close_open(state, emit);
                }
            }
            FlushReason::SessionClose(_)
            | FlushReason::Shutdown
            | FlushReason::EndOfInput
            | FlushReason::Manual => self.close_open(state, emit),
            FlushReason::Checkpoint(_) | FlushReason::Rebalance => {}
        }
    }
}

impl<G, V, R> SnapshottingScan for BucketReduceScan<G, V, R>
where
    G: WallBucketGrid,
    V: BucketTimed<G>,
    R: BucketReducer<V>,
    R::State: Clone + Serialize + for<'de> Deserialize<'de>,
{
    type Snapshot = BucketReduceSnapshot<G::T, R::State>;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        BucketReduceSnapshot {
            open_bucket_start: state.open_bucket_start,
            reducer_state: state.reducer_state.clone(),
            previous_event_time: state.previous_event_time,
            watermark: state.watermark,
        }
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        BucketReduceState {
            open_bucket_start: snapshot.open_bucket_start,
            reducer_state: snapshot.reducer_state,
            previous_event_time: snapshot.previous_event_time,
            watermark: snapshot.watermark,
        }
    }
}

impl<G, V, R> FallibleRestoreScan for BucketReduceScan<G, V, R>
where
    G: WallBucketGrid,
    V: BucketTimed<G>,
    R: BucketReducer<V>,
    R::State: Clone + Serialize + for<'de> Deserialize<'de>,
{
    type RestoreError = BucketRestoreError<G::T, R::Error>;

    fn try_restore(&self, snapshot: Self::Snapshot) -> Result<Self::State, Self::RestoreError> {
        if !self.grid.is_valid() {
            return Err(BucketRestoreError::InvalidGrid);
        }
        if let Some(start) = snapshot.open_bucket_start {
            let expected = self.grid.bucket_start(start);
            if start != expected {
                return Err(BucketRestoreError::MisalignedOpenBucket {
                    stored: start,
                    expected,
                });
            }
            let Some(previous) = snapshot.previous_event_time else {
                return Err(BucketRestoreError::MissingPreviousEventTime {
                    open_bucket_start: start,
                });
            };
            let end = self.grid.bucket_end_exclusive(start);
            if previous < start || previous >= end {
                return Err(BucketRestoreError::PreviousEventOutsideOpenBucket {
                    previous,
                    bucket_start: start,
                    bucket_end: end,
                });
            }
            if let Some(watermark) = snapshot.watermark {
                if end <= watermark {
                    return Err(BucketRestoreError::OpenBucketAtOrBeforeWatermark {
                        bucket_end: end,
                        watermark,
                    });
                }
                if previous <= watermark {
                    return Err(BucketRestoreError::PreviousEventAtOrBeforeWatermark {
                        previous,
                        watermark,
                    });
                }
            }
        }
        self.reducer
            .validate_state(&snapshot.reducer_state)
            .map_err(BucketRestoreError::Reducer)?;
        Ok(self.restore(snapshot))
    }
}

impl<T, RS> VersionedSnapshot for BucketReduceSnapshot<T, RS> {
    const VERSION: u32 = 1;
}

#[cfg(test)]
mod tests {
    use super::*;
    use helio_scan::{
        FallibleRestoreScan, FlushReason, FlushableScan, Scan, SnapshottingScan, VecEmitter,
    };
    use helio_time::SecondWallBucket;

    #[derive(Debug, Clone, Copy)]
    struct Tick {
        at: i64,
        value: f64,
    }

    impl BucketTimed<SecondWallBucket> for Tick {
        fn bucket_time(&self, _grid: &SecondWallBucket) -> i64 {
            self.at
        }
    }

    type MomentsOutput = BucketReduceOutput<i64, OnlineMoments, StatsError>;
    type MomentsScan =
        BucketReduceScan<SecondWallBucket, Tick, F64MomentsReducer<fn(&Tick) -> f64>>;

    fn scan() -> MomentsScan {
        BucketReduceScan::new(
            SecondWallBucket { width_sec: 10 },
            F64MomentsReducer::new(|tick: &Tick| tick.value),
        )
    }

    fn closed(output: &MomentsOutput) -> Option<&ReducedBucket<i64, OnlineMoments>> {
        match output {
            BucketReduceOutput::Closed(bucket) => Some(bucket),
            BucketReduceOutput::Rejected(_) => None,
        }
    }

    #[test]
    fn moments_reducer_emits_mean_and_variance() {
        let scan = scan();
        let mut state = scan.init();
        let mut emit = VecEmitter::new();
        scan.step(&mut state, Tick { at: 1, value: 1.0 }, &mut emit);
        scan.step(&mut state, Tick { at: 8, value: 3.0 }, &mut emit);
        scan.step(&mut state, Tick { at: 12, value: 9.0 }, &mut emit);

        let bucket = closed(&emit.0[0]).unwrap();
        assert_eq!((bucket.bucket_start, bucket.bucket_end), (0, 10));
        assert_eq!(bucket.summary.mean(), Some(2.0));
        assert_eq!(bucket.summary.sample_variance(), Some(2.0));
    }

    #[test]
    fn regressed_input_is_rejected_without_corrupting_bucket() {
        let scan = scan();
        let mut state = scan.init();
        let mut emit = VecEmitter::new();
        scan.step(&mut state, Tick { at: 5, value: 5.0 }, &mut emit);
        let before = scan.snapshot(&state);
        scan.step(
            &mut state,
            Tick {
                at: 4,
                value: 100.0,
            },
            &mut emit,
        );
        assert!(matches!(
            emit.0.last(),
            Some(BucketReduceOutput::Rejected(
                BucketRejection::RegressedEventTime { .. }
            ))
        ));
        assert_eq!(scan.snapshot(&state), before);
    }

    #[test]
    fn watermark_finalizes_and_rejects_late_data() {
        let scan = scan();
        let mut state = scan.init();
        let mut emit = VecEmitter::new();
        scan.step(&mut state, Tick { at: 5, value: 2.0 }, &mut emit);
        scan.flush(&mut state, FlushReason::Watermark(10), &mut emit);
        assert!(matches!(emit.0[0], BucketReduceOutput::Closed(_)));

        scan.step(&mut state, Tick { at: 9, value: 3.0 }, &mut emit);
        assert!(matches!(
            emit.0[1],
            BucketReduceOutput::Rejected(BucketRejection::AtOrBeforeWatermark { .. })
        ));
    }

    #[test]
    fn checkpoint_does_not_finalize_partial_bucket_and_restore_is_exact() {
        let scan = scan();
        let mut state = scan.init();
        let mut before = VecEmitter::new();
        scan.step(&mut state, Tick { at: 1, value: 2.0 }, &mut before);
        scan.flush(&mut state, FlushReason::Checkpoint(1), &mut before);
        assert!(before.0.is_empty());

        let snapshot = scan.snapshot(&state);
        let mut restored = scan.restore(snapshot);
        let mut resumed = VecEmitter::new();
        scan.step(&mut restored, Tick { at: 3, value: 4.0 }, &mut resumed);
        scan.flush(&mut restored, FlushReason::EndOfInput, &mut resumed);
        let bucket = closed(&resumed.0[0]).unwrap();
        assert_eq!(bucket.summary.count(), 2);
        assert_eq!(bucket.summary.mean(), Some(3.0));
    }

    #[test]
    fn non_finite_projection_is_an_explicit_rejection() {
        let scan = scan();
        let mut state = scan.init();
        let mut emit = VecEmitter::new();
        scan.step(
            &mut state,
            Tick {
                at: 1,
                value: f64::NAN,
            },
            &mut emit,
        );
        assert!(matches!(
            emit.0[0],
            BucketReduceOutput::Rejected(BucketRejection::Reducer(StatsError::NonFiniteInput))
        ));
        assert!(state.open_bucket_start.is_none());
    }

    #[test]
    fn fallible_restore_rejects_corrupt_reducer_state() {
        let scan = scan();
        let invalid_moments: OnlineMoments =
            serde_json::from_str(r#"{"count":1,"mean":0.0,"m2":-1.0}"#).unwrap();
        let snapshot = BucketReduceSnapshot {
            open_bucket_start: Some(0),
            reducer_state: invalid_moments,
            previous_event_time: Some(1),
            watermark: None,
        };
        assert_eq!(
            scan.try_restore(snapshot),
            Err(BucketRestoreError::Reducer(StatsError::InvalidSnapshot))
        );
    }

    #[test]
    fn fallible_restore_rejects_closed_bucket_behind_watermark() {
        let scan = scan();
        let mut moments = OnlineMoments::new();
        moments.try_push(1.0).unwrap();
        let snapshot = BucketReduceSnapshot {
            open_bucket_start: Some(0),
            reducer_state: moments,
            previous_event_time: Some(1),
            watermark: Some(10),
        };
        assert_eq!(
            scan.try_restore(snapshot),
            Err(BucketRestoreError::OpenBucketAtOrBeforeWatermark {
                bucket_end: 10,
                watermark: 10,
            })
        );
    }
}
