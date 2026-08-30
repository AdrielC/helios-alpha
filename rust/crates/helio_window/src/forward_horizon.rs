use helio_scan::{Emit, FlushReason, FlushableScan, Scan, SnapshottingScan, VersionedSnapshot};
use helio_time::WindowSpec;
use serde::{Deserialize, Serialize};

// Horizon length is `Frequency::Samples` / session steps in config; full time-keyed eviction is future work.

/// Mixed stream of bars (one row per trading session day) and treatment definitions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HorizonInput {
    Bar {
        session_day: i32,
        close: f64,
        available_at: i64,
    },
    Treatment {
        id: u32,
        horizon_trading_days: u32,
        available_at: i64,
    },
}

/// Completed forward window (simple return from entry close to exit close).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForwardHorizonOutcome {
    pub treatment_id: u32,
    pub entry_session_day: i32,
    pub exit_session_day: i32,
    pub entry_available_at: i64,
    pub exit_available_at: i64,
    pub simple_return: f64,
}

/// Emitted when a horizon is cut short by flush (still useful for replay tests).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForwardHorizonIncomplete {
    pub treatment_id: u32,
    pub entry_session_day: i32,
    pub last_session_day: i32,
    pub last_close: f64,
    pub entry_available_at: i64,
    pub last_available_at: i64,
    pub simple_return: f64,
    pub bars_remaining: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingTreatment {
    pub id: u32,
    pub horizon_trading_days: u32,
    pub available_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActiveHorizon {
    pub id: u32,
    pub entry_session_day: i32,
    pub entry_close: f64,
    pub entry_available_at: i64,
    pub bars_remaining: u32,
}

/// Tracks treatments, attaches them on the first bar whose `available_at` is **strictly after** the
/// treatment, decrements existing horizons before attaching new ones, and finalizes on 0.
///
/// **Session policy:** each `Bar` is one trading day; `horizon_trading_days` counts bars after
/// attachment. [`FlushReason::SessionClose`] emits [`ForwardHorizonIncomplete`] for open windows
/// (using last seen bar).
///
/// For semantic alignment with the time kernel, interpret a horizon of *n* bars as
/// [`helio_time::Frequency::Samples`] inside [`WindowSpec::Trailing`]. Time-keyed eviction TBD.
#[derive(Debug, Clone)]
pub struct ForwardHorizonScan {
    /// Documentary / future use: trailing horizon semantics in unified spec form.
    pub window_spec: WindowSpec,
}

impl Default for ForwardHorizonScan {
    fn default() -> Self {
        Self {
            window_spec: WindowSpec::trailing_samples(1),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForwardHorizonState {
    pub pending: Vec<PendingTreatment>,
    pub active: Vec<ActiveHorizon>,
    pub last_session_day: Option<i32>,
    pub last_close: Option<f64>,
    pub last_available_at: Option<i64>,
    /// Regressing records are rejected rather than being allowed to rewrite an earlier decision.
    pub rejected_time_regressions: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForwardHorizonSnapshot {
    pub pending: Vec<PendingTreatment>,
    pub active: Vec<ActiveHorizon>,
    pub last_session_day: Option<i32>,
    pub last_close: Option<f64>,
    pub last_available_at: Option<i64>,
    pub rejected_time_regressions: u64,
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
            last_available_at: None,
            rejected_time_regressions: 0,
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
                available_at,
            } => {
                if state
                    .last_available_at
                    .is_some_and(|frontier| available_at < frontier)
                {
                    state.rejected_time_regressions =
                        state.rejected_time_regressions.saturating_add(1);
                    return;
                }
                state.last_available_at = Some(available_at);
                state.pending.push(PendingTreatment {
                    id,
                    horizon_trading_days: horizon_trading_days.max(1),
                    available_at,
                });
            }
            HorizonInput::Bar {
                session_day,
                close,
                available_at,
            } => {
                if state
                    .last_available_at
                    .is_some_and(|frontier| available_at < frontier)
                {
                    state.rejected_time_regressions =
                        state.rejected_time_regressions.saturating_add(1);
                    return;
                }
                state.last_session_day = Some(session_day);
                state.last_close = Some(close);
                state.last_available_at = Some(available_at);

                // Existing windows consume this bar. A treatment cannot consume its own entry bar,
                // so eligible pending rows are attached only after this pass.
                state.active.retain_mut(|a| {
                    a.bars_remaining = a.bars_remaining.saturating_sub(1);
                    if a.bars_remaining == 0 {
                        let simple_return = close / a.entry_close - 1.0;
                        emit.emit(ForwardHorizonOutput::Complete(ForwardHorizonOutcome {
                            treatment_id: a.id,
                            entry_session_day: a.entry_session_day,
                            exit_session_day: session_day,
                            entry_available_at: a.entry_available_at,
                            exit_available_at: available_at,
                            simple_return,
                        }));
                        false
                    } else {
                        true
                    }
                });

                let mut waiting = Vec::with_capacity(state.pending.len());
                for p in state.pending.drain(..) {
                    if p.available_at < available_at {
                        state.active.push(ActiveHorizon {
                            id: p.id,
                            entry_session_day: session_day,
                            entry_close: close,
                            entry_available_at: available_at,
                            bars_remaining: p.horizon_trading_days,
                        });
                    } else {
                        waiting.push(p);
                    }
                }
                state.pending = waiting;
            }
        }
    }
}

impl ForwardHorizonScan {
    fn flush_incomplete<E: Emit<ForwardHorizonOutput>>(
        state: &mut ForwardHorizonState,
        emit: &mut E,
    ) {
        let (ls, lc, la) = match (
            state.last_session_day,
            state.last_close,
            state.last_available_at,
        ) {
            (Some(d), Some(c), Some(a)) => (d, c, a),
            _ => return,
        };
        for a in state.active.drain(..) {
            let simple_return = lc / a.entry_close - 1.0;
            emit.emit(ForwardHorizonOutput::Incomplete(ForwardHorizonIncomplete {
                treatment_id: a.id,
                entry_session_day: a.entry_session_day,
                last_session_day: ls,
                last_close: lc,
                entry_available_at: a.entry_available_at,
                last_available_at: la,
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
            last_available_at: state.last_available_at,
            rejected_time_regressions: state.rejected_time_regressions,
        }
    }

    fn restore(&self, snapshot: Self::Snapshot) -> Self::State {
        ForwardHorizonState {
            pending: snapshot.pending,
            active: snapshot.active,
            last_session_day: snapshot.last_session_day,
            last_close: snapshot.last_close,
            last_available_at: snapshot.last_available_at,
            rejected_time_regressions: snapshot.rejected_time_regressions,
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
        let s = ForwardHorizonScan::default();
        let mut st = s.init();
        let mut e = VecEmitter::new();
        s.step(
            &mut st,
            HorizonInput::Bar {
                session_day: 1,
                close: 100.0,
                available_at: 10,
            },
            &mut e,
        );
        s.step(
            &mut st,
            HorizonInput::Treatment {
                id: 1,
                horizon_trading_days: 5,
                available_at: 11,
            },
            &mut e,
        );
        s.step(
            &mut st,
            HorizonInput::Bar {
                session_day: 2,
                close: 102.0,
                available_at: 20,
            },
            &mut e,
        );
        s.flush(&mut st, FlushReason::SessionClose(SessionDate(2)), &mut e);
        assert_eq!(e.0.len(), 1);
        match &e.0[0] {
            ForwardHorizonOutput::Incomplete(i) => {
                assert_eq!(i.bars_remaining, 5);
                assert_eq!(i.last_session_day, 2);
            }
            _ => panic!("expected incomplete"),
        }
    }

    #[test]
    fn simultaneous_completions_preserve_treatment_order() {
        let scan = ForwardHorizonScan::default();
        let mut state = scan.init();
        let mut emit = VecEmitter::new();

        for id in [10, 20, 30] {
            scan.step(
                &mut state,
                HorizonInput::Treatment {
                    id,
                    horizon_trading_days: 1,
                    available_at: 10,
                },
                &mut emit,
            );
        }
        scan.step(
            &mut state,
            HorizonInput::Bar {
                session_day: 1,
                close: 100.0,
                available_at: 20,
            },
            &mut emit,
        );

        scan.step(
            &mut state,
            HorizonInput::Bar {
                session_day: 2,
                close: 101.0,
                available_at: 30,
            },
            &mut emit,
        );

        let ids: Vec<_> = emit
            .0
            .iter()
            .map(|output| match output {
                ForwardHorizonOutput::Complete(outcome) => outcome.treatment_id,
                ForwardHorizonOutput::Incomplete(_) => panic!("unexpected incomplete horizon"),
            })
            .collect();
        assert_eq!(ids, vec![10, 20, 30]);
    }

    #[test]
    fn treatment_never_attaches_to_a_bar_at_the_same_availability_instant() {
        let scan = ForwardHorizonScan::default();
        let mut state = scan.init();
        let mut emit = VecEmitter::new();
        scan.step(
            &mut state,
            HorizonInput::Treatment {
                id: 7,
                horizon_trading_days: 1,
                available_at: 20,
            },
            &mut emit,
        );
        scan.step(
            &mut state,
            HorizonInput::Bar {
                session_day: 1,
                close: 100.0,
                available_at: 20,
            },
            &mut emit,
        );
        assert!(state.active.is_empty());
        assert_eq!(state.pending.len(), 1);

        scan.step(
            &mut state,
            HorizonInput::Bar {
                session_day: 2,
                close: 101.0,
                available_at: 21,
            },
            &mut emit,
        );
        scan.step(
            &mut state,
            HorizonInput::Bar {
                session_day: 3,
                close: 103.0,
                available_at: 22,
            },
            &mut emit,
        );
        match &emit.0[0] {
            ForwardHorizonOutput::Complete(outcome) => {
                assert_eq!(outcome.entry_session_day, 2);
                assert_eq!(outcome.exit_session_day, 3);
                assert_eq!(outcome.entry_available_at, 21);
                assert_eq!(outcome.exit_available_at, 22);
            }
            _ => panic!("expected complete horizon"),
        }
    }

    #[test]
    fn regressing_treatment_is_rejected_after_later_market_data() {
        let scan = ForwardHorizonScan::default();
        let mut state = scan.init();
        let mut emit = VecEmitter::new();
        scan.step(
            &mut state,
            HorizonInput::Bar {
                session_day: 5,
                close: 100.0,
                available_at: 50,
            },
            &mut emit,
        );
        scan.step(
            &mut state,
            HorizonInput::Treatment {
                id: 9,
                horizon_trading_days: 1,
                available_at: 40,
            },
            &mut emit,
        );
        assert!(state.pending.is_empty());
        assert_eq!(state.rejected_time_regressions, 1);
    }
}
