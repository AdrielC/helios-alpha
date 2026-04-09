use crate::control::FlushReason;
use crate::emit::Emit;
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

    pub fn reset(&mut self) {
        self.state = self.machine.init();
    }
}

impl<M: FlushableScan> Runner<M> {
    pub fn flush<E: Emit<M::Out>>(&mut self, signal: FlushReason<M::Offset>, emit: &mut E) {
        self.machine.flush(&mut self.state, signal, emit);
    }
}
