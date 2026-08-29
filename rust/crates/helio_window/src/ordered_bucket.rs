//! Bounded reorder → generic bucket reduction as one restartable pipeline.

use helio_scan::{
    Emit, FallibleRestoreScan, FlushReason, FlushableScan, Scan, SnapshottingScan,
    VersionedSnapshot,
};
use helio_time::WallBucketGrid;
use serde::{Deserialize, Serialize};

use crate::{
    BucketReduceOutput, BucketReduceScan, BucketReduceSnapshot, BucketReduceState, BucketReducer,
    BucketRestoreError, BucketTimed, EventTimeReorderScan, EventTimeReorderSnapshot,
    EventTimeReorderState, ReorderBuildError, ReorderOutput, ReorderRestoreError, WatermarkTime,
};

/// Output from the ingress ordering layer or the bucket layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum OrderedBucketOutput<T, S, E> {
    /// Late, overflowed, sequence-exhausted, or invalid-watermark ingress outcome.
    Ingress(ReorderOutput<T>),
    Bucket(BucketReduceOutput<i64, S, E>),
}

#[derive(Debug, Clone)]
pub struct OrderedBucketPipeline<G, T, R> {
    pub reorder: EventTimeReorderScan<T>,
    pub bucket: BucketReduceScan<G, T, R>,
}

impl<G, T, R> OrderedBucketPipeline<G, T, R> {
    pub fn try_new(max_pending: usize, grid: G, reducer: R) -> Result<Self, ReorderBuildError> {
        Ok(Self {
            reorder: EventTimeReorderScan::try_new(max_pending)?,
            bucket: BucketReduceScan::new(grid, reducer),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OrderedBucketState<T, RS> {
    pub reorder: EventTimeReorderState<T>,
    pub bucket: BucketReduceState<i64, RS>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrderedBucketSnapshot<T, RS> {
    pub reorder: EventTimeReorderSnapshot<T>,
    pub bucket: BucketReduceSnapshot<i64, RS>,
}

/// Validation failure from either stateful layer of an ordered bucket pipeline.
#[derive(Debug, Clone, PartialEq)]
pub enum OrderedBucketRestoreError<E> {
    Reorder(ReorderRestoreError),
    Bucket(BucketRestoreError<i64, E>),
}

struct BucketOutputBridge<'a, T, E> {
    sink: &'a mut E,
    _input: std::marker::PhantomData<T>,
}

impl<T, S, Error, E> Emit<BucketReduceOutput<i64, S, Error>> for BucketOutputBridge<'_, T, E>
where
    E: Emit<OrderedBucketOutput<T, S, Error>>,
{
    fn emit(&mut self, item: BucketReduceOutput<i64, S, Error>) {
        self.sink.emit(OrderedBucketOutput::Bucket(item));
    }
}

struct ReorderBridge<'a, G, T, R, E>
where
    R: BucketReducer<T>,
{
    bucket: &'a BucketReduceScan<G, T, R>,
    bucket_state: &'a mut BucketReduceState<i64, R::State>,
    sink: &'a mut E,
}

impl<G, T, R, E> Emit<ReorderOutput<T>> for ReorderBridge<'_, G, T, R, E>
where
    G: WallBucketGrid<T = i64>,
    T: BucketTimed<G>,
    R: BucketReducer<T>,
    E: Emit<OrderedBucketOutput<T, R::Summary, R::Error>>,
{
    fn emit(&mut self, item: ReorderOutput<T>) {
        match item {
            ReorderOutput::Ready(input) => {
                let mut bridge = BucketOutputBridge {
                    sink: self.sink,
                    _input: std::marker::PhantomData::<T>,
                };
                self.bucket.step(self.bucket_state, input, &mut bridge);
            }
            other => self.sink.emit(OrderedBucketOutput::Ingress(other)),
        }
    }
}

impl<G, T, R> Scan for OrderedBucketPipeline<G, T, R>
where
    G: WallBucketGrid<T = i64>,
    T: BucketTimed<G> + WatermarkTime,
    R: BucketReducer<T>,
{
    type In = T;
    type Out = OrderedBucketOutput<T, R::Summary, R::Error>;
    type State = OrderedBucketState<T, R::State>;

    fn init(&self) -> Self::State {
        OrderedBucketState {
            reorder: self.reorder.init(),
            bucket: self.bucket.init(),
        }
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        let mut bridge = ReorderBridge {
            bucket: &self.bucket,
            bucket_state: &mut state.bucket,
            sink: emit,
        };
        self.reorder.step(&mut state.reorder, input, &mut bridge);
    }
}

impl<G, T, R> FlushableScan for OrderedBucketPipeline<G, T, R>
where
    G: WallBucketGrid<T = i64>,
    T: BucketTimed<G> + WatermarkTime,
    R: BucketReducer<T>,
{
    type Offset = i64;

    fn flush<E>(&self, state: &mut Self::State, signal: FlushReason<Self::Offset>, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        {
            let mut bridge = ReorderBridge {
                bucket: &self.bucket,
                bucket_state: &mut state.bucket,
                sink: emit,
            };
            self.reorder
                .flush(&mut state.reorder, signal.clone(), &mut bridge);
        }
        let mut bridge = BucketOutputBridge {
            sink: emit,
            _input: std::marker::PhantomData::<T>,
        };
        self.bucket.flush(&mut state.bucket, signal, &mut bridge);
    }
}

impl<G, T, R> SnapshottingScan for OrderedBucketPipeline<G, T, R>
where
    G: WallBucketGrid<T = i64>,
    T: Clone + BucketTimed<G> + WatermarkTime + Serialize + for<'de> Deserialize<'de>,
    R: BucketReducer<T>,
    R::State: Clone + Serialize + for<'de> Deserialize<'de>,
{
    type Snapshot = OrderedBucketSnapshot<T, R::State>;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        OrderedBucketSnapshot {
            reorder: self.reorder.snapshot(&state.reorder),
            bucket: self.bucket.snapshot(&state.bucket),
        }
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        OrderedBucketState {
            reorder: self.reorder.restore(snapshot.reorder),
            bucket: self.bucket.restore(snapshot.bucket),
        }
    }
}

impl<G, T, R> FallibleRestoreScan for OrderedBucketPipeline<G, T, R>
where
    G: WallBucketGrid<T = i64>,
    T: Clone + BucketTimed<G> + WatermarkTime + Serialize + for<'de> Deserialize<'de>,
    R: BucketReducer<T>,
    R::State: Clone + Serialize + for<'de> Deserialize<'de>,
{
    type RestoreError = OrderedBucketRestoreError<R::Error>;

    fn try_restore(&self, snapshot: Self::Snapshot) -> Result<Self::State, Self::RestoreError> {
        let reorder = self
            .reorder
            .try_restore(snapshot.reorder)
            .map_err(OrderedBucketRestoreError::Reorder)?;
        let bucket = self
            .bucket
            .try_restore(snapshot.bucket)
            .map_err(OrderedBucketRestoreError::Bucket)?;
        Ok(OrderedBucketState { reorder, bucket })
    }
}

impl<T, RS> VersionedSnapshot for OrderedBucketSnapshot<T, RS> {
    const VERSION: u32 = 1;
}

#[cfg(test)]
mod tests {
    use helio_scan::{
        FallibleRestoreScan, FlushReason, FlushableScan, Scan, SnapshottingScan, VecEmitter,
    };
    use helio_stats::{OnlineMoments, StatsError};
    use helio_time::SecondWallBucket;

    use super::*;
    use crate::{BucketReduceOutput, F64MomentsReducer, ReducedBucket};

    #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
    struct Tick {
        at: i64,
        value: f64,
    }

    impl WatermarkTime for Tick {
        fn event_time(&self) -> i64 {
            self.at
        }
    }

    impl BucketTimed<SecondWallBucket> for Tick {
        fn bucket_time(&self, _grid: &SecondWallBucket) -> i64 {
            self.at
        }
    }

    type Pipeline =
        OrderedBucketPipeline<SecondWallBucket, Tick, F64MomentsReducer<fn(&Tick) -> f64>>;
    type Output = OrderedBucketOutput<Tick, OnlineMoments, StatsError>;

    fn pipeline() -> Pipeline {
        OrderedBucketPipeline::try_new(
            16,
            SecondWallBucket { width_sec: 10 },
            F64MomentsReducer::new((|tick: &Tick| tick.value) as fn(&Tick) -> f64),
        )
        .unwrap()
    }

    fn first_closed(outputs: &[Output]) -> &ReducedBucket<i64, OnlineMoments> {
        outputs
            .iter()
            .find_map(|output| match output {
                OrderedBucketOutput::Bucket(BucketReduceOutput::Closed(bucket)) => Some(bucket),
                _ => None,
            })
            .unwrap()
    }

    #[test]
    fn out_of_order_ingress_reduces_in_event_time_order() {
        let pipeline = pipeline();
        let mut state = pipeline.init();
        let mut emit = VecEmitter::new();
        pipeline.step(&mut state, Tick { at: 8, value: 3.0 }, &mut emit);
        pipeline.step(&mut state, Tick { at: 1, value: 1.0 }, &mut emit);
        pipeline.flush(&mut state, FlushReason::Watermark(10), &mut emit);

        let bucket = first_closed(&emit.0);
        assert_eq!(bucket.summary.count(), 2);
        assert_eq!(bucket.summary.mean(), Some(2.0));
    }

    #[test]
    fn checkpoint_resume_matches_uninterrupted() {
        let pipeline = pipeline();
        let inputs = [
            Tick { at: 8, value: 3.0 },
            Tick { at: 1, value: 1.0 },
            Tick {
                at: 12,
                value: 10.0,
            },
        ];

        let mut full_state = pipeline.init();
        let mut full_emit = VecEmitter::new();
        for input in inputs {
            pipeline.step(&mut full_state, input, &mut full_emit);
        }
        pipeline.flush(&mut full_state, FlushReason::EndOfInput, &mut full_emit);

        let mut resumed_state = pipeline.init();
        let mut resumed_emit = VecEmitter::new();
        pipeline.step(&mut resumed_state, inputs[0], &mut resumed_emit);
        pipeline.flush(
            &mut resumed_state,
            FlushReason::Checkpoint(1),
            &mut resumed_emit,
        );
        let mut resumed_state = pipeline.restore(pipeline.snapshot(&resumed_state));
        for input in &inputs[1..] {
            pipeline.step(&mut resumed_state, *input, &mut resumed_emit);
        }
        pipeline.flush(
            &mut resumed_state,
            FlushReason::EndOfInput,
            &mut resumed_emit,
        );
        assert_eq!(resumed_emit.0, full_emit.0);
    }

    #[test]
    fn fallible_restore_rejects_corrupt_bucket_state() {
        let pipeline = pipeline();
        let mut state = pipeline.init();
        let mut emit = VecEmitter::new();
        pipeline.step(&mut state, Tick { at: 1, value: 1.0 }, &mut emit);
        pipeline.flush(&mut state, FlushReason::Watermark(1), &mut emit);
        let mut snapshot = pipeline.snapshot(&state);
        snapshot.bucket.watermark = Some(10);
        assert_eq!(
            pipeline.try_restore(snapshot),
            Err(OrderedBucketRestoreError::Bucket(
                BucketRestoreError::OpenBucketAtOrBeforeWatermark {
                    bucket_end: 10,
                    watermark: 10,
                }
            ))
        );
    }
}
