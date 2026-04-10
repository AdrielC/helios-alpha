//! Windowing and horizon **scans** on top of [`helio_scan`], with **operational** buffers and
//! aggregators. Semantic frequency / bounds live in **`helio_time`**.
//!
//! **Semantics vs implementation:** Many rolling paths are **sample-count** (`WindowSpec` +
//! [`WindowSpec::sample_capacity`](helio_time::WindowSpec::sample_capacity)). Fixed-time, calendar,
//! and session *extent* semantics in `helio_time` are not all enforced as automatic eviction here yet;
//! see crate `README.md` and [TIME_AND_WINDOWS.md](../../../docs/TIME_AND_WINDOWS.md).
//!
//! **Next serious engineering (typical order):** time-keyed eviction, session-driven window expiry,
//! clearer aggregator capability contracts, and optional watermark-driven finalization — without
//! blurring [`helio_scan`].
//!
//! [`helio_scan`]: helio_scan

mod agg;
mod buffer;
mod dedup;
mod event_cluster;
mod forward_horizon;
mod join_latest;
mod lag;
mod rolling;
mod session_window;
mod signal_pipeline;
mod watermark;
mod window_state;

pub use agg::*;
pub use buffer::*;
pub use dedup::*;
pub use event_cluster::*;
pub use forward_horizon::*;
pub use join_latest::*;
pub use lag::*;
pub use rolling::*;
pub use session_window::*;
pub use signal_pipeline::*;
pub use watermark::*;
pub use window_state::*;
