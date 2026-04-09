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
    run_batch(scan, state, inputs, emit);
}

/// Ordered batch of inputs; same semantics as [`run_iter`] (sequential [`Scan::step`]).
#[inline]
pub fn run_batch<S, E, I>(scan: &S, state: &mut S::State, inputs: I, emit: &mut E)
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

/// Async MPSC: process until the channel closes (requires `helio_scan` feature `stream`).
#[cfg(feature = "stream")]
pub async fn run_stream<S, E>(
    scan: &S,
    state: &mut S::State,
    rx: &mut tokio::sync::mpsc::Receiver<S::In>,
    emit: &mut E,
) where
    S: Scan,
    E: Emit<S::Out>,
{
    while let Some(msg) = rx.recv().await {
        scan.step(state, msg, emit);
    }
}

#[cfg(all(test, feature = "stream"))]
mod stream_tests {
    use super::*;
    use crate::emit::VecEmitter;
    use crate::Scan;

    struct Inc;

    impl Scan for Inc {
        type In = i32;
        type Out = i32;
        type State = i32;

        fn init(&self) -> Self::State {
            0
        }

        fn step<E: crate::Emit<Self::Out>>(&self, st: &mut Self::State, input: Self::In, emit: &mut E) {
            *st += input;
            emit.emit(*st);
        }
    }

    #[tokio::test]
    async fn run_stream_drains() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(4);
        let scan = Inc;
        let mut st = scan.init();
        let mut out = VecEmitter::new();
        tx.send(1).await.unwrap();
        tx.send(2).await.unwrap();
        drop(tx);
        run_stream(&scan, &mut st, &mut rx, &mut out).await;
        assert_eq!(out.into_inner(), vec![1, 3]);
    }
}
