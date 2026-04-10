//! Deterministic matched controls on **session entry** (weekday match, exclude impact windows).

use helio_scan::{
    Emit, FlushReason, FlushableScan, Scan, SessionDate, SnapshottingScan, VersionedSnapshot,
};
use helio_time::{utc_weekday_for_ts, SimpleWeekdayCalendar, TradingCalendar};
use serde::{Deserialize, Serialize};
use std::marker::PhantomData;

use crate::{AlignedEventShock, EventId, EventShockSignal, Exposure, Symbol};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventShockControlConfig {
    pub seed: u64,
    pub controls_per_treatment: u32,
    /// Same label as treatment signals (for reporting).
    pub strategy_name: String,
    /// Holding length in trading sessions (same as treatment signal horizon).
    pub horizon_sessions: u32,
    pub exposure: Exposure,
    /// Optional rough vol filter: require |log(close/close_prev) - treatment_vol| <= band (requires bar map in config — v1 omit, use `vol_match_epsilon` only when treatment vol passed per event; here we skip vol if `vol_epsilon` is None).
    pub vol_epsilon: Option<f64>,
}

impl Default for EventShockControlConfig {
    fn default() -> Self {
        Self {
            seed: 42,
            controls_per_treatment: 1,
            strategy_name: String::new(),
            horizon_sessions: 5,
            exposure: Exposure::Pair {
                long: Symbol("XLU".into()),
                short: Symbol("SPY".into()),
            },
            vol_epsilon: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventShockControlSamplerState {
    pub excluded: Vec<(SessionDate, SessionDate)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventShockControlSamplerSnapshot {
    pub excluded: Vec<(SessionDate, SessionDate)>,
}

#[inline]
fn session_weekday(d: SessionDate) -> i32 {
    utc_weekday_for_ts(d.0 as i64 * 86_400)
}

fn overlaps_excluded(s: SessionDate, ex: &[(SessionDate, SessionDate)]) -> bool {
    for &(a, b) in ex {
        let lo = a.0.min(b.0);
        let hi = a.0.max(b.0);
        if s.0 >= lo && s.0 <= hi {
            return true;
        }
    }
    false
}

/// LCG step for deterministic pseudo-random indexing.
#[inline]
fn mix(seed: u64, a: u64, b: u32) -> u64 {
    seed.wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407)
        .wrapping_add(a)
        .wrapping_add(b as u64)
}

#[derive(Debug, Clone)]
pub struct EventShockControlSamplerScan<C: TradingCalendar + Copy = SimpleWeekdayCalendar> {
    pub cfg: EventShockControlConfig,
    pub calendar: C,
    pub candidate_entries: Vec<SessionDate>,
    _p: PhantomData<C>,
}

impl<C: TradingCalendar + Copy> EventShockControlSamplerScan<C> {
    pub fn new(
        cfg: EventShockControlConfig,
        calendar: C,
        candidate_entries: Vec<SessionDate>,
    ) -> Self {
        Self {
            cfg,
            calendar,
            candidate_entries,
            _p: PhantomData,
        }
    }
}

impl<C: TradingCalendar + Copy> Scan for EventShockControlSamplerScan<C> {
    type In = AlignedEventShock;
    type Out = EventShockSignal;
    type State = EventShockControlSamplerState;

    fn init(&self) -> Self::State {
        EventShockControlSamplerState {
            excluded: Vec::new(),
        }
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        let lo = input.impact_start_session;
        let hi = input.impact_end_session;
        let w = lo.0.min(hi.0);
        let z = lo.0.max(hi.0);
        state.excluded.push((SessionDate(w), SessionDate(z)));

        let target_wd = session_weekday(input.entry_session);
        let n = self.candidate_entries.len();
        if n == 0 {
            return;
        }

        for k in 0..self.cfg.controls_per_treatment {
            let start = mix(self.cfg.seed, input.event_id.0, k) as usize % n;
            let mut found: Option<SessionDate> = None;
            for j in 0..n {
                let idx = (start + j) % n;
                let c = self.candidate_entries[idx];
                if c == input.entry_session {
                    continue;
                }
                if session_weekday(c) != target_wd {
                    continue;
                }
                if overlaps_excluded(c, &state.excluded) {
                    continue;
                }
                found = Some(c);
                break;
            }
            if let Some(entry) = found {
                let exit = self.calendar.add_sessions(entry, self.cfg.horizon_sessions);
                let ctrl_id = EventId(
                    input
                        .event_id
                        .0
                        .wrapping_mul(1_000_003)
                        .wrapping_add(k as u64 + 1),
                );
                emit.emit(EventShockSignal {
                    event_id: ctrl_id,
                    entry_session: entry,
                    exit_session: exit,
                    exposure: self.cfg.exposure.clone(),
                    strategy_name: self.cfg.strategy_name.clone(),
                    scope: input.scope.clone(),
                    matched_treatment: Some(input.event_id),
                });
            }
        }
    }
}

impl<C: TradingCalendar + Copy> FlushableScan for EventShockControlSamplerScan<C> {
    type Offset = u64;

    fn flush<E>(&self, _state: &mut Self::State, _signal: FlushReason<Self::Offset>, _emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
    }
}

impl<C: TradingCalendar + Copy> SnapshottingScan for EventShockControlSamplerScan<C> {
    type Snapshot = EventShockControlSamplerSnapshot;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        EventShockControlSamplerSnapshot {
            excluded: state.excluded.clone(),
        }
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        EventShockControlSamplerState {
            excluded: snapshot.excluded,
        }
    }
}

impl VersionedSnapshot for EventShockControlSamplerSnapshot {
    const VERSION: u32 = 1;
}
