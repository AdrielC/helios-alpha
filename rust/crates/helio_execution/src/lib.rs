//! Capital-facing controls for Helios Alpha.
//!
//! This crate is deliberately separate from signal research. It provides fixed-point order
//! values, pre-trade risk authorization, transaction-cost and capacity estimates, idempotent
//! broker dispatch, operational readiness, incident state, and evidence-backed capital admission.
//! No live order can pass [`OrderGateway`] without a current [`CapitalAuthorization`].

mod admission;
mod broker;
mod cost;
mod operations;
mod risk;
mod types;

pub use admission::*;
pub use broker::*;
pub use cost::*;
pub use operations::*;
pub use risk::*;
pub use types::*;
