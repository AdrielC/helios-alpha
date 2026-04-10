use serde::{de::DeserializeOwned, Serialize};

use crate::combinator::{Then, ZipInput};
use crate::control::FlushReason;
use crate::emit::{Emit, VecEmitter};

/// Base scan: one step updates state and may emit **zero or more** outputs (including **one** for
/// the common 1:1 case, or **none** until a sub-problem is “saturated”, e.g. a full time window).
pub trait Scan {
    type In;
    type Out;
    type State;

    fn init(&self) -> Self::State;

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>;

    /// Collect all outputs from one [`step`](Scan::step) into a `Vec` (same semantics as `VecEmitter`).
    ///
    /// Prefer [`step`](Scan::step) when you can stream into a custom [`Emit`](crate::emit::Emit) sink
    /// to avoid allocation on hot paths.
    fn step_collect(&self, state: &mut Self::State, input: Self::In) -> Vec<Self::Out> {
        let mut e = VecEmitter::new();
        self.step(state, input, &mut e);
        e.into_inner()
    }

    /// Pipeline this scan into `right`: each output of `self` becomes an input of `right`.
    ///
    /// Same as [`Then`].
    fn then<B>(self, right: B) -> Then<Self, B>
    where
        Self: Sized,
        B: Scan<In = Self::Out>,
    {
        Then { left: self, right }
    }

    /// Fan-out on the same stream: run both scans on each input (order: `a` outputs first, then `b`).
    ///
    /// Requires a cloneable input type. Same as [`ZipInput`].
    fn and<B>(self, other: B) -> ZipInput<Self, B>
    where
        Self: Sized,
        B: Scan<In = Self::In>,
        Self::In: Clone,
    {
        ZipInput { a: self, b: other }
    }
}

/// Optional: logical version for snapshot migration.
pub trait VersionedSnapshot {
    const VERSION: u32;
}

/// React to control boundaries (session end, watermark, checkpoint, shutdown, …).
pub trait FlushableScan: Scan {
    type Offset;

    fn flush<E>(&self, state: &mut Self::State, signal: FlushReason<Self::Offset>, emit: &mut E)
    where
        E: Emit<Self::Out>;

    /// Collect all outputs from one [`flush`](FlushableScan::flush) (same idea as [`Scan::step_collect`]).
    fn flush_collect(
        &self,
        state: &mut Self::State,
        signal: FlushReason<Self::Offset>,
    ) -> Vec<Self::Out> {
        let mut e = VecEmitter::new();
        self.flush(state, signal, &mut e);
        e.into_inner()
    }
}

/// Serialize/deserialize runtime state for external storage.
pub trait SnapshottingScan: Scan {
    type Snapshot: Clone + Serialize + DeserializeOwned;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot;
    fn restore(&self, snapshot: Self::Snapshot) -> Self::State;
}
