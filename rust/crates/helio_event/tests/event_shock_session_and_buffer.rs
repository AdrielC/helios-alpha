//! Session calendar validation and execution pending-queue cap.

use helio_event::*;
use helio_scan::{Scan, SessionDate, SnapshottingScan, VecEmitter};
use helio_time::{AvailableAt, SimpleWeekdayCalendar};

#[test]
fn validate_rejects_weekend_raw_bar_session_when_shock_rolls_forward() {
    let cal = SimpleWeekdayCalendar;
    // Saturday 1970-01-03 UTC = calendar day 2 (weekend)
    let shocks = vec![EventShock {
        id: EventId(1),
        kind: EventKind::default(),
        tags: String::new(),
        observed_at: None,
        available_at: AvailableAt(172_800),
        impact_start: 172_800,
        impact_end: 260_000,
        severity: 1.0,
        confidence: 1.0,
        scope: EventScope::Global,
    }];
    let bars = vec![DailyBar {
        session: SessionDate(2),
        symbol: Symbol("SPY".into()),
        open: 1.0,
        high: 1.0,
        low: 1.0,
        close: 1.0,
    }];
    let err = validate_bar_sessions_vs_shock_calendar(&shocks, &bars, cal).unwrap_err();
    assert!(
        err.contains("raw UTC calendar day 2"),
        "unexpected message: {err}"
    );
}

#[test]
fn execution_buffer_cap_drops_oldest_pending_signal() {
    let cal = SimpleWeekdayCalendar;
    let exec = SignalExecutionScan::with_timing_and_buffer(
        cal,
        ExecutionEntryTiming::EntrySessionOpen,
        ExecutionBufferPolicy::Cap {
            max_pending: 1,
            overflow: ExecutionBufferOverflow::DropOldest,
        },
    );
    let sig1 = EventShockSignal {
        event_id: EventId(1),
        entry_session: SessionDate(0),
        exit_session: SessionDate(1),
        exposure: Exposure::Long(Symbol("SPY".into())),
        strategy_name: "t".into(),
        scope: EventScope::Global,
        matched_treatment: None,
    };
    let sig2 = EventShockSignal {
        event_id: EventId(2),
        entry_session: SessionDate(0),
        exit_session: SessionDate(1),
        exposure: Exposure::Long(Symbol("SPY".into())),
        strategy_name: "t".into(),
        scope: EventScope::Global,
        matched_treatment: None,
    };
    let mut st = exec.init();
    let mut e = VecEmitter::new();
    exec.step(
        &mut st,
        EventShockReplayRecord::Signal(sig1.clone()),
        &mut e,
    );
    exec.step(&mut st, EventShockReplayRecord::Signal(sig2.clone()), &mut e);
    let snap = exec.snapshot(&st);
    assert_eq!(snap.pending.len(), 1);
    assert_eq!(snap.pending[0].event_id, EventId(2));
}
