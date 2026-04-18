//! Incremental / checkpoint replay helpers for [`EventShockVerticalScan`](crate::EventShockVerticalScan).
//!
//! **Checkpoints:** [`collect_vertical_trades_with_checkpoint_resume`] snapshots after exactly
//! `checkpoint_after` records, restores, then continues. For [`FlushReason::Checkpoint`] inside a
//! live driver, only emit at boundaries where your persistence layer can resume the same ordered
//! stream; sub-scans each receive the flush (execution currently ignores it and keeps pending
//! signals).

use helio_scan::{
    run_receiver, run_slice, FlushReason, FlushableScan, Scan, SnapshottingScan, VecEmitter,
};
use helio_time::TradingCalendar;
use std::sync::mpsc;

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
///
/// Snapshots after `checkpoint_after` vertical records (not bytes). `checkpoint_after == 0` means
/// snapshot before any input (empty state), then replay all records on the restored state.
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

/// Same ordered stream through [`run_receiver`] (channel-driven incremental feed).
pub fn collect_vertical_trades_receiver<C: TradingCalendar + Copy>(
    vertical: &EventShockVerticalScan<C>,
    records: &[EventShockVerticalRecord],
) -> Vec<TradeResult> {
    let (tx, rx) = mpsc::channel();
    for r in records {
        tx.send(r.clone()).expect("send");
    }
    drop(tx);
    let mut st = vertical.init();
    let mut e = VecEmitter::new();
    run_receiver(vertical, &mut st, &rx, &mut e);
    vertical.flush(&mut st, FlushReason::EndOfInput, &mut e);
    e.into_inner()
}

/// Snapshot/restore at each `checkpoint_after` boundary (0 = no intermediate checkpoints).
pub fn collect_vertical_trades_with_checkpoint_cadence<C: TradingCalendar + Copy>(
    vertical: &EventShockVerticalScan<C>,
    records: &[EventShockVerticalRecord],
    checkpoint_every: usize,
) -> Vec<TradeResult> {
    if checkpoint_every == 0 {
        return collect_vertical_trades_incremental(vertical, records);
    }
    let mut out = Vec::new();
    let mut st = vertical.init();
    let mut e = VecEmitter::new();
    for (i, r) in records.iter().enumerate() {
        vertical.step(&mut st, r.clone(), &mut e);
        out.extend(e.into_inner());
        e = VecEmitter::new();
        if (i + 1) % checkpoint_every == 0 && i + 1 < records.len() {
            let snap = vertical.snapshot(&st);
            st = vertical.restore(snap);
        }
    }
    vertical.flush(&mut st, FlushReason::EndOfInput, &mut e);
    out.extend(e.into_inner());
    out
}
