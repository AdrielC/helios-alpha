//! Keyed, bounded, and restartable hypothesis machines.
//!
//! A hypothesis begins with causally available evidence, owns independent model state, accepts
//! delayed follow-up evidence, and may schedule deterministic timers or emit external actions.
//! The runtime supplies lifecycle, ordering, boundedness, audit events, and validated snapshots.
//! The injected model supplies domain meaning and conditional inference.
//!
//! This crate deliberately contains no trading, astronomy, or sensor vocabulary. Those belong in
//! model implementations above this layer.

mod engine;
mod machine;
mod model;
#[cfg(feature = "service")]
mod service;

pub use engine::*;
pub use machine::*;
pub use model::*;
#[cfg(feature = "service")]
pub use service::*;
