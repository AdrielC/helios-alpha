//! End-to-end replay record: shocks + bars → [`TradeResult`](crate::TradeResult).

use helio_scan::{
    Emit, FlushReason, FlushableScan, Scan, SessionDate, SnapshottingScan, VersionedSnapshot,
};
use helio_time::{utc_calendar_day, AvailableAt, SimpleWeekdayCalendar, TradingCalendar};
use serde::{Deserialize, Serialize};

use crate::{
    EventShockAlignPipelineScan, EventShockControlConfig, EventShockControlSamplerScan,
    EventShockFilterConfig, EventShockReplayRecord, EventShockStreamItem, EventShockToSignalScan,
    ExecutionEntryTiming, ExitPolicy, Exposure, SignalExecutionScan, TradeResult,
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

impl<C: TradingCalendar + Copy> EventShockVerticalScan<C> {
    pub fn new(
        decision_available: Option<AvailableAt>,
        filter: EventShockFilterConfig,
        calendar: C,
        exit_policy: ExitPolicy,
        exposure: Exposure,
        mut control_cfg: EventShockControlConfig,
        candidate_entries: Vec<SessionDate>,
        execution_entry_timing: ExecutionEntryTiming,
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
            exec: SignalExecutionScan::with_timing(calendar, execution_entry_timing),
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
                let mut aligned = helio_scan::VecEmitter::new();
                self.align_pipe
                    .step(&mut state.align_pipe, shock, &mut aligned);
                for a in aligned.into_inner() {
                    let mut sigs = helio_scan::VecEmitter::new();
                    self.to_signal
                        .step(&mut state.to_signal, a.clone(), &mut sigs);
                    for s in sigs.into_inner() {
                        let mut e2 = helio_scan::VecEmitter::new();
                        self.exec
                            .step(&mut state.exec, EventShockReplayRecord::Signal(s), &mut e2);
                        for tr in e2.into_inner() {
                            emit.emit(tr);
                        }
                    }
                    let mut ctr = helio_scan::VecEmitter::new();
                    self.control.step(&mut state.control, a, &mut ctr);
                    for s in ctr.into_inner() {
                        let mut e2 = helio_scan::VecEmitter::new();
                        self.exec
                            .step(&mut state.exec, EventShockReplayRecord::Signal(s), &mut e2);
                        for tr in e2.into_inner() {
                            emit.emit(tr);
                        }
                    }
                }
            }
            EventShockVerticalRecord::Bar(b) => {
                let mut e2 = helio_scan::VecEmitter::new();
                self.exec
                    .step(&mut state.exec, EventShockReplayRecord::Bar(b), &mut e2);
                for tr in e2.into_inner() {
                    emit.emit(tr);
                }
            }
        }
    }
}

impl<C: TradingCalendar + Copy> FlushableScan for EventShockVerticalScan<C> {
    type Offset = u64;

    fn flush<E>(&self, state: &mut Self::State, signal: FlushReason<Self::Offset>, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        self.align_pipe.flush(
            &mut state.align_pipe,
            signal.clone(),
            &mut helio_scan::VecEmitter::new(),
        );
        self.to_signal.flush(
            &mut state.to_signal,
            signal.clone(),
            &mut helio_scan::VecEmitter::new(),
        );
        self.control.flush(
            &mut state.control,
            signal.clone(),
            &mut helio_scan::VecEmitter::new(),
        );
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
