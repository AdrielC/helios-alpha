//! Daily-bar execution simulation for [`EventShockSignal`](crate::EventShockSignal).

use helio_scan::{
    Emit, FlushReason, FlushableScan, Scan, SessionDate, SnapshottingScan, VersionedSnapshot,
};
use helio_time::{SimpleWeekdayCalendar, TradingCalendar};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::marker::PhantomData;

use crate::{EventShockSignal, Exposure, Symbol, TradeResult};

/// Where to price the **opening** leg of a simulated hold (deterministic, no slippage).
///
/// Spec default: **next session open** after the aligned entry session (i.e. after
/// [`TradingCalendar::first_session_strictly_after_ts`](helio_time::TradingCalendar) produced
/// `entry_session`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ExecutionEntryTiming {
    /// Use **open** of the session **after** `signal.entry_session`.
    #[default]
    NextSessionOpen,
    /// Use **open** of `signal.entry_session` (legacy alignment).
    EntrySessionOpen,
}

/// One row per symbol per session (UTC-aligned session index).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DailyBar {
    pub session: SessionDate,
    pub symbol: Symbol,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EventShockReplayRecord {
    Signal(EventShockSignal),
    Bar(DailyBar),
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EventShockExecutionState {
    pending: Vec<EventShockSignal>,
    bars: HashMap<(SessionDate, String), DailyBar>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventShockExecutionSnapshot {
    pub pending: Vec<EventShockSignal>,
    pub bars: Vec<DailyBar>,
}

fn sym_key(s: &Symbol) -> String {
    s.0.clone()
}

fn symbols_in_exposure(ex: &Exposure) -> Vec<Symbol> {
    match ex {
        Exposure::Long(a) => vec![a.clone()],
        Exposure::Short(a) => vec![a.clone()],
        Exposure::Pair { long, short } => vec![long.clone(), short.clone()],
    }
}

fn bar_close(state: &EventShockExecutionState, session: SessionDate, sym: &Symbol) -> Option<f64> {
    state.bars.get(&(session, sym_key(sym))).map(|b| b.close)
}

fn bar_open(state: &EventShockExecutionState, session: SessionDate, sym: &Symbol) -> Option<f64> {
    state.bars.get(&(session, sym_key(sym))).map(|b| b.open)
}

/// Deterministic daily simulation: enter at **open** (see [`ExecutionEntryTiming`]), exit at **close** of `exit_session`.
#[derive(Debug, Clone, Copy)]
pub struct SignalExecutionScan<C: TradingCalendar + Copy = SimpleWeekdayCalendar> {
    pub calendar: C,
    pub entry_timing: ExecutionEntryTiming,
    _p: PhantomData<C>,
}

impl<C: TradingCalendar + Copy> SignalExecutionScan<C> {
    pub fn new(calendar: C) -> Self {
        Self::with_timing(calendar, ExecutionEntryTiming::default())
    }

    pub fn with_timing(calendar: C, entry_timing: ExecutionEntryTiming) -> Self {
        Self {
            calendar,
            entry_timing,
            _p: PhantomData,
        }
    }

    fn entry_price_session(&self, entry_session: SessionDate) -> SessionDate {
        match self.entry_timing {
            ExecutionEntryTiming::NextSessionOpen => self.calendar.next_session_after(entry_session),
            ExecutionEntryTiming::EntrySessionOpen => entry_session,
        }
    }
}

impl<C: TradingCalendar + Copy> SignalExecutionScan<C> {
    fn try_execute(
        &self,
        state: &EventShockExecutionState,
        sig: &EventShockSignal,
    ) -> Option<TradeResult> {
        let e = sig.entry_session;
        let x = sig.exit_session;
        if x.0 < e.0 {
            return None;
        }
        let e_px = self.entry_price_session(e);
        let gross = match &sig.exposure {
            Exposure::Long(sym) => {
                let o = bar_open(state, e_px, sym)?;
                let c = bar_close(state, x, sym)?;
                if o == 0.0 {
                    return None;
                }
                c / o - 1.0
            }
            Exposure::Short(sym) => {
                let o = bar_open(state, e_px, sym)?;
                let c = bar_close(state, x, sym)?;
                if c == 0.0 {
                    return None;
                }
                o / c - 1.0
            }
            Exposure::Pair { long, short } => {
                let ol = bar_open(state, e_px, long)?;
                let cl = bar_close(state, x, long)?;
                let os = bar_open(state, e_px, short)?;
                let cs = bar_close(state, x, short)?;
                if ol == 0.0 || os == 0.0 {
                    return None;
                }
                (cl / ol - 1.0) - (cs / os - 1.0)
            }
        };
        let mdd = self.max_drawdown(state, sig)?;
        let holding = self.calendar.inclusive_session_count(e, x);
        Some(TradeResult {
            event_id: sig.event_id,
            entry_session: e,
            exit_session: x,
            gross_return: gross,
            max_drawdown: mdd,
            holding_period_sessions: holding,
            matched_treatment: sig.matched_treatment,
        })
    }

    fn max_drawdown(
        &self,
        state: &EventShockExecutionState,
        sig: &EventShockSignal,
    ) -> Option<f64> {
        let e = sig.entry_session;
        let x = sig.exit_session;
        let e_px = self.entry_price_session(e);
        let mut peak = f64::NEG_INFINITY;
        let mut max_dd = 0.0f64;
        let mut d = e_px;
        loop {
            let mark = match &sig.exposure {
                Exposure::Long(sym) => {
                    let o = bar_open(state, e_px, sym)?;
                    let c = bar_close(state, d, sym)?;
                    if o == 0.0 {
                        return None;
                    }
                    c / o
                }
                Exposure::Short(sym) => {
                    let o = bar_open(state, e_px, sym)?;
                    let c = bar_close(state, d, sym)?;
                    if c == 0.0 {
                        return None;
                    }
                    o / c
                }
                Exposure::Pair { long, short } => {
                    let ol = bar_open(state, e_px, long)?;
                    let cl = bar_close(state, d, long)?;
                    let os = bar_open(state, e_px, short)?;
                    let cs = bar_close(state, d, short)?;
                    if ol == 0.0 || os == 0.0 {
                        return None;
                    }
                    (cl / ol) / (cs / os)
                }
            };
            if mark > peak {
                peak = mark;
            }
            let dd = if peak > 0.0 {
                (peak - mark) / peak
            } else {
                0.0
            };
            if dd > max_dd {
                max_dd = dd;
            }
            if d == x {
                break;
            }
            d = self.calendar.next_session_after(d);
            if d.0 > x.0 {
                break;
            }
        }
        Some(max_dd)
    }
}

impl<C: TradingCalendar + Copy> Scan for SignalExecutionScan<C> {
    type In = EventShockReplayRecord;
    type Out = TradeResult;
    type State = EventShockExecutionState;

    fn init(&self) -> Self::State {
        EventShockExecutionState::default()
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        match input {
            EventShockReplayRecord::Signal(sig) => {
                state.pending.push(sig);
                self.drain_ready(state, emit);
            }
            EventShockReplayRecord::Bar(b) => {
                let k = (b.session, sym_key(&b.symbol));
                state.bars.insert(k, b);
                self.drain_ready(state, emit);
            }
        }
    }
}

impl<C: TradingCalendar + Copy> SignalExecutionScan<C> {
    fn drain_ready<E: Emit<TradeResult>>(
        &self,
        state: &mut EventShockExecutionState,
        emit: &mut E,
    ) {
        let mut i = 0;
        while i < state.pending.len() {
            if let Some(tr) = self.try_execute(state, &state.pending[i]) {
                let _ = state.pending.swap_remove(i);
                emit.emit(tr);
            } else {
                i += 1;
            }
        }
    }
}

impl<C: TradingCalendar + Copy> FlushableScan for SignalExecutionScan<C> {
    type Offset = u64;

    fn flush<E>(&self, _state: &mut Self::State, _signal: FlushReason<Self::Offset>, _emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
    }
}

impl<C: TradingCalendar + Copy> SnapshottingScan for SignalExecutionScan<C> {
    type Snapshot = EventShockExecutionSnapshot;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        EventShockExecutionSnapshot {
            pending: state.pending.clone(),
            bars: state.bars.values().cloned().collect(),
        }
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        let mut bars = HashMap::new();
        for b in snapshot.bars {
            bars.insert((b.session, sym_key(&b.symbol)), b);
        }
        EventShockExecutionState {
            pending: snapshot.pending,
            bars,
        }
    }
}

impl VersionedSnapshot for EventShockExecutionSnapshot {
    const VERSION: u32 = 1;
}

/// Preload bars for symbols referenced by `signals` (caller supplies iterator in session order).
pub fn collect_required_symbols(signals: &[EventShockSignal]) -> std::collections::HashSet<String> {
    let mut s = std::collections::HashSet::new();
    for sig in signals {
        for sym in symbols_in_exposure(&sig.exposure) {
            s.insert(sym_key(&sym));
        }
    }
    s
}
