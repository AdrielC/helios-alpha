use helio_scan::{Emit, FlushReason, FlushableScan, Scan, SnapshottingScan, VersionedSnapshot};
use serde::{Deserialize, Serialize};

/// Mixed stream of bars (one row per trading session day) and treatment definitions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HorizonInput {
    Bar { session_day: i32, close: f64 },
    Treatment { id: u32, horizon_trading_days: u32 },
}

/// Completed forward window (simple return from entry close to exit close).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForwardHorizonOutcome {
    pub treatment_id: u32,
    pub entry_session_day: i32,
    pub exit_session_day: i32,
    pub simple_return: f64,
}

/// Emitted when a horizon is cut short by flush (still useful for replay tests).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForwardHorizonIncomplete {
    pub treatment_id: u32,
    pub entry_session_day: i32,
    pub last_session_day: i32,
    pub last_close: f64,
    pub simple_return: f64,
    pub bars_remaining: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingTreatment {
    pub id: u32,
    pub horizon_trading_days: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActiveHorizon {
    pub id: u32,
    pub entry_session_day: i32,
    pub entry_close: f64,
    pub bars_remaining: u32,
}

/// Tracks treatments, attaches them on the **next** bar, decrements horizon per bar, finalizes on 0.
///
/// **Session policy:** each `Bar` is one trading day; `horizon_trading_days` counts bars after
/// attachment. [`FlushReason::SessionClose`] emits [`ForwardHorizonIncomplete`] for open windows
/// (using last seen bar).
#[derive(Debug, Clone, Copy, Default)]
pub struct ForwardHorizonScan;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForwardHorizonState {
    pub pending: Vec<PendingTreatment>,
    pub active: Vec<ActiveHorizon>,
    pub last_session_day: Option<i32>,
    pub last_close: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForwardHorizonSnapshot {
    pub pending: Vec<PendingTreatment>,
    pub active: Vec<ActiveHorizon>,
    pub last_session_day: Option<i32>,
    pub last_close: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ForwardHorizonOutput {
    Complete(ForwardHorizonOutcome),
    Incomplete(ForwardHorizonIncomplete),
}

impl Scan for ForwardHorizonScan {
    type In = HorizonInput;
    type Out = ForwardHorizonOutput;
    type State = ForwardHorizonState;

    fn init(&self) -> Self::State {
        ForwardHorizonState {
            pending: Vec::new(),
            active: Vec::new(),
            last_session_day: None,
            last_close: None,
        }
    }

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        match input {
            HorizonInput::Treatment {
                id,
                horizon_trading_days,
            } => {
                state.pending.push(PendingTreatment {
                    id,
                    horizon_trading_days,
                });
            }
            HorizonInput::Bar { session_day, close } => {
                state.last_session_day = Some(session_day);
                state.last_close = Some(close);
                for p in state.pending.drain(..) {
                    state.active.push(ActiveHorizon {
                        id: p.id,
                        entry_session_day: session_day,
                        entry_close: close,
                        bars_remaining: p.horizon_trading_days,
                    });
                }
                let mut i = 0;
                while i < state.active.len() {
                    state.active[i].bars_remaining =
                        state.active[i].bars_remaining.saturating_sub(1);
                    if state.active[i].bars_remaining == 0 {
                        let a = state.active.swap_remove(i);
                        let simple_return = close / a.entry_close - 1.0;
                        emit.emit(ForwardHorizonOutput::Complete(ForwardHorizonOutcome {
                            treatment_id: a.id,
                            entry_session_day: a.entry_session_day,
                            exit_session_day: session_day,
                            simple_return,
                        }));
                    } else {
                        i += 1;
                    }
                }
            }
        }
    }
}

impl ForwardHorizonScan {
    fn flush_incomplete<E: Emit<ForwardHorizonOutput>>(
        state: &mut ForwardHorizonState,
        emit: &mut E,
    ) {
        let (ls, lc) = match (state.last_session_day, state.last_close) {
            (Some(d), Some(c)) => (d, c),
            _ => return,
        };
        for a in state.active.drain(..) {
            let simple_return = lc / a.entry_close - 1.0;
            emit.emit(ForwardHorizonOutput::Incomplete(ForwardHorizonIncomplete {
                treatment_id: a.id,
                entry_session_day: a.entry_session_day,
                last_session_day: ls,
                last_close: lc,
                simple_return,
                bars_remaining: a.bars_remaining,
            }));
        }
    }
}

impl FlushableScan for ForwardHorizonScan {
    type Offset = u64;

    fn flush<E>(&self, state: &mut Self::State, signal: FlushReason<Self::Offset>, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        match signal {
            FlushReason::SessionClose(_) => Self::flush_incomplete(state, emit),
            FlushReason::EndOfInput | FlushReason::Shutdown | FlushReason::Manual => {
                Self::flush_incomplete(state, emit);
                state.pending.clear();
            }
            _ => {}
        }
    }
}

impl SnapshottingScan for ForwardHorizonScan {
    type Snapshot = ForwardHorizonSnapshot;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        ForwardHorizonSnapshot {
            pending: state.pending.clone(),
            active: state.active.clone(),
            last_session_day: state.last_session_day,
            last_close: state.last_close,
        }
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        ForwardHorizonState {
            pending: snapshot.pending,
            active: snapshot.active,
            last_session_day: snapshot.last_session_day,
            last_close: snapshot.last_close,
        }
    }
}

impl VersionedSnapshot for ForwardHorizonSnapshot {
    const VERSION: u32 = 1;
}

#[cfg(test)]
mod tests {
    use super::*;
    use helio_scan::{SessionDate, VecEmitter};

    #[test]
    fn session_close_flushes_incomplete() {
        let s = ForwardHorizonScan;
        let mut st = s.init();
        let mut e = VecEmitter::new();
        s.step(
            &mut st,
            HorizonInput::Bar {
                session_day: 1,
                close: 100.0,
            },
            &mut e,
        );
        s.step(
            &mut st,
            HorizonInput::Treatment {
                id: 1,
                horizon_trading_days: 5,
            },
            &mut e,
        );
        s.step(
            &mut st,
            HorizonInput::Bar {
                session_day: 2,
                close: 102.0,
            },
            &mut e,
        );
        s.flush(&mut st, FlushReason::SessionClose(SessionDate(2)), &mut e);
        assert_eq!(e.0.len(), 1);
        match &e.0[0] {
            ForwardHorizonOutput::Incomplete(i) => {
                assert_eq!(i.bars_remaining, 4);
                assert_eq!(i.last_session_day, 2);
            }
            _ => panic!("expected incomplete"),
        }
    }
}
