use serde::{Deserialize, Serialize};

/// Wall-clock or business-calendar session boundary (placeholder; refine per domain).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionDate(pub i32);

/// Why a flush was requested; different scans react differently.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FlushReason<O> {
    SessionClose(SessionDate),
    Checkpoint(O),
    Watermark(O),
    Shutdown,
    Rebalance,
    EndOfInput,
    Manual,
}

/// Extra bookkeeping for persisted checkpoints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointMeta {
    pub format_version: u32,
    pub label: Option<String>,
}

impl Default for CheckpointMeta {
    fn default() -> Self {
        Self {
            format_version: 1,
            label: None,
        }
    }
}

/// Persisted machine state plus stream position for deterministic resume.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checkpoint<S, O> {
    pub snapshot: S,
    pub offset: O,
    pub watermark: Option<O>,
    pub metadata: CheckpointMeta,
}

impl<S, O> Checkpoint<S, O> {
    pub fn new(snapshot: S, offset: O) -> Self {
        Self {
            snapshot,
            offset,
            watermark: None,
            metadata: CheckpointMeta::default(),
        }
    }
}
