//! Adapters from domain-specific rows into [`EventShock`](crate::EventShock).

use helio_time::AvailableAt;

use crate::{EventId, EventKind, EventScope, EventShock};

/// Minimal solar-style row (forecast availability + impact window in epoch seconds).
#[derive(Debug, Clone)]
pub struct SolarShockRow {
    pub id: u64,
    pub available_at: i64,
    pub impact_start: i64,
    pub impact_end: i64,
    pub severity: f64,
    pub confidence: f64,
}

pub fn solar_row_to_event_shock(row: SolarShockRow) -> EventShock {
    EventShock {
        id: EventId(row.id),
        kind: EventKind::Solar,
        observed_at: None,
        available_at: AvailableAt(row.available_at),
        impact_start: row.impact_start,
        impact_end: row.impact_end,
        severity: row.severity,
        confidence: row.confidence,
        scope: EventScope::Global,
    }
}
