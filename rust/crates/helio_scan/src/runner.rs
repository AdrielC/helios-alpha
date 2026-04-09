use crate::batch::ScanBatchExt;
use crate::control::FlushReason;
use crate::emit::Emit;
use crate::flush_batch::FlushableScanBatchExt;
use crate::scan::{FlushableScan, Scan};

/// Owns scan configuration and mutable state; drives `step` / `flush` from a source loop.
pub struct Runner<M: Scan> {
    pub machine: M,
    pub state: M::State,
}

impl<M: Scan> Runner<M> {
    pub fn new(machine: M) -> Self {
        let state = machine.init();
        Self { machine, state }
    }

    pub fn step<E: Emit<M::Out>>(&mut self, input: M::In, emit: &mut E) {
        self.machine.step(&mut self.state, input, emit);
    }

    /// Same semantics as stepping each item in order ([`ScanBatchExt::step_batch`]).
    pub fn step_batch<E: Emit<M::Out>, It: IntoIterator<Item = M::In>>(
        &mut self,
        inputs: It,
        emit: &mut E,
    ) {
        self.machine.step_batch(&mut self.state, inputs, emit);
    }

    pub fn reset(&mut self) {
        self.state = self.machine.init();
    }
}

impl<M: FlushableScan> Runner<M> {
    pub fn flush<E: Emit<M::Out>>(&mut self, signal: FlushReason<M::Offset>, emit: &mut E) {
        self.machine.flush(&mut self.state, signal, emit);
    }

    pub fn flush_batch<E: Emit<M::Out>, It: IntoIterator<Item = FlushReason<M::Offset>>>(
        &mut self,
        signals: It,
        emit: &mut E,
    ) {
        self.machine.flush_batch(&mut self.state, signals, emit);
    }
}
