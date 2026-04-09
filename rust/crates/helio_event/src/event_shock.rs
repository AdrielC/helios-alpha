//! Generic **forecastable event shock** model: observation → availability → impact window → signal.
//! Domain-agnostic; solar, weather, earnings, etc. differ only in metadata and upstream adapters.

use helio_scan::{
    Emit, FlushReason, FlushableScan, Scan, SessionDate, SnapshottingScan, VersionedSnapshot,
};
use helio_time::{AvailabilityGateScan, AvailableAt, ObservedAt, SessionAlignScan, Timed};
use serde::{Deserialize, Serialize};

/// Epoch seconds UTC (same unit as [`AvailableAt`](helio_time::AvailableAt)).
pub type UtcTs = i64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EventId(pub u64);

/// Coarse category for analytics and routing (not used in core logic).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EventKind {
    Solar,
    Weather,
    Earnings,
    Macro,
    SupplyShock,
    Other,
}

/// Where the shock is expected to matter for risk or PnL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EventScope {
    Global,
    Sector(u32),
    Region(u32),
}

/// Forecastable shock with an impact window. **Causal use:** only fields knowable at
/// `available_at` (on the wrapping [`Timed`]) may inform decisions; `impact_*` must be such
/// forecasts, not post-impact facts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventShock {
    pub id: EventId,
    pub observed_at: Option<ObservedAt>,
    /// Inclusive start of forecasted physical / economic impact (UTC epoch seconds).
    pub impact_start: UtcTs,
    /// Exclusive or inclusive end per your downstream convention; core uses it only for mid-window
    /// exit and ordering. Treat as inclusive end if you set `impact_end >= impact_start`.
    pub impact_end: UtcTs,
    pub severity: f64,
    pub confidence: f64,
    pub scope: EventScope,
    pub kind: EventKind,
}

/// `impact_start - available_at` in seconds. Core filter for tradable lead.
#[inline]
pub fn signal_lead_secs(shock: &EventShock, available_at: AvailableAt) -> i64 {
    shock.impact_start.saturating_sub(available_at.0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExitPolicy {
    AtImpactStart,
    MidImpactWindow,
    /// Exit at `entry_time + delta_secs` (e.g. ~2 sessions encoded upstream as seconds).
    FixedHorizonAfterEntry { delta_secs: i64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Exposure {
    LongVol,
    ShortVol,
    SectorPair { long_sector: u32, short_sector: u32 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventSignal {
    pub event_id: EventId,
    pub entry_time: UtcTs,
    pub exit_time: UtcTs,
    pub exposure: Exposure,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EventShockFilterConfig {
    pub min_severity: f64,
    pub min_confidence: f64,
    pub min_lead_secs: i64,
    pub max_lead_secs: i64,
}

impl Default for EventShockFilterConfig {
    fn default() -> Self {
        Self {
            min_severity: 0.0,
            min_confidence: 0.0,
            min_lead_secs: 0,
            max_lead_secs: i64::MAX,
        }
    }
}

/// Severity, confidence, and **lead-time** band (`min_lead <= impact_start - available_at <= max_lead`).
#[derive(Debug, Clone, Copy)]
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
        let lead = signal_lead_secs(v, input.available_at);
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

/// v1 entry = `available_at` instant; exit from [`ExitPolicy`]. `exposure` is fixed per scan config.
#[derive(Debug, Clone, Copy)]
pub struct EventToSignalScan {
    pub exit_policy: ExitPolicy,
    pub exposure: Exposure,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventToSignalState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventToSignalSnapshot;

impl EventToSignalScan {
    fn exit_ts(&self, shock: &EventShock, entry: UtcTs) -> UtcTs {
        match self.exit_policy {
            ExitPolicy::AtImpactStart => shock.impact_start,
            ExitPolicy::MidImpactWindow => {
                shock.impact_start + (shock.impact_end - shock.impact_start) / 2
            }
            ExitPolicy::FixedHorizonAfterEntry { delta_secs } => entry.saturating_add(delta_secs),
        }
    }
}

impl Scan for EventToSignalScan {
    type In = Timed<EventShock>;
    type Out = EventSignal;
    type State = EventToSignalState;

    fn init(&self) -> Self::State {
        EventToSignalState
    }

    fn step<E>(&self, _state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        let entry_time = input.available_at.0;
        let exit_time = self.exit_ts(&input.value, entry_time);
        emit.emit(EventSignal {
            event_id: input.value.id,
            entry_time,
            exit_time,
            exposure: self.exposure,
        });
    }
}

impl FlushableScan for EventToSignalScan {
    type Offset = u64;

    fn flush<E>(&self, _state: &mut Self::State, _signal: FlushReason<Self::Offset>, _emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
    }
}

impl SnapshottingScan for EventToSignalScan {
    type Snapshot = EventToSignalSnapshot;

    fn snapshot(&self, _state: &Self::State) -> Self::Snapshot {
        EventToSignalSnapshot
    }

    fn restore(&self, _snapshot: Self::Snapshot) -> Self::State {
        EventToSignalState
    }
}

impl VersionedSnapshot for EventToSignalSnapshot {
    const VERSION: u32 = 1;
}

/// Hook for portfolio constraints / sizing; v1 is pass-through.
#[derive(Debug, Clone, Copy, Default)]
pub struct PortfolioScan;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortfolioScanState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortfolioScanSnapshot;

impl Scan for PortfolioScan {
    type In = EventSignal;
    type Out = EventSignal;
    type State = PortfolioScanState;

    fn init(&self) -> Self::State {
        PortfolioScanState
    }

    fn step<E>(&self, _state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        emit.emit(input);
    }
}

impl FlushableScan for PortfolioScan {
    type Offset = u64;

    fn flush<E>(&self, _state: &mut Self::State, _signal: FlushReason<Self::Offset>, _emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
    }
}

impl SnapshottingScan for PortfolioScan {
    type Snapshot = PortfolioScanSnapshot;

    fn snapshot(&self, _state: &Self::State) -> Self::Snapshot {
        PortfolioScanSnapshot
    }

    fn restore(&self, _snapshot: Self::Snapshot) -> Self::State {
        PortfolioScanState
    }
}

impl VersionedSnapshot for PortfolioScanSnapshot {
    const VERSION: u32 = 1;
}

/// Ordered stream item for the kernel (alias for clarity).
pub type EventStreamItem = Timed<EventShock>;

/// **EventStream → AvailabilityGate → EventFilter → EventClockAlign → EventToSignal → PortfolioScan**
#[derive(Debug, Clone)]
pub struct EventShockKernelScan {
    pub gate: AvailabilityGateScan<EventShock>,
    pub filter: EventShockFilterScan,
    pub align: SessionAlignScan<EventShock>,
    pub to_signal: EventToSignalScan,
    pub portfolio: PortfolioScan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventShockKernelState {
    pub gate: helio_time::AvailabilityGateState,
    pub filter: EventShockFilterState,
    pub align: helio_time::SessionAlignState,
    pub to_signal: EventToSignalState,
    pub portfolio: PortfolioScanState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventShockKernelSnapshot {
    pub gate: helio_time::AvailabilityGateSnapshot,
    pub filter: EventShockFilterSnapshot,
    pub align: helio_time::SessionAlignSnapshot,
    pub to_signal: EventToSignalSnapshot,
    pub portfolio: PortfolioScanSnapshot,
}

impl EventShockKernelScan {
    pub fn new(
        decision_available: Option<AvailableAt>,
        session: SessionDate,
        filter: EventShockFilterConfig,
        to_signal: EventToSignalScan,
    ) -> Self {
        Self {
            gate: AvailabilityGateScan::new(decision_available),
            filter: EventShockFilterScan { cfg: filter },
            align: SessionAlignScan::new(session),
            to_signal,
            portfolio: PortfolioScan,
        }
    }
}

impl Scan for EventShockKernelScan {
    type In = EventStreamItem;
    type Out = EventSignal;
    type State = EventShockKernelState;

    fn init(&self) -> Self::State {
        EventShockKernelState {
            gate: self.gate.init(),
            filter: self.filter.init(),
            align: self.align.init(),
            to_signal: self.to_signal.init(),
            portfolio: self.portfolio.init(),
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
                let mut c = helio_scan::VecEmitter::new();
                self.align.step(&mut state.align, y, &mut c);
                for z in c.into_inner() {
                    let mut d = helio_scan::VecEmitter::new();
                    self.to_signal.step(&mut state.to_signal, z, &mut d);
                    for sig in d.into_inner() {
                        self.portfolio.step(&mut state.portfolio, sig, emit);
                    }
                }
            }
        }
    }
}

impl FlushableScan for EventShockKernelScan {
    type Offset = u64;

    fn flush<E>(&self, state: &mut Self::State, signal: FlushReason<Self::Offset>, _emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        let mut e_gate = helio_scan::VecEmitter::new();
        self.gate.flush(&mut state.gate, signal.clone(), &mut e_gate);
        let mut e_filter = helio_scan::VecEmitter::new();
        self.filter
            .flush(&mut state.filter, signal.clone(), &mut e_filter);
        let mut e_align = helio_scan::VecEmitter::new();
        self.align.flush(&mut state.align, signal.clone(), &mut e_align);
        let mut e_sig = helio_scan::VecEmitter::new();
        self.to_signal
            .flush(&mut state.to_signal, signal.clone(), &mut e_sig);
        let mut e_port = helio_scan::VecEmitter::new();
        self.portfolio.flush(&mut state.portfolio, signal, &mut e_port);
    }
}

impl SnapshottingScan for EventShockKernelScan {
    type Snapshot = EventShockKernelSnapshot;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        EventShockKernelSnapshot {
            gate: self.gate.snapshot(&state.gate),
            filter: self.filter.snapshot(&state.filter),
            align: self.align.snapshot(&state.align),
            to_signal: self.to_signal.snapshot(&state.to_signal),
            portfolio: self.portfolio.snapshot(&state.portfolio),
        }
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        EventShockKernelState {
            gate: self.gate.restore(snapshot.gate),
            filter: self.filter.restore(snapshot.filter),
            align: self.align.restore(snapshot.align),
            to_signal: self.to_signal.restore(snapshot.to_signal),
            portfolio: self.portfolio.restore(snapshot.portfolio),
        }
    }
}

impl VersionedSnapshot for EventShockKernelSnapshot {
    const VERSION: u32 = 1;
}

#[cfg(test)]
mod tests {
    use super::*;
    use helio_scan::VecEmitter;

    fn shock(impact_start: UtcTs, impact_end: UtcTs, sev: f64) -> EventShock {
        EventShock {
            id: EventId(1),
            observed_at: None,
            impact_start,
            impact_end,
            severity: sev,
            confidence: 1.0,
            scope: EventScope::Global,
            kind: EventKind::Solar,
        }
    }

    #[test]
    fn lead_filter_drops_short_and_long_lead() {
        let f = EventShockFilterScan {
            cfg: EventShockFilterConfig {
                min_severity: 0.5,
                min_confidence: 0.0,
                min_lead_secs: 100,
                max_lead_secs: 1000,
            },
        };
        let mut st = f.init();
        let mut e = VecEmitter::new();
        // lead = 50
        f.step(
            &mut st,
            Timed {
                value: shock(150, 200, 1.0),
                observed_at: None,
                available_at: AvailableAt(100),
                effective_at: None,
                session_date: None,
            },
            &mut e,
        );
        assert!(e.0.is_empty());
        // lead = 2000
        f.step(
            &mut st,
            Timed {
                value: shock(3000, 3100, 1.0),
                observed_at: None,
                available_at: AvailableAt(1000),
                effective_at: None,
                session_date: None,
            },
            &mut e,
        );
        assert!(e.0.is_empty());
        // lead = 500 OK
        f.step(
            &mut st,
            Timed {
                value: shock(1500, 1600, 1.0),
                observed_at: None,
                available_at: AvailableAt(1000),
                effective_at: None,
                session_date: None,
            },
            &mut e,
        );
        assert_eq!(e.0.len(), 1);
    }

    #[test]
    fn kernel_respects_availability_gate() {
        let kernel = EventShockKernelScan::new(
            Some(AvailableAt(1000)),
            SessionDate(5),
            EventShockFilterConfig::default(),
            EventToSignalScan {
                exit_policy: ExitPolicy::AtImpactStart,
                exposure: Exposure::LongVol,
            },
        );
        let mut st = kernel.init();
        let mut e = VecEmitter::new();
        // Future availability: decision clock 1000 cannot use information available only at 1500.
        let item = Timed {
            value: shock(2000, 3000, 1.0),
            observed_at: None,
            available_at: AvailableAt(1500),
            effective_at: None,
            session_date: None,
        };
        kernel.step(&mut st, item, &mut e);
        assert!(e.0.is_empty());

        let item2 = Timed {
            value: shock(2000, 3000, 1.0),
            observed_at: None,
            available_at: AvailableAt(1000),
            effective_at: None,
            session_date: None,
        };
        kernel.step(&mut st, item2, &mut e);
        assert_eq!(e.0.len(), 1);
        assert_eq!(e.0[0].entry_time, 1000);
        assert_eq!(e.0[0].exit_time, 2000);
        assert_eq!(e.0[0].exposure, Exposure::LongVol);
        assert_eq!(e.0[0].event_id, EventId(1));
    }
}
