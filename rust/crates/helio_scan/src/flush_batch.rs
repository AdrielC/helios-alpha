use crate::control::FlushReason;
use crate::emit::Emit;
use crate::scan::FlushableScan;

/// Apply multiple flush signals in order (e.g. batched control plane).
pub trait FlushableScanBatchExt: FlushableScan {
    fn flush_batch<E, It>(&self, state: &mut Self::State, signals: It, emit: &mut E)
    where
        It: IntoIterator<Item = FlushReason<Self::Offset>>,
        E: Emit<Self::Out>,
    {
        for signal in signals {
            self.flush(state, signal, emit);
        }
    }
}

impl<S: FlushableScan + ?Sized> FlushableScanBatchExt for S {}
