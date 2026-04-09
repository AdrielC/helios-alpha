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

/// Weather-style forecast shock (hurricane / wind / surge window), same UTC fields as solar rows.
#[derive(Debug, Clone)]
pub struct WeatherShockRow {
    pub id: u64,
    pub available_at: i64,
    pub impact_start: i64,
    pub impact_end: i64,
    pub severity: f64,
    pub confidence: f64,
    /// Optional NWS-style region code for scope (0 = unset → Global).
    pub region_code: Option<u32>,
}

pub fn weather_row_to_event_shock(row: WeatherShockRow) -> EventShock {
    let scope = match row.region_code {
        Some(r) => EventScope::Region(r),
        None => EventScope::Global,
    };
    EventShock {
        id: EventId(row.id),
        kind: EventKind::Weather,
        observed_at: None,
        available_at: AvailableAt(row.available_at),
        impact_start: row.impact_start,
        impact_end: row.impact_end,
        severity: row.severity,
        confidence: row.confidence,
        scope,
    }
}
