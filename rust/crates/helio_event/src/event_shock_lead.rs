//! Lead-time analysis: `impact_start - available_at` (seconds) vs tradability bands.

use serde::{Deserialize, Serialize};

use crate::EventShock;

/// `lead_time_secs = impact_start - available_at` per ingested shock (no gate/filter).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LeadTimeReport {
    pub n_events: u64,
    pub min_lead_secs: i64,
    pub max_lead_secs: i64,
    /// Count with `min <= lead <= max` (inclusive).
    pub n_tradable_under_band: u64,
    pub band_min_secs: i64,
    pub band_max_secs: i64,
}

/// Summarize lead times; `band_*` define the “tradeable” window for reporting.
pub fn summarize_lead_times(
    shocks: &[EventShock],
    band_min_secs: i64,
    band_max_secs: i64,
) -> LeadTimeReport {
    let n = shocks.len() as u64;
    if shocks.is_empty() {
        return LeadTimeReport {
            n_events: 0,
            min_lead_secs: 0,
            max_lead_secs: 0,
            n_tradable_under_band: 0,
            band_min_secs,
            band_max_secs,
        };
    }
    let mut mn = i64::MAX;
    let mut mx = i64::MIN;
    let mut n_ok = 0u64;
    for s in shocks {
        let lead = crate::signal_lead_secs(s);
        mn = mn.min(lead);
        mx = mx.max(lead);
        if lead >= band_min_secs && lead <= band_max_secs {
            n_ok += 1;
        }
    }
    LeadTimeReport {
        n_events: n,
        min_lead_secs: mn,
        max_lead_secs: mx,
        n_tradable_under_band: n_ok,
        band_min_secs,
        band_max_secs,
    }
}
