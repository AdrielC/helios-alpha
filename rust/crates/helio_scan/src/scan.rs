use serde::{de::DeserializeOwned, Serialize};

use crate::combinator::{Then, ZipInput};
use crate::control::FlushReason;
use crate::emit::Emit;

/// Base scan: one step updates state and may emit zero or more outputs.
pub trait Scan {
    type In;
    type Out;
    type State;

    fn init(&self) -> Self::State;

    fn step<E>(&self, state: &mut Self::State, input: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>;

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
}

/// Serialize/deserialize runtime state for external storage.
pub trait SnapshottingScan: Scan {
    type Snapshot: Clone + Serialize + DeserializeOwned;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot;
    fn restore(&self, snapshot: Self::Snapshot) -> Self::State;
}
