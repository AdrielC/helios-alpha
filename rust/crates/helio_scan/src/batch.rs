//! **Opaque batching:** `step_batch` is exactly `step` in order. No algebraic fusion unless a scan
//! implements [`crate::BatchOptimizedScan`] with a proven equivalent.

use crate::emit::Emit;
use crate::scan::Scan;

/// Extension for driving a machine over multiple inputs per call. Default: sequential [`Scan::step`].
pub trait ScanBatchExt: Scan {
    /// Semantically equivalent to `for x in inputs { self.step(state, x, emit); }`.
    fn step_batch<E, It>(&self, state: &mut Self::State, inputs: It, emit: &mut E)
    where
        It: IntoIterator<Item = Self::In>,
        E: Emit<Self::Out>,
    {
        for input in inputs {
            self.step(state, input, emit);
        }
    }
}

impl<S: Scan + ?Sized> ScanBatchExt for S {}
