//! CSV / JSON Lines loaders → [`EventShock`](crate::EventShock).

use helio_scan::SessionDate;
use helio_time::{utc_calendar_day, AvailableAt};
use serde::Deserialize;

use crate::{
    timed_shock, DailyBar, EventId, EventScope, EventShock, EventShockVerticalRecord, Symbol,
};

#[derive(Debug, Deserialize)]
struct EventShockCsvRow {
    id: u64,
    available_at: i64,
    impact_start: i64,
    impact_end: i64,
    severity: f64,
    confidence: f64,
    scope: String,
    #[serde(default)]
    scope_id: Option<u32>,
    #[serde(default)]
    symbol: Option<String>,
    #[serde(default)]
    tags: Option<String>,
}

fn parse_scope(row: &EventShockCsvRow) -> Result<EventScope, String> {
    match row.scope.to_ascii_lowercase().as_str() {
        "global" => Ok(EventScope::Global),
        "region" => row
            .scope_id
            .map(EventScope::Region)
            .ok_or_else(|| "scope_id required for region".to_string()),
        "sector" => row
            .scope_id
            .map(EventScope::Sector)
            .ok_or_else(|| "scope_id required for sector".to_string()),
        "basket" => row
            .scope_id
            .map(EventScope::Basket)
            .ok_or_else(|| "scope_id required for basket".to_string()),
        "instrument" => row
            .symbol
            .as_ref()
            .map(|x| EventScope::Instrument(Symbol(x.clone())))
            .ok_or_else(|| "symbol required for instrument scope".to_string()),
        other => Err(format!("unknown scope: {other}")),
    }
}

fn row_to_shock(row: EventShockCsvRow) -> Result<EventShock, String> {
    let scope = parse_scope(&row)?;
    Ok(EventShock {
        id: EventId(row.id),
        tags: row.tags.unwrap_or_default(),
        observed_at: None,
        available_at: AvailableAt(row.available_at),
        impact_start: row.impact_start,
        impact_end: row.impact_end,
        severity: row.severity,
        confidence: row.confidence,
        scope,
    })
}

/// Header: `id,available_at,impact_start,impact_end,severity,confidence,scope[,scope_id][,symbol][,tags]`
///
/// `tags` is optional; use comma-separated tokens for your own taxonomy (ignored by the vertical).
pub fn load_event_shocks_csv(data: &str) -> Result<Vec<EventShock>, String> {
    let mut rdr = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_reader(data.as_bytes());
    let mut out = Vec::new();
    for rec in rdr.deserialize::<EventShockCsvRow>() {
        let row = rec.map_err(|e| e.to_string())?;
        out.push(row_to_shock(row)?);
    }
    Ok(out)
}

/// Compact CSV without `scope` / `scope_id` / `symbol` (global scope, empty tags):  
/// `id,available_at,impact_start,impact_end,severity,confidence`
pub fn load_compact_event_shocks_csv(data: &str) -> Result<Vec<EventShock>, String> {
    #[derive(Debug, Deserialize)]
    struct CompactCsvRow {
        id: u64,
        available_at: i64,
        impact_start: i64,
        impact_end: i64,
        severity: f64,
        confidence: f64,
    }
    let mut rdr = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_reader(data.as_bytes());
    let mut out = Vec::new();
    for rec in rdr.deserialize::<CompactCsvRow>() {
        let row = rec.map_err(|e| e.to_string())?;
        out.push(EventShock {
            id: EventId(row.id),
            tags: String::new(),
            observed_at: None,
            available_at: AvailableAt(row.available_at),
            impact_start: row.impact_start,
            impact_end: row.impact_end,
            severity: row.severity,
            confidence: row.confidence,
            scope: EventScope::Global,
        });
    }
    Ok(out)
}

/// Same columns as [`load_compact_event_shocks_csv`] plus optional `region_code` → [`EventScope::Region`].
pub fn load_compact_region_event_shocks_csv(data: &str) -> Result<Vec<EventShock>, String> {
    #[derive(Debug, Deserialize)]
    struct RegionCsvRow {
        id: u64,
        available_at: i64,
        impact_start: i64,
        impact_end: i64,
        severity: f64,
        confidence: f64,
        #[serde(default)]
        region_code: Option<u32>,
    }
    let mut rdr = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_reader(data.as_bytes());
    let mut out = Vec::new();
    for rec in rdr.deserialize::<RegionCsvRow>() {
        let row = rec.map_err(|e| e.to_string())?;
        let scope = match row.region_code {
            Some(r) => EventScope::Region(r),
            None => EventScope::Global,
        };
        out.push(EventShock {
            id: EventId(row.id),
            tags: String::new(),
            observed_at: None,
            available_at: AvailableAt(row.available_at),
            impact_start: row.impact_start,
            impact_end: row.impact_end,
            severity: row.severity,
            confidence: row.confidence,
            scope,
        });
    }
    Ok(out)
}

/// One JSON object per line, same fields as full CSV (snake_case).
pub fn load_event_shocks_jsonl(data: &str) -> Result<Vec<EventShock>, String> {
    let mut out = Vec::new();
    for line in data.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let row: EventShockCsvRow = serde_json::from_str(t).map_err(|e| e.to_string())?;
        out.push(row_to_shock(row)?);
    }
    Ok(out)
}

#[derive(Debug, Deserialize)]
struct DailyBarCsvRow {
    session: i32,
    symbol: String,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
}

pub fn load_daily_bars_csv(data: &str) -> Result<Vec<DailyBar>, String> {
    let mut rdr = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_reader(data.as_bytes());
    let mut out = Vec::new();
    for rec in rdr.deserialize::<DailyBarCsvRow>() {
        let row = rec.map_err(|e| e.to_string())?;
        out.push(DailyBar {
            session: SessionDate(row.session),
            symbol: Symbol(row.symbol),
            open: row.open,
            high: row.high,
            low: row.low,
            close: row.close,
        });
    }
    Ok(out)
}

/// Bars first per session, then shocks; **stable** among ties (preserves shock ingest order).
pub fn build_vertical_replay(
    shocks: Vec<EventShock>,
    bars: Vec<DailyBar>,
) -> Vec<EventShockVerticalRecord> {
    let mut tagged: Vec<(usize, EventShockVerticalRecord)> = Vec::new();
    let mut i = 0usize;
    for b in bars {
        tagged.push((i, EventShockVerticalRecord::Bar(b)));
        i += 1;
    }
    let mut shock_seq = 0u32;
    for s in shocks {
        let day = utc_calendar_day(s.available_at.0);
        let mut t = timed_shock(s);
        t.session_date = Some(SessionDate(day));
        tagged.push((i, EventShockVerticalRecord::Shock(shock_seq, t)));
        shock_seq = shock_seq.wrapping_add(1);
        i += 1;
    }
    tagged.sort_by(|(ia, a), (ib, b)| {
        crate::vertical_merge_key(a)
            .cmp(&crate::vertical_merge_key(b))
            .then_with(|| ia.cmp(ib))
    });
    tagged.into_iter().map(|(_, r)| r).collect()
}

pub fn candidate_entries_from_bars(bars: &[DailyBar]) -> Vec<SessionDate> {
    let mut v: Vec<SessionDate> = bars.iter().map(|b| b.session).collect();
    v.sort_by_key(|d| d.0);
    v.dedup_by_key(|d| d.0);
    v
}
