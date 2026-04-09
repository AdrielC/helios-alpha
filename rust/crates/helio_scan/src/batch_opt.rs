//! **Opt-in** batch paths that may be faster than repeated [`Scan::step`]. Implementors must preserve
//! **exact** semantics vs sequential stepping (same state transitions and **same outputs in order**).
//!
//! **Hygiene:** Prefer **not** implementing this trait until there is a real fast path or fusion
//! with a proof of equivalence. A `step_batch_optimized` that only loops [`Scan::step`] is legal but
//! can read as “optimized” in profiles — use [`crate::ScanBatchExt::step_batch`] instead unless the
//! trait serves benchmarking or a genuine specialization.

use crate::emit::Emit;
use crate::scan::Scan;

/// Lawful batch fast path. Prefer [`crate::ScanBatchExt::step_batch`] until a proven faster implementation exists.
pub trait BatchOptimizedScan: Scan {
    fn step_batch_optimized<E>(&self, state: &mut Self::State, batch: &[Self::In], emit: &mut E)
    where
        E: Emit<Self::Out>;
}
