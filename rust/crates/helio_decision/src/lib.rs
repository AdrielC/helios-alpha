//! Provider-neutral, typed external inference. Answers are evidence, never order authority.
//! Network and journal effects are injected at the native service boundary.
mod codec;
mod contract;
pub use codec::*;
pub use contract::*;

#[cfg(feature = "native")]
mod native;
#[cfg(feature = "native")]
pub use native::*;
