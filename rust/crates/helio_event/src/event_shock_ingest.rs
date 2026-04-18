//! CSV / JSON Lines loaders → [`EventShock`](crate::EventShock).

use helio_scan::SessionDate;
use helio_time::{utc_calendar_day, utc_naive_civil_day_index, AvailableAt, TradingCalendar};
use serde::Deserialize;

use crate::{
    timed_shock, DailyBar, EventId, EventKind, EventScope, EventShock, EventShockVerticalRecord,
    Symbol,
};

/// Trading session used to **merge** a shock into the vertical replay stream (same key as
/// [`crate::vertical_merge_key`] for [`EventShockVerticalRecord::Shock`] when `session_date` is set).
///
/// Uses [`TradingCalendar::session_on_or_after_ts`] on [`EventShock::available_at`] so weekend /
/// holiday semantics match the calendar used by the vertical scan (see
/// [`build_vertical_replay_with_calendar`]).
#[inline]
pub fn merge_session_for_shock<C: TradingCalendar + Copy>(
    shock: &EventShock,
    calendar: C,
) -> SessionDate {
    calendar.session_on_or_after_ts(shock.available_at.0)
}

/// Calendar-aware replay: shocks use [`merge_session_for_shock`]; bars keep their CSV `session`
/// (must already match the same calendar). Prefer this over [`build_vertical_replay`] for
/// production-style data.
pub fn build_vertical_replay_with_calendar<C: TradingCalendar + Copy>(
    shocks: Vec<EventShock>,
    bars: Vec<DailyBar>,
    calendar: C,
) -> Vec<EventShockVerticalRecord> {
    let mut tagged: Vec<(usize, EventShockVerticalRecord)> = Vec::new();
    let mut i = 0usize;
    for b in bars {
        tagged.push((i, EventShockVerticalRecord::Bar(b)));
        i += 1;
    }
    let mut shock_seq = 0u32;
    for s in shocks {
        let session = merge_session_for_shock(&s, calendar);
        let mut t = timed_shock(s);
        t.session_date = Some(session);
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

/// Reject one **specific** ingest mistake: bar rows keyed to [`utc_naive_civil_day_index`] of
/// `available_at` when that naive UTC midnight bucket is **not** a trading session under
/// `calendar`, while [`merge_session_for_shock`] rolls `available_at` to a different
/// [`SessionDate`] (typical: shock on Sat/Sun UTC, bars still using the weekend day index).
///
/// **Not a full “session day” audit:** [`utc_naive_civil_day_index`] is only `floor_div(ts, 86400)`
/// in UTC — it is **not** “the session the event belongs to” for venues whose session **starts
/// before local midnight** or spans calendar boundaries. Real bars should use the same
/// **session-date convention** as your exchange calendar (often “trade date” or “session open
/// date” in a chosen zone), then index `SessionDate` consistently; this helper only catches the
/// UTC-weekend index footgun above.
pub fn validate_bar_sessions_vs_shock_calendar<C: TradingCalendar + Copy>(
    shocks: &[EventShock],
    bars: &[DailyBar],
    calendar: C,
) -> Result<(), String> {
    for s in shocks {
        let naive_utc_day = utc_naive_civil_day_index(s.available_at.0);
        let expected = merge_session_for_shock(s, calendar);
        if naive_utc_day == expected.0 {
            continue;
        }
        if bars.iter().any(|b| b.session.0 == naive_utc_day) {
            return Err(format!(
                "bar session index {naive_utc_day} equals naive UTC civil day (floor_div(epoch_sec,86400)) of shock {} available_at, but that day is not a trading session under this calendar — merge_session_for_shock maps to session {}. Bar session indices must follow your venue session-date rule, not raw UTC midnight buckets when they disagree with the calendar.",
                s.id.0,
                expected.0
            ));
        }
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct EventShockCsvRow {
    id: u64,
    #[serde(default)]
    kind: EventKind,
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
        kind: row.kind,
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

/// Header: `id[,kind],available_at,impact_start,impact_end,severity,confidence,scope[,scope_id][,symbol][,tags]`
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
            kind: EventKind::default(),
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
            kind: EventKind::default(),
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

/// Merge shocks with daily bars into a single ordered stream for [`crate::EventShockVerticalScan`].
///
/// ## Merge order
///
/// Records are sorted by [`crate::vertical_merge_key`]:
///
/// 1. Primary: **session day index** (`DailyBar.session` for bars; shock uses
///    `utc_calendar_day(available_at)` as the merge session — **raw UTC day**, not
///    [`TradingCalendar::session_on_or_after_ts`]).
/// 2. Secondary: **bars before shocks** within the same session (`0` vs `1` in the key).
/// 3. Tertiary: shock `stream_seq` (ingest order among shocks sharing the same bucket).
/// 4. **Stable tie-break**: original ingest order (`bars` first in file order, then `shocks`).
///
/// ## Late bars
///
/// If a bar for session `S` is ordered **after** shocks keyed to `S`, those shocks were already
/// stepped before the bar arrived; execution may still fill once the bar appears (signals stay
/// pending). Ordering is **not** automatically "causal by wall clock" beyond this sort key.
///
/// For calendar-consistent shock keys, use [`build_vertical_replay_with_calendar`].
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
