//! End-to-end replay record: shocks + bars → [`TradeResult`](crate::TradeResult).

use helio_scan::{
    DiscardEmitter, Emit, FlushReason, FlushableScan, Scan, SessionDate, SnapshottingScan,
    VersionedSnapshot,
};
use helio_time::{utc_calendar_day, AvailableAt, SimpleWeekdayCalendar, TradingCalendar};
use serde::{Deserialize, Serialize};

use crate::{
    AlignedEventShock, EventShockAlignPipelineScan, EventShockControlConfig,
    EventShockControlSamplerScan, EventShockFilterConfig, EventShockReplayRecord, EventShockSignal,
    EventShockStreamItem, EventShockToSignalScan, ExecutionBufferPolicy, ExecutionEntryTiming,
    ExitPolicy, Exposure, SignalExecutionScan, TradeResult,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EventShockVerticalRecord {
    /// `stream_seq` preserves ingest order among shocks that share the same merge bucket.
    Shock(u32, EventShockStreamItem),
    Bar(crate::DailyBar),
}

#[derive(Debug, Clone)]
pub struct EventShockVerticalScan<C: TradingCalendar + Copy = SimpleWeekdayCalendar> {
    pub align_pipe: EventShockAlignPipelineScan<C>,
    pub to_signal: EventShockToSignalScan<C>,
    pub control: EventShockControlSamplerScan<C>,
    pub exec: SignalExecutionScan<C>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EventShockVerticalState<C: TradingCalendar + Copy = SimpleWeekdayCalendar> {
    pub align_pipe: crate::EventShockAlignPipelineState<C>,
    pub to_signal: crate::EventShockToSignalState,
    pub control: crate::EventShockControlSamplerState,
    pub exec: crate::EventShockExecutionState,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventShockVerticalSnapshot {
    pub align_pipe: crate::EventShockAlignPipelineSnapshot,
    pub to_signal: crate::EventShockToSignalSnapshot,
    pub control: crate::EventShockControlSamplerSnapshot,
    pub exec: crate::EventShockExecutionSnapshot,
}

struct SignalExecutionEmit<'a, C, E>
where
    C: TradingCalendar + Copy,
{
    scan: &'a SignalExecutionScan<C>,
    state: &'a mut crate::EventShockExecutionState,
    sink: &'a mut E,
}

impl<C, E> Emit<EventShockSignal> for SignalExecutionEmit<'_, C, E>
where
    C: TradingCalendar + Copy,
    E: Emit<TradeResult>,
{
    #[inline]
    fn emit(&mut self, signal: EventShockSignal) {
        self.scan.step(
            self.state,
            EventShockReplayRecord::Signal(signal),
            self.sink,
        );
    }
}

struct AlignedExecutionEmit<'a, C, E>
where
    C: TradingCalendar + Copy,
{
    to_signal: &'a EventShockToSignalScan<C>,
    to_signal_state: &'a mut crate::EventShockToSignalState,
    control: &'a EventShockControlSamplerScan<C>,
    control_state: &'a mut crate::EventShockControlSamplerState,
    exec: &'a SignalExecutionScan<C>,
    exec_state: &'a mut crate::EventShockExecutionState,
    sink: &'a mut E,
}

impl<C, E> Emit<AlignedEventShock> for AlignedExecutionEmit<'_, C, E>
where
    C: TradingCalendar + Copy,
    E: Emit<TradeResult>,
{
    fn emit(&mut self, aligned: AlignedEventShock) {
        {
            let mut signal_emit = SignalExecutionEmit {
                scan: self.exec,
                state: self.exec_state,
                sink: self.sink,
            };
            self.to_signal
                .step(self.to_signal_state, aligned.clone(), &mut signal_emit);
        }
        let mut control_emit = SignalExecutionEmit {
            scan: self.exec,
            state: self.exec_state,
            sink: self.sink,
        };
        self.control
            .step(self.control_state, aligned, &mut control_emit);
    }
}

impl<C: TradingCalendar + Copy> EventShockVerticalScan<C> {
    pub fn new(
        decision_available: Option<AvailableAt>,
        filter: EventShockFilterConfig,
        calendar: C,
        exit_policy: ExitPolicy,
        exposure: Exposure,
        control_cfg: EventShockControlConfig,
        candidate_entries: Vec<SessionDate>,
        execution_entry_timing: ExecutionEntryTiming,
        strategy_name: impl Into<String>,
    ) -> Self {
        Self::with_exec_buffer(
            decision_available,
            filter,
            calendar,
            exit_policy,
            exposure,
            control_cfg,
            candidate_entries,
            execution_entry_timing,
            ExecutionBufferPolicy::default(),
            strategy_name,
        )
    }

    pub fn with_exec_buffer(
        decision_available: Option<AvailableAt>,
        filter: EventShockFilterConfig,
        calendar: C,
        exit_policy: ExitPolicy,
        exposure: Exposure,
        mut control_cfg: EventShockControlConfig,
        candidate_entries: Vec<SessionDate>,
        execution_entry_timing: ExecutionEntryTiming,
        exec_buffer: ExecutionBufferPolicy,
        strategy_name: impl Into<String>,
    ) -> Self {
        let strategy_name = strategy_name.into();
        control_cfg.strategy_name = strategy_name.clone();
        control_cfg.horizon_sessions = match exit_policy {
            ExitPolicy::FixedHorizonSessions { n } => n,
            _ => control_cfg.horizon_sessions.max(1),
        };
        let ctrl = EventShockControlSamplerScan::new(control_cfg, calendar, candidate_entries);
        Self {
            align_pipe: EventShockAlignPipelineScan::new(decision_available, filter, calendar),
            to_signal: EventShockToSignalScan {
                exit_policy,
                exposure,
                calendar,
                strategy_name,
            },
            control: ctrl,
            exec: SignalExecutionScan::with_timing_and_buffer(
                calendar,
                execution_entry_timing,
                exec_buffer,
            ),
        }
    }
}

impl<C: TradingCalendar + Copy> Scan for EventShockVerticalScan<C> {
    type In = EventShockVerticalRecord;
    type Out = TradeResult;
    type State = EventShockVerticalState<C>;

    fn init(&self) -> Self::State {
        EventShockVerticalState {
            align_pipe: self.align_pipe.init(),
            to_signal: self.to_signal.init(),
            control: self.control.init(),
            exec: self.exec.init(),
        }
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        match input {
            EventShockVerticalRecord::Shock(_, shock) => {
                let mut aligned_emit = AlignedExecutionEmit {
                    to_signal: &self.to_signal,
                    to_signal_state: &mut state.to_signal,
                    control: &self.control,
                    control_state: &mut state.control,
                    exec: &self.exec,
                    exec_state: &mut state.exec,
                    sink: emit,
                };
                self.align_pipe
                    .step(&mut state.align_pipe, shock, &mut aligned_emit);
            }
            EventShockVerticalRecord::Bar(b) => {
                self.exec
                    .step(&mut state.exec, EventShockReplayRecord::Bar(b), emit);
            }
        }
    }
}

impl<C: TradingCalendar + Copy> FlushableScan for EventShockVerticalScan<C> {
    type Offset = u64;

    /// Forwards `signal` to sub-scans in order (align, to_signal, control, exec).
    ///
    /// **`FlushReason::Checkpoint`:** each sub-scan receives the same variant; callers should only
    /// emit checkpoints at record boundaries consistent with their persistence policy. The
    /// execution sub-scan currently ignores flush (pending signals remain until priced out).
    fn flush<E>(&self, state: &mut Self::State, signal: FlushReason<Self::Offset>, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        self.align_pipe
            .flush(&mut state.align_pipe, signal.clone(), &mut DiscardEmitter);
        self.to_signal
            .flush(&mut state.to_signal, signal.clone(), &mut DiscardEmitter);
        self.control
            .flush(&mut state.control, signal.clone(), &mut DiscardEmitter);
        self.exec.flush(&mut state.exec, signal, emit);
    }
}

impl<C: TradingCalendar + Copy> SnapshottingScan for EventShockVerticalScan<C> {
    type Snapshot = EventShockVerticalSnapshot;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        EventShockVerticalSnapshot {
            align_pipe: self.align_pipe.snapshot(&state.align_pipe),
            to_signal: self.to_signal.snapshot(&state.to_signal),
            control: self.control.snapshot(&state.control),
            exec: self.exec.snapshot(&state.exec),
        }
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        EventShockVerticalState {
            align_pipe: self.align_pipe.restore(snapshot.align_pipe),
            to_signal: self.to_signal.restore(snapshot.to_signal),
            control: self.control.restore(snapshot.control),
            exec: self.exec.restore(snapshot.exec),
        }
    }
}

impl VersionedSnapshot for EventShockVerticalSnapshot {
    const VERSION: u32 = 1;
}

/// Stable sort key for merging shocks and bars: bars first per session, then shocks.
///
/// Tuple is `(session_index, kind, stream_seq)` where `kind` is `0` for bars and `1` for shocks
/// so bars sort before shocks in the same session. For shocks, `session_index` is
/// [`EventShockStreamItem::session_date`] when set, otherwise [`utc_calendar_day`] of
/// `available_at` (legacy [`crate::build_vertical_replay`]); prefer setting `session_date` via
/// [`crate::build_vertical_replay_with_calendar`].
#[inline]
pub fn vertical_merge_key(rec: &EventShockVerticalRecord) -> (i32, u8, u32) {
    match rec {
        EventShockVerticalRecord::Bar(b) => (b.session.0, 0, 0),
        EventShockVerticalRecord::Shock(seq, t) => {
            let s = t
                .session_date
                .map(|d| d.0)
                .unwrap_or_else(|| utc_calendar_day(t.available_at.0));
            (s, 1, *seq)
        }
    }
}
