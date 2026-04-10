//! Windowing and horizon **scans** on top of [`helio_scan`], with **operational** buffers and
//! aggregators. Semantic frequency / bounds live in **`helio_time`**.
//!
//! **Semantics vs implementation:** [`WindowState`] / [`RollingAggregatorScan`] are **sample-count**
//! when [`WindowSpec::sample_capacity`](helio_time::WindowSpec::sample_capacity) is `Some`.
//! **Time-keyed** trailing eviction (fixed `Frequency::Fixed` spans) lives in [`time_keyed`] and
//! [`TimeKeyedRollingAggregatorScan`]. **Session-keyed** trailing windows use [`session_keyed`].
//! Calendar `Frequency` in `WindowSpec` is still not auto-wired to ring buffers — see
//! [TIME_AND_WINDOWS.md](../../../docs/TIME_AND_WINDOWS.md).
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
mod rolling_time_keyed;
mod session_keyed;
mod session_window;
mod time_keyed;
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
pub use rolling_time_keyed::*;
pub use session_keyed::*;
pub use session_window::*;
pub use time_keyed::*;
pub use signal_pipeline::*;
pub use watermark::*;
pub use window_state::*;
