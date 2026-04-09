//! **Opt-in** batch paths that may be faster than repeated [`Scan::step`]. Implementors must preserve
//! **exact** semantics vs sequential stepping (same state transitions and **same outputs in order**).

use crate::emit::Emit;
use crate::scan::Scan;

/// Lawful batch fast path. Default approach: implement via [`crate::ScanBatchExt::step_batch`] until
/// a real fusion proof exists (often in window aggregators, not arbitrary state machines).
pub trait BatchOptimizedScan: Scan {
    fn step_batch_optimized<E>(&self, state: &mut Self::State, batch: &[Self::In], emit: &mut E)
    where
        E: Emit<Self::Out>;
}
