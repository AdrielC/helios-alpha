use serde::{Deserialize, Serialize};

/// How bucket/window boundaries align in time. Resolution uses exchange calendar / UTC per caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Anchor {
    Epoch,
    SessionOpen,
    UtcMidnight,
    CalendarWeekStart,
    CalendarMonthStart,
    /// Caller-defined alignment; no default interpretation in this crate.
    Custom,
}
