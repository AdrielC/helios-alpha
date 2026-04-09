//! **Level-2 adapters:** drive a [`Scan`] from iterators, slices, or `std::sync::mpsc` without putting
//! transport into the core traits. Async / crossbeam / Tokio belong in separate crates or feature
//! modules that call these patterns.

use std::sync::mpsc::Receiver;

use crate::batch::ScanBatchExt;
use crate::emit::Emit;
use crate::scan::Scan;

/// Consume an iterator of inputs (in-memory, tests, `Vec`, etc.).
#[inline]
pub fn run_iter<S, E, I>(scan: &S, state: &mut S::State, inputs: I, emit: &mut E)
where
    S: Scan,
    E: Emit<S::Out>,
    I: IntoIterator<Item = S::In>,
{
    scan.step_batch(state, inputs, emit);
}

/// Consume a slice (convenience; clones elements unless `In` is `Copy`).
#[inline]
pub fn run_slice<S, E>(scan: &S, state: &mut S::State, batch: &[S::In], emit: &mut E)
where
    S: Scan,
    E: Emit<S::Out>,
    S::In: Clone,
{
    scan.step_batch(state, batch.iter().cloned(), emit);
}

/// Blocking: process messages until the sender disconnects and the channel drains.
pub fn run_receiver<S, E>(scan: &S, state: &mut S::State, rx: &Receiver<S::In>, emit: &mut E)
where
    S: Scan,
    E: Emit<S::Out>,
{
    while let Ok(msg) = rx.recv() {
        scan.step(state, msg, emit);
    }
}
