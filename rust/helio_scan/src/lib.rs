//! Composable scan machines: state transitions with multi-emission, flush, snapshot, and resume.
//!
//! Design slogan: *Scans are restartable, flushable, causality-aware state machines over ordered
//! streams. Composition preserves structure. State is inspectable, snapshotable, and resumable by offset.*

mod combinator;
mod control;
mod emit;
mod examples;
mod focus;
mod persist;
mod runner;
mod scan;

pub use combinator::*;
pub use control::*;
pub use emit::*;
pub use examples::*;
pub use focus::*;
pub use persist::*;
pub use runner::*;
pub use scan::*;
