//! Generic **forecastable event shock** → session alignment → signal → simulated trade results.
//!
//! Ingest stays **domain-agnostic**: attach opaque [`EventShock::tags`] (CSV / JSONL) for your
//! own taxonomy; the scan stack does not interpret them.

use helio_scan::{
    Emit, FlushReason, FlushableScan, Scan, SessionDate, SnapshottingScan, VersionedSnapshot,
};
use helio_time::{
    AvailabilityGateScan, AvailableAt, ObservedAt, SimpleWeekdayCalendar, Timed, TradingCalendar,
};
use serde::{Deserialize, Serialize};
use std::marker::PhantomData;

/// Epoch seconds UTC (wall), same unit as [`AvailableAt`](helio_time::AvailableAt).
pub type UtcTs = i64;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Symbol(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EventId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EventScope {
    Global,
    Region(u32),
    Sector(u32),
    Basket(u32),
    Instrument(Symbol),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventShock {
    pub id: EventId,
    /// Caller-defined labels (comma-separated in CSV); not used by the shock vertical.
    #[serde(default)]
    pub tags: String,
    pub observed_at: Option<ObservedAt>,
    /// Earliest instant the event may be acted on (causal cut for the shock itself).
    pub available_at: AvailableAt,
    pub impact_start: UtcTs,
    pub impact_end: UtcTs,
    pub severity: f64,
    pub confidence: f64,
    pub scope: EventScope,
}

#[inline]
pub fn signal_lead_secs(shock: &EventShock) -> i64 {
    shock.impact_start.saturating_sub(shock.available_at.0)
}

/// Session-aligned shock for strategy scans (UTC calendar semantics via [`TradingCalendar`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlignedEventShock {
    pub event_id: EventId,
    pub entry_session: SessionDate,
    pub impact_start_session: SessionDate,
    pub impact_end_session: SessionDate,
    pub severity: f64,
    pub confidence: f64,
    pub scope: EventScope,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Exposure {
    Long(Symbol),
    Short(Symbol),
    Pair { long: Symbol, short: Symbol },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventShockSignal {
    pub event_id: EventId,
    pub entry_session: SessionDate,
    pub exit_session: SessionDate,
    pub exposure: Exposure,
    /// `Some(treatment)` when this row is a matched control for causal comparison.
    pub matched_treatment: Option<EventId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TradeResult {
    pub event_id: EventId,
    pub entry_session: SessionDate,
    pub exit_session: SessionDate,
    pub gross_return: f64,
    pub max_drawdown: f64,
    pub holding_period_sessions: u32,
    /// When set, this row is a matched control for the given treatment event.
    pub matched_treatment: Option<EventId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExitPolicy {
    AtImpactStartSession,
    MidImpactWindowSession,
    FixedHorizonSessions { n: u32 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ScopeFilter {
    Any,
    Match(EventScope),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventShockFilterConfig {
    pub min_severity: f64,
    pub min_confidence: f64,
    pub min_lead_secs: i64,
    pub max_lead_secs: i64,
    pub scope: ScopeFilter,
}

impl Default for EventShockFilterConfig {
    fn default() -> Self {
        Self {
            min_severity: 0.0,
            min_confidence: 0.0,
            min_lead_secs: 0,
            max_lead_secs: i64::MAX,
            scope: ScopeFilter::Any,
        }
    }
}

fn scope_matches(filter: &ScopeFilter, scope: &EventScope) -> bool {
    match filter {
        ScopeFilter::Any => true,
        ScopeFilter::Match(s) => s == scope,
    }
}

#[derive(Debug, Clone)]
pub struct EventShockFilterScan {
    pub cfg: EventShockFilterConfig,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventShockFilterState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventShockFilterSnapshot;

impl Scan for EventShockFilterScan {
    type In = Timed<EventShock>;
    type Out = Timed<EventShock>;
    type State = EventShockFilterState;

    fn init(&self) -> Self::State {
        EventShockFilterState
    }

    fn step<E>(&self, _state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        let v = &input.value;
        if v.severity < self.cfg.min_severity || v.confidence < self.cfg.min_confidence {
            return;
        }
        if v.impact_end < v.impact_start {
            return;
        }
        if v.impact_start < input.available_at.0 {
            return;
        }
        if !scope_matches(&self.cfg.scope, &v.scope) {
            return;
        }
        let lead = signal_lead_secs(v);
        if lead < self.cfg.min_lead_secs || lead > self.cfg.max_lead_secs {
            return;
        }
        emit.emit(input);
    }
}

impl FlushableScan for EventShockFilterScan {
    type Offset = u64;

    fn flush<E>(&self, _state: &mut Self::State, _signal: FlushReason<Self::Offset>, _emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
    }
}

impl SnapshottingScan for EventShockFilterScan {
    type Snapshot = EventShockFilterSnapshot;

    fn snapshot(&self, _state: &Self::State) -> Self::Snapshot {
        EventShockFilterSnapshot
    }

    fn restore(&self, _snapshot: Self::Snapshot) -> Self::State {
        EventShockFilterState
    }
}

impl VersionedSnapshot for EventShockFilterSnapshot {
    const VERSION: u32 = 1;
}

/// Map UTC instants to sessions; **entry** = first trading session strictly after `available_at`.
#[derive(Debug, Clone, Copy)]
pub struct EventShockAlignScan<C: TradingCalendar + Copy = SimpleWeekdayCalendar> {
    pub calendar: C,
    _p: PhantomData<C>,
}

impl<C: TradingCalendar + Copy> EventShockAlignScan<C> {
    pub fn new(calendar: C) -> Self {
        Self {
            calendar,
            _p: PhantomData,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventShockAlignState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventShockAlignSnapshot;

impl<C: TradingCalendar + Copy> Scan for EventShockAlignScan<C> {
    type In = Timed<EventShock>;
    type Out = AlignedEventShock;
    type State = EventShockAlignState;

    fn init(&self) -> Self::State {
        EventShockAlignState
    }

    fn step<E>(&self, _state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        let v = &input.value;
        let entry_session = self
            .calendar
            .first_session_strictly_after_ts(input.available_at.0);
        let impact_start_session = self.calendar.session_on_or_after_ts(v.impact_start);
        let impact_end_session = self.calendar.session_on_or_before_ts(v.impact_end);
        emit.emit(AlignedEventShock {
            event_id: v.id,
            entry_session,
            impact_start_session,
            impact_end_session,
            severity: v.severity,
            confidence: v.confidence,
            scope: v.scope.clone(),
        });
    }
}

impl<C: TradingCalendar + Copy> FlushableScan for EventShockAlignScan<C> {
    type Offset = u64;

    fn flush<E>(&self, _state: &mut Self::State, _signal: FlushReason<Self::Offset>, _emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
    }
}

impl<C: TradingCalendar + Copy> SnapshottingScan for EventShockAlignScan<C> {
    type Snapshot = EventShockAlignSnapshot;

    fn snapshot(&self, _state: &Self::State) -> Self::Snapshot {
        EventShockAlignSnapshot
    }

    fn restore(&self, _snapshot: Self::Snapshot) -> Self::State {
        EventShockAlignState
    }
}

impl VersionedSnapshot for EventShockAlignSnapshot {
    const VERSION: u32 = 1;
}

#[derive(Debug, Clone)]
pub struct EventShockToSignalScan<C: TradingCalendar + Copy = SimpleWeekdayCalendar> {
    pub exit_policy: ExitPolicy,
    pub exposure: Exposure,
    pub calendar: C,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventShockToSignalState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventShockToSignalSnapshot;

impl<C: TradingCalendar + Copy> EventShockToSignalScan<C> {
    fn exit_session(&self, a: &AlignedEventShock) -> SessionDate {
        match self.exit_policy {
            ExitPolicy::AtImpactStartSession => a.impact_start_session,
            ExitPolicy::MidImpactWindowSession => self
                .calendar
                .mid_session_inclusive(a.impact_start_session, a.impact_end_session),
            ExitPolicy::FixedHorizonSessions { n } => {
                self.calendar.add_sessions(a.entry_session, n)
            }
        }
    }
}

impl<C: TradingCalendar + Copy> Scan for EventShockToSignalScan<C> {
    type In = AlignedEventShock;
    type Out = EventShockSignal;
    type State = EventShockToSignalState;

    fn init(&self) -> Self::State {
        EventShockToSignalState
    }

    fn step<E>(&self, _state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        let exit_session = self.exit_session(&input);
        if exit_session.0 < input.entry_session.0 {
            return;
        }
        emit.emit(EventShockSignal {
            event_id: input.event_id,
            entry_session: input.entry_session,
            exit_session,
            exposure: self.exposure.clone(),
            matched_treatment: None,
        });
    }
}

impl<C: TradingCalendar + Copy> FlushableScan for EventShockToSignalScan<C> {
    type Offset = u64;

    fn flush<E>(&self, _state: &mut Self::State, _signal: FlushReason<Self::Offset>, _emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
    }
}

impl<C: TradingCalendar + Copy> SnapshottingScan for EventShockToSignalScan<C> {
    type Snapshot = EventShockToSignalSnapshot;

    fn snapshot(&self, _state: &Self::State) -> Self::Snapshot {
        EventShockToSignalSnapshot
    }

    fn restore(&self, _snapshot: Self::Snapshot) -> Self::State {
        EventShockToSignalState
    }
}

impl VersionedSnapshot for EventShockToSignalSnapshot {
    const VERSION: u32 = 1;
}

pub type EventShockAvailabilityGateScan = AvailabilityGateScan<EventShock>;

/// `Timed<EventShock>` stream item: `available_at` on the struct and on [`Timed`] must agree for ingest helpers.
pub type EventShockStreamItem = Timed<EventShock>;

/// Gate → filter → align (no signal yet — fork treatment vs controls downstream).
#[derive(Debug, Clone)]
pub struct EventShockAlignPipelineScan<C: TradingCalendar + Copy = SimpleWeekdayCalendar> {
    pub gate: AvailabilityGateScan<EventShock>,
    pub filter: EventShockFilterScan,
    pub align: EventShockAlignScan<C>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventShockAlignPipelineState<C: TradingCalendar + Copy = SimpleWeekdayCalendar> {
    pub gate: helio_time::AvailabilityGateState,
    pub filter: EventShockFilterState,
    pub align: EventShockAlignState,
    _p: PhantomData<C>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventShockAlignPipelineSnapshot {
    pub gate: helio_time::AvailabilityGateSnapshot,
    pub filter: EventShockFilterSnapshot,
    pub align: EventShockAlignSnapshot,
}

impl<C: TradingCalendar + Copy> EventShockAlignPipelineScan<C> {
    pub fn new(
        decision_available: Option<AvailableAt>,
        filter: EventShockFilterConfig,
        calendar: C,
    ) -> Self {
        Self {
            gate: AvailabilityGateScan::new(decision_available),
            filter: EventShockFilterScan { cfg: filter },
            align: EventShockAlignScan::new(calendar),
        }
    }
}

impl<C: TradingCalendar + Copy> Scan for EventShockAlignPipelineScan<C> {
    type In = EventShockStreamItem;
    type Out = AlignedEventShock;
    type State = EventShockAlignPipelineState<C>;

    fn init(&self) -> Self::State {
        EventShockAlignPipelineState {
            gate: self.gate.init(),
            filter: self.filter.init(),
            align: self.align.init(),
            _p: PhantomData,
        }
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        let mut a = helio_scan::VecEmitter::new();
        self.gate.step(&mut state.gate, input, &mut a);
        for x in a.into_inner() {
            let mut b = helio_scan::VecEmitter::new();
            self.filter.step(&mut state.filter, x, &mut b);
            for y in b.into_inner() {
                self.align.step(&mut state.align, y, emit);
            }
        }
    }
}

impl<C: TradingCalendar + Copy> FlushableScan for EventShockAlignPipelineScan<C> {
    type Offset = u64;

    fn flush<E>(&self, state: &mut Self::State, signal: FlushReason<Self::Offset>, _emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        let mut e_gate = helio_scan::VecEmitter::new();
        self.gate
            .flush(&mut state.gate, signal.clone(), &mut e_gate);
        let mut e_filter = helio_scan::VecEmitter::new();
        self.filter
            .flush(&mut state.filter, signal.clone(), &mut e_filter);
        self.align
            .flush(&mut state.align, signal, &mut helio_scan::VecEmitter::new());
    }
}

impl<C: TradingCalendar + Copy> SnapshottingScan for EventShockAlignPipelineScan<C> {
    type Snapshot = EventShockAlignPipelineSnapshot;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        EventShockAlignPipelineSnapshot {
            gate: self.gate.snapshot(&state.gate),
            filter: self.filter.snapshot(&state.filter),
            align: self.align.snapshot(&state.align),
        }
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        EventShockAlignPipelineState {
            gate: self.gate.restore(snapshot.gate),
            filter: self.filter.restore(snapshot.filter),
            align: self.align.restore(snapshot.align),
            _p: PhantomData,
        }
    }
}

impl VersionedSnapshot for EventShockAlignPipelineSnapshot {
    const VERSION: u32 = 1;
}

/// Gate → filter → align → signal (v1 vertical core).
#[derive(Debug, Clone)]
pub struct EventShockSignalKernelScan<C: TradingCalendar + Copy = SimpleWeekdayCalendar> {
    pub align_pipe: EventShockAlignPipelineScan<C>,
    pub to_signal: EventShockToSignalScan<C>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventShockSignalKernelState<C: TradingCalendar + Copy = SimpleWeekdayCalendar> {
    pub align_pipe: EventShockAlignPipelineState<C>,
    pub to_signal: EventShockToSignalState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventShockSignalKernelSnapshot {
    pub align_pipe: EventShockAlignPipelineSnapshot,
    pub to_signal: EventShockToSignalSnapshot,
}

impl<C: TradingCalendar + Copy> EventShockSignalKernelScan<C> {
    pub fn new(
        decision_available: Option<AvailableAt>,
        filter: EventShockFilterConfig,
        calendar: C,
        exit_policy: ExitPolicy,
        exposure: Exposure,
    ) -> Self {
        Self {
            align_pipe: EventShockAlignPipelineScan::new(decision_available, filter, calendar),
            to_signal: EventShockToSignalScan {
                exit_policy,
                exposure,
                calendar,
            },
        }
    }
}

impl<C: TradingCalendar + Copy> Scan for EventShockSignalKernelScan<C> {
    type In = EventShockStreamItem;
    type Out = EventShockSignal;
    type State = EventShockSignalKernelState<C>;

    fn init(&self) -> Self::State {
        EventShockSignalKernelState {
            align_pipe: self.align_pipe.init(),
            to_signal: self.to_signal.init(),
        }
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        let mut c = helio_scan::VecEmitter::new();
        self.align_pipe.step(&mut state.align_pipe, input, &mut c);
        for z in c.into_inner() {
            let mut d = helio_scan::VecEmitter::new();
            self.to_signal.step(&mut state.to_signal, z, &mut d);
            for sig in d.into_inner() {
                emit.emit(sig);
            }
        }
    }
}

impl<C: TradingCalendar + Copy> FlushableScan for EventShockSignalKernelScan<C> {
    type Offset = u64;

    fn flush<E>(&self, state: &mut Self::State, signal: FlushReason<Self::Offset>, _emit: &mut E)
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
            signal,
            &mut helio_scan::VecEmitter::new(),
        );
    }
}

impl<C: TradingCalendar + Copy> SnapshottingScan for EventShockSignalKernelScan<C> {
    type Snapshot = EventShockSignalKernelSnapshot;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        EventShockSignalKernelSnapshot {
            align_pipe: self.align_pipe.snapshot(&state.align_pipe),
            to_signal: self.to_signal.snapshot(&state.to_signal),
        }
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        EventShockSignalKernelState {
            align_pipe: self.align_pipe.restore(snapshot.align_pipe),
            to_signal: self.to_signal.restore(snapshot.to_signal),
        }
    }
}

impl VersionedSnapshot for EventShockSignalKernelSnapshot {
    const VERSION: u32 = 1;
}

/// Wrap [`EventShock`] so [`Timed::available_at`] matches the payload (for gate correctness).
#[inline]
pub fn timed_shock(shock: EventShock) -> Timed<EventShock> {
    let a = shock.available_at;
    Timed {
        value: shock,
        observed_at: None,
        available_at: a,
        effective_at: None,
        session_date: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use helio_scan::VecEmitter;

    fn shock(av: i64, impact_s: i64, impact_e: i64, sev: f64) -> Timed<EventShock> {
        timed_shock(EventShock {
            id: EventId(1),
            tags: String::new(),
            observed_at: None,
            available_at: AvailableAt(av),
            impact_start: impact_s,
            impact_end: impact_e,
            severity: sev,
            confidence: 1.0,
            scope: EventScope::Global,
        })
    }

    #[test]
    fn lead_filter_drops_short_and_long_lead() {
        let f = EventShockFilterScan {
            cfg: EventShockFilterConfig {
                min_severity: 0.5,
                min_confidence: 0.0,
                min_lead_secs: 100,
                max_lead_secs: 1000,
                scope: ScopeFilter::Any,
            },
        };
        let mut st = f.init();
        let mut e = VecEmitter::new();
        f.step(&mut st, shock(100, 150, 200, 1.0), &mut e);
        assert!(e.0.is_empty());
        f.step(&mut st, shock(1000, 3000, 3100, 1.0), &mut e);
        assert!(e.0.is_empty());
        f.step(&mut st, shock(1000, 1500, 1600, 1.0), &mut e);
        assert_eq!(e.0.len(), 1);
    }

    #[test]
    fn signal_kernel_respects_availability_gate() {
        let cal = SimpleWeekdayCalendar;
        let d = |n: i32| (n as i64) * 86_400;
        let kernel = EventShockSignalKernelScan::new(
            Some(AvailableAt(d(10) + 100)),
            EventShockFilterConfig {
                min_severity: 0.0,
                min_confidence: 0.0,
                min_lead_secs: 0,
                max_lead_secs: i64::MAX,
                scope: ScopeFilter::Any,
            },
            cal,
            ExitPolicy::AtImpactStartSession,
            Exposure::Long(Symbol("SPY".into())),
        );
        let mut st = kernel.init();
        let mut e = VecEmitter::new();
        let mut s = shock(d(10) + 200, d(15), d(20), 1.0);
        s.available_at = AvailableAt(d(10) + 500);
        s.value.available_at = AvailableAt(d(10) + 500);
        kernel.step(&mut st, s, &mut e);
        assert!(e.0.is_empty());

        let s2 = shock(d(10) + 100, d(15), d(20), 1.0);
        kernel.step(&mut st, s2, &mut e);
        assert_eq!(e.0.len(), 1);
        assert_eq!(e.0[0].event_id, EventId(1));
        assert!(e.0[0].exit_session.0 >= e.0[0].entry_session.0);
    }
}
