//! Incremental / checkpoint replay helpers for [`EventShockVerticalScan`](crate::EventShockVerticalScan).

use helio_scan::{run_slice, FlushReason, FlushableScan, Scan, SnapshottingScan, VecEmitter};
use helio_time::TradingCalendar;

use crate::{EventShockVerticalRecord, EventShockVerticalScan, TradeResult};

/// One record at a time (same semantics as a batched `for` loop).
pub fn collect_vertical_trades_incremental<C: TradingCalendar + Copy>(
    vertical: &EventShockVerticalScan<C>,
    records: &[EventShockVerticalRecord],
) -> Vec<TradeResult> {
    let mut st = vertical.init();
    let mut e = VecEmitter::new();
    for r in records {
        vertical.step(&mut st, r.clone(), &mut e);
    }
    vertical.flush(&mut st, FlushReason::EndOfInput, &mut e);
    e.into_inner()
}

/// Batch all inputs in one `step_batch` call (when inputs are already in memory).
pub fn collect_vertical_trades_batch<C: TradingCalendar + Copy>(
    vertical: &EventShockVerticalScan<C>,
    records: &[EventShockVerticalRecord],
) -> Vec<TradeResult> {
    let mut st = vertical.init();
    let mut e = VecEmitter::new();
    run_slice(vertical, &mut st, records, &mut e);
    vertical.flush(&mut st, FlushReason::EndOfInput, &mut e);
    e.into_inner()
}

/// Mid-stream snapshot + restore + continue; must match [`collect_vertical_trades_incremental`].
pub fn collect_vertical_trades_with_checkpoint_resume<C: TradingCalendar + Copy>(
    vertical: &EventShockVerticalScan<C>,
    records: &[EventShockVerticalRecord],
    checkpoint_after: usize,
) -> Vec<TradeResult> {
    let mut e_first = VecEmitter::new();
    let mut st = vertical.init();
    for r in records.iter().take(checkpoint_after) {
        vertical.step(&mut st, r.clone(), &mut e_first);
    }
    let snap = vertical.snapshot(&st);
    let mut e_rest = VecEmitter::new();
    let mut st2 = vertical.restore(snap);
    for r in records.iter().skip(checkpoint_after) {
        vertical.step(&mut st2, r.clone(), &mut e_rest);
    }
    vertical.flush(&mut st2, FlushReason::EndOfInput, &mut e_rest);
    let mut out = e_first.into_inner();
    out.extend(e_rest.into_inner());
    out
}
