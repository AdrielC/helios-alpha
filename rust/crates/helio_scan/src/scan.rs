use serde::{de::DeserializeOwned, Serialize};

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
