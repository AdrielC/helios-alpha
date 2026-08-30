//! Portable source protocol for deterministic replay, restart, and live handoff.
//!
//! The interface is deliberately synchronous and bounded. Infrastructure adapters may perform
//! asynchronous I/O outside this crate, but every poll returns a caller-sized batch and every
//! restart is expressed by an explicit [`SourceCheckpoint`]. This keeps the protocol usable in
//! native services, tests, WASI components, and durable Golem workers.

use std::collections::BTreeMap;
use std::num::NonZeroUsize;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SourceId(pub String);

impl SourceId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SourceOffset {
    pub partition: String,
    pub sequence: u64,
}

impl SourceOffset {
    pub fn new(partition: impl Into<String>, sequence: u64) -> Self {
        Self {
            partition: partition.into(),
            sequence,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourcePhase {
    Backfill,
    Live,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceCapabilities {
    pub backfill: bool,
    pub live: bool,
    pub resumable: bool,
    pub rewindable: bool,
}

impl SourceCapabilities {
    pub const REPLAY_AND_LIVE: Self = Self {
        backfill: true,
        live: true,
        resumable: true,
        rewindable: true,
    };
}

/// A source record carries three different clocks.
///
/// `event_time` belongs to the underlying phenomenon and may be in the future for forecasts.
/// `available_at` is the first instant Helios may use the payload. `observed_at` is when this
/// process received it and therefore may not precede `available_at`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceEnvelope<T> {
    pub source: SourceId,
    pub offset: SourceOffset,
    pub event_time: i64,
    pub available_at: i64,
    pub observed_at: i64,
    pub phase: SourcePhase,
    pub payload: T,
}

/// Last committed source position and availability frontier per partition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceCheckpoint {
    pub source: SourceId,
    pub positions: BTreeMap<String, u64>,
    pub available_at: BTreeMap<String, i64>,
}

impl SourceCheckpoint {
    pub fn empty(source: SourceId) -> Self {
        Self {
            source,
            positions: BTreeMap::new(),
            available_at: BTreeMap::new(),
        }
    }

    pub fn position(&self, partition: &str) -> Option<u64> {
        self.positions.get(partition).copied()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceRequest {
    /// Replay only information available at or before `as_of`.
    Backfill {
        checkpoint: SourceCheckpoint,
        from_available_at: Option<i64>,
        until_available_at: i64,
        as_of: i64,
    },
    /// Continue after the exact committed prefix.
    Live { checkpoint: SourceCheckpoint },
}

impl SourceRequest {
    pub fn checkpoint(&self) -> &SourceCheckpoint {
        match self {
            Self::Backfill { checkpoint, .. } | Self::Live { checkpoint } => checkpoint,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SourcePoll<T> {
    Records(Vec<SourceEnvelope<T>>),
    /// Backfill reached its fenced end and a live adapter may resume from this checkpoint.
    CaughtUp(SourceCheckpoint),
    /// No live record is currently available. This is not end-of-input.
    Idle(SourceCheckpoint),
    End(SourceCheckpoint),
    /// Emitted once by [`ReplayThenLive`] after the live adapter accepts the backfill checkpoint.
    Handoff(SourceCheckpoint),
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SourceError {
    #[error("source request targets {requested:?}, adapter owns {actual:?}")]
    WrongSource {
        requested: SourceId,
        actual: SourceId,
    },
    #[error("source does not support {0:?}")]
    UnsupportedPhase(SourcePhase),
    #[error("backfill until_available_at {until} exceeds causal as_of cut {as_of}")]
    InvalidCausalCut { until: i64, as_of: i64 },
    #[error("record from {actual:?} arrived on adapter {expected:?}")]
    ForeignRecord {
        expected: SourceId,
        actual: SourceId,
    },
    #[error("record observed_at {observed_at} precedes available_at {available_at}")]
    ObservedBeforeAvailable { observed_at: i64, available_at: i64 },
    #[error("record phase {actual:?} does not match requested phase {expected:?}")]
    WrongPhase {
        expected: SourcePhase,
        actual: SourcePhase,
    },
    #[error(
        "record available_at {available_at} is outside requested backfill range [{from:?}, {until}]"
    )]
    OutsideBackfillRange {
        available_at: i64,
        from: Option<i64>,
        until: i64,
    },
    #[error("record available_at {available_at} exceeds causal cut {as_of}")]
    FutureAvailability { available_at: i64, as_of: i64 },
    #[error("partition {partition} jumped from sequence {previous} to {actual}")]
    OffsetGap {
        partition: String,
        previous: u64,
        actual: u64,
    },
    #[error("partition {partition} availability moved backward from {previous} to {actual}")]
    AvailabilityRegression {
        partition: String,
        previous: i64,
        actual: i64,
    },
    #[error("adapter returned {actual} records, above requested maximum {requested}")]
    OversizedBatch { requested: usize, actual: usize },
    #[error("source adapter failure: {0}")]
    Adapter(String),
}

/// Master interface implemented by file, historical API, replay, and live feed adapters.
///
/// `start` must be idempotent for the same request. `poll` must never return more than
/// `max_records`, and a live adapter must resume strictly after the supplied checkpoint.
pub trait HelioSource {
    type Item;

    fn source_id(&self) -> &SourceId;
    fn capabilities(&self) -> SourceCapabilities;
    fn start(&mut self, request: SourceRequest) -> Result<(), SourceError>;
    fn poll(&mut self, max_records: NonZeroUsize) -> Result<SourcePoll<Self::Item>, SourceError>;
}

/// Validates causal time and exact source-prefix continuity around any adapter.
pub struct CausalSource<S: HelioSource> {
    inner: S,
    checkpoint: SourceCheckpoint,
    causal_cut: Option<i64>,
    expected_phase: Option<SourcePhase>,
    from_available_at: Option<i64>,
    until_available_at: Option<i64>,
    started: bool,
}

impl<S: HelioSource> CausalSource<S> {
    pub fn new(inner: S) -> Self {
        let checkpoint = SourceCheckpoint::empty(inner.source_id().clone());
        Self {
            inner,
            checkpoint,
            causal_cut: None,
            expected_phase: None,
            from_available_at: None,
            until_available_at: None,
            started: false,
        }
    }

    pub fn checkpoint(&self) -> &SourceCheckpoint {
        &self.checkpoint
    }

    pub fn into_inner(self) -> S {
        self.inner
    }

    fn validate_request(&self, request: &SourceRequest) -> Result<(), SourceError> {
        if request.checkpoint().source != *self.inner.source_id() {
            return Err(SourceError::WrongSource {
                requested: request.checkpoint().source.clone(),
                actual: self.inner.source_id().clone(),
            });
        }
        match request {
            SourceRequest::Backfill {
                until_available_at,
                as_of,
                ..
            } => {
                if !self.inner.capabilities().backfill {
                    return Err(SourceError::UnsupportedPhase(SourcePhase::Backfill));
                }
                if until_available_at > as_of {
                    return Err(SourceError::InvalidCausalCut {
                        until: *until_available_at,
                        as_of: *as_of,
                    });
                }
            }
            SourceRequest::Live { .. } if !self.inner.capabilities().live => {
                return Err(SourceError::UnsupportedPhase(SourcePhase::Live));
            }
            SourceRequest::Live { .. } => {}
        }
        Ok(())
    }

    fn accept(&mut self, record: &SourceEnvelope<S::Item>) -> Result<bool, SourceError> {
        if record.source != *self.inner.source_id() {
            return Err(SourceError::ForeignRecord {
                expected: self.inner.source_id().clone(),
                actual: record.source.clone(),
            });
        }
        if record.observed_at < record.available_at {
            return Err(SourceError::ObservedBeforeAvailable {
                observed_at: record.observed_at,
                available_at: record.available_at,
            });
        }
        if let Some(expected) = self.expected_phase {
            if record.phase != expected {
                return Err(SourceError::WrongPhase {
                    expected,
                    actual: record.phase,
                });
            }
        }
        if let Some(until) = self.until_available_at {
            if self
                .from_available_at
                .is_some_and(|from| record.available_at < from)
                || record.available_at > until
            {
                return Err(SourceError::OutsideBackfillRange {
                    available_at: record.available_at,
                    from: self.from_available_at,
                    until,
                });
            }
        }
        if let Some(as_of) = self.causal_cut {
            if record.available_at > as_of {
                return Err(SourceError::FutureAvailability {
                    available_at: record.available_at,
                    as_of,
                });
            }
        }

        let partition = record.offset.partition.as_str();
        if let Some(previous) = self.checkpoint.position(partition) {
            if record.offset.sequence <= previous {
                return Ok(false);
            }
            if record.offset.sequence != previous.saturating_add(1) {
                return Err(SourceError::OffsetGap {
                    partition: partition.to_string(),
                    previous,
                    actual: record.offset.sequence,
                });
            }
        }
        if let Some(previous) = self.checkpoint.available_at.get(partition).copied() {
            if record.available_at < previous {
                return Err(SourceError::AvailabilityRegression {
                    partition: partition.to_string(),
                    previous,
                    actual: record.available_at,
                });
            }
        }
        self.checkpoint
            .positions
            .insert(partition.to_string(), record.offset.sequence);
        self.checkpoint
            .available_at
            .insert(partition.to_string(), record.available_at);
        Ok(true)
    }
}

impl<S: HelioSource> HelioSource for CausalSource<S> {
    type Item = S::Item;

    fn source_id(&self) -> &SourceId {
        self.inner.source_id()
    }

    fn capabilities(&self) -> SourceCapabilities {
        self.inner.capabilities()
    }

    fn start(&mut self, request: SourceRequest) -> Result<(), SourceError> {
        self.validate_request(&request)?;
        self.checkpoint = request.checkpoint().clone();
        match &request {
            SourceRequest::Backfill {
                from_available_at,
                until_available_at,
                as_of,
                ..
            } => {
                self.causal_cut = Some(*as_of);
                self.expected_phase = Some(SourcePhase::Backfill);
                self.from_available_at = *from_available_at;
                self.until_available_at = Some(*until_available_at);
            }
            SourceRequest::Live { .. } => {
                self.causal_cut = None;
                self.expected_phase = Some(SourcePhase::Live);
                self.from_available_at = None;
                self.until_available_at = None;
            }
        };
        self.inner.start(request)?;
        self.started = true;
        Ok(())
    }

    fn poll(&mut self, max_records: NonZeroUsize) -> Result<SourcePoll<Self::Item>, SourceError> {
        if !self.started {
            return Err(SourceError::Adapter("source has not been started".into()));
        }
        match self.inner.poll(max_records)? {
            SourcePoll::Records(records) => {
                if records.len() > max_records.get() {
                    return Err(SourceError::OversizedBatch {
                        requested: max_records.get(),
                        actual: records.len(),
                    });
                }
                let mut accepted = Vec::with_capacity(records.len());
                for record in records {
                    if self.accept(&record)? {
                        accepted.push(record);
                    }
                }
                Ok(SourcePoll::Records(accepted))
            }
            SourcePoll::CaughtUp(_) => Ok(SourcePoll::CaughtUp(self.checkpoint.clone())),
            SourcePoll::Idle(_) => Ok(SourcePoll::Idle(self.checkpoint.clone())),
            SourcePoll::End(_) => Ok(SourcePoll::End(self.checkpoint.clone())),
            SourcePoll::Handoff(_) => Ok(SourcePoll::Handoff(self.checkpoint.clone())),
        }
    }
}

enum HandoffPhase {
    NotStarted,
    Backfill,
    Live,
}

/// Runs a bounded historical adapter to a fenced checkpoint, then resumes a live adapter after the
/// exact same prefix. The live source must be resumable, otherwise `start` fails closed.
pub struct ReplayThenLive<B, L>
where
    B: HelioSource,
    L: HelioSource<Item = B::Item>,
{
    backfill: CausalSource<B>,
    live: CausalSource<L>,
    phase: HandoffPhase,
}

impl<B, L> ReplayThenLive<B, L>
where
    B: HelioSource,
    L: HelioSource<Item = B::Item>,
{
    pub fn new(backfill: B, live: L) -> Result<Self, SourceError> {
        if backfill.source_id() != live.source_id() {
            return Err(SourceError::WrongSource {
                requested: backfill.source_id().clone(),
                actual: live.source_id().clone(),
            });
        }
        if !live.capabilities().live || !live.capabilities().resumable {
            return Err(SourceError::UnsupportedPhase(SourcePhase::Live));
        }
        Ok(Self {
            backfill: CausalSource::new(backfill),
            live: CausalSource::new(live),
            phase: HandoffPhase::NotStarted,
        })
    }

    pub fn checkpoint(&self) -> &SourceCheckpoint {
        match self.phase {
            HandoffPhase::Live => self.live.checkpoint(),
            HandoffPhase::NotStarted | HandoffPhase::Backfill => self.backfill.checkpoint(),
        }
    }
}

impl<B, L> HelioSource for ReplayThenLive<B, L>
where
    B: HelioSource,
    L: HelioSource<Item = B::Item>,
{
    type Item = B::Item;

    fn source_id(&self) -> &SourceId {
        self.backfill.source_id()
    }

    fn capabilities(&self) -> SourceCapabilities {
        SourceCapabilities::REPLAY_AND_LIVE
    }

    fn start(&mut self, request: SourceRequest) -> Result<(), SourceError> {
        if !matches!(request, SourceRequest::Backfill { .. }) {
            return Err(SourceError::UnsupportedPhase(SourcePhase::Live));
        }
        self.backfill.start(request)?;
        self.phase = HandoffPhase::Backfill;
        Ok(())
    }

    fn poll(&mut self, max_records: NonZeroUsize) -> Result<SourcePoll<Self::Item>, SourceError> {
        match self.phase {
            HandoffPhase::NotStarted => {
                Err(SourceError::Adapter("source has not been started".into()))
            }
            HandoffPhase::Backfill => match self.backfill.poll(max_records)? {
                SourcePoll::CaughtUp(checkpoint) | SourcePoll::End(checkpoint) => {
                    self.live.start(SourceRequest::Live {
                        checkpoint: checkpoint.clone(),
                    })?;
                    self.phase = HandoffPhase::Live;
                    Ok(SourcePoll::Handoff(checkpoint))
                }
                other => Ok(other),
            },
            HandoffPhase::Live => self.live.poll(max_records),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    struct ScriptedSource {
        id: SourceId,
        capabilities: SourceCapabilities,
        polls: VecDeque<SourcePoll<i32>>,
        started: Vec<SourceRequest>,
    }

    impl ScriptedSource {
        fn new(capabilities: SourceCapabilities, polls: Vec<SourcePoll<i32>>) -> Self {
            Self {
                id: SourceId::new("fixture"),
                capabilities,
                polls: polls.into(),
                started: Vec::new(),
            }
        }
    }

    impl HelioSource for ScriptedSource {
        type Item = i32;

        fn source_id(&self) -> &SourceId {
            &self.id
        }

        fn capabilities(&self) -> SourceCapabilities {
            self.capabilities
        }

        fn start(&mut self, request: SourceRequest) -> Result<(), SourceError> {
            self.started.push(request);
            Ok(())
        }

        fn poll(
            &mut self,
            _max_records: NonZeroUsize,
        ) -> Result<SourcePoll<Self::Item>, SourceError> {
            Ok(self
                .polls
                .pop_front()
                .unwrap_or_else(|| SourcePoll::Idle(SourceCheckpoint::empty(self.id.clone()))))
        }
    }

    fn envelope(sequence: u64, available_at: i64, phase: SourcePhase) -> SourceEnvelope<i32> {
        SourceEnvelope {
            source: SourceId::new("fixture"),
            offset: SourceOffset::new("p0", sequence),
            event_time: available_at - 5,
            available_at,
            observed_at: available_at + 1,
            phase,
            payload: sequence as i32,
        }
    }

    fn empty_checkpoint() -> SourceCheckpoint {
        SourceCheckpoint::empty(SourceId::new("fixture"))
    }

    #[test]
    fn causal_wrapper_deduplicates_replay_and_rejects_gaps() {
        let source = ScriptedSource::new(
            SourceCapabilities::REPLAY_AND_LIVE,
            vec![SourcePoll::Records(vec![
                envelope(10, 100, SourcePhase::Backfill),
                envelope(10, 100, SourcePhase::Backfill),
                envelope(12, 102, SourcePhase::Backfill),
            ])],
        );
        let mut source = CausalSource::new(source);
        source
            .start(SourceRequest::Backfill {
                checkpoint: empty_checkpoint(),
                from_available_at: Some(90),
                until_available_at: 110,
                as_of: 110,
            })
            .unwrap();
        let err = source.poll(NonZeroUsize::new(8).unwrap()).unwrap_err();
        assert_eq!(
            err,
            SourceError::OffsetGap {
                partition: "p0".into(),
                previous: 10,
                actual: 12,
            }
        );
    }

    #[test]
    fn replay_then_live_handoff_uses_exact_checkpoint() {
        let backfill = ScriptedSource::new(
            SourceCapabilities::REPLAY_AND_LIVE,
            vec![
                SourcePoll::Records(vec![
                    envelope(7, 70, SourcePhase::Backfill),
                    envelope(8, 80, SourcePhase::Backfill),
                ]),
                SourcePoll::CaughtUp(empty_checkpoint()),
            ],
        );
        let live = ScriptedSource::new(
            SourceCapabilities::REPLAY_AND_LIVE,
            vec![SourcePoll::Records(vec![envelope(
                9,
                90,
                SourcePhase::Live,
            )])],
        );
        let mut source = ReplayThenLive::new(backfill, live).unwrap();
        source
            .start(SourceRequest::Backfill {
                checkpoint: empty_checkpoint(),
                from_available_at: None,
                until_available_at: 80,
                as_of: 80,
            })
            .unwrap();

        let first = source.poll(NonZeroUsize::new(2).unwrap()).unwrap();
        assert!(matches!(first, SourcePoll::Records(ref rows) if rows.len() == 2));
        let handoff = source.poll(NonZeroUsize::new(2).unwrap()).unwrap();
        assert!(matches!(handoff, SourcePoll::Handoff(ref cp) if cp.position("p0") == Some(8)));
        let live = source.poll(NonZeroUsize::new(2).unwrap()).unwrap();
        assert!(matches!(live, SourcePoll::Records(ref rows) if rows[0].offset.sequence == 9));
        assert_eq!(source.checkpoint().position("p0"), Some(9));
    }

    #[test]
    fn replay_rejects_information_after_the_decision_cut() {
        let source = ScriptedSource::new(
            SourceCapabilities::REPLAY_AND_LIVE,
            vec![SourcePoll::Records(vec![envelope(
                1,
                101,
                SourcePhase::Backfill,
            )])],
        );
        let mut source = CausalSource::new(source);
        source
            .start(SourceRequest::Backfill {
                checkpoint: empty_checkpoint(),
                from_available_at: None,
                until_available_at: 100,
                as_of: 100,
            })
            .unwrap();
        assert_eq!(
            source.poll(NonZeroUsize::new(1).unwrap()).unwrap_err(),
            SourceError::OutsideBackfillRange {
                available_at: 101,
                from: None,
                until: 100,
            }
        );
    }

    #[test]
    fn replay_rejects_live_records_even_when_their_clock_is_causal() {
        let source = ScriptedSource::new(
            SourceCapabilities::REPLAY_AND_LIVE,
            vec![SourcePoll::Records(vec![envelope(
                1,
                100,
                SourcePhase::Live,
            )])],
        );
        let mut source = CausalSource::new(source);
        source
            .start(SourceRequest::Backfill {
                checkpoint: empty_checkpoint(),
                from_available_at: Some(90),
                until_available_at: 100,
                as_of: 100,
            })
            .unwrap();

        assert_eq!(
            source.poll(NonZeroUsize::new(1).unwrap()).unwrap_err(),
            SourceError::WrongPhase {
                expected: SourcePhase::Backfill,
                actual: SourcePhase::Live,
            }
        );
    }

    #[test]
    fn replay_rejects_records_before_the_requested_availability_floor() {
        let source = ScriptedSource::new(
            SourceCapabilities::REPLAY_AND_LIVE,
            vec![SourcePoll::Records(vec![envelope(
                1,
                89,
                SourcePhase::Backfill,
            )])],
        );
        let mut source = CausalSource::new(source);
        source
            .start(SourceRequest::Backfill {
                checkpoint: empty_checkpoint(),
                from_available_at: Some(90),
                until_available_at: 100,
                as_of: 100,
            })
            .unwrap();

        assert_eq!(
            source.poll(NonZeroUsize::new(1).unwrap()).unwrap_err(),
            SourceError::OutsideBackfillRange {
                available_at: 89,
                from: Some(90),
                until: 100,
            }
        );
    }
}
