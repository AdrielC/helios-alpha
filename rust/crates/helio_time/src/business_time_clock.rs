//! **Business time** in a venue IANA zone: multi-interval sessions, DST-safe local→UTC mapping,
//! and second-accurate arithmetic along open hours. Pairs with [`crate::layered_schedule`] for
//! historical regime templates.

use chrono::{DateTime, Datelike, LocalResult, NaiveDate, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;

use crate::layered_schedule::{LayeredScheduleNode, SessionTemplate};

/// Supplies **which** local calendar days are session days (holidays, weekends, ad-hoc closures).
pub trait SessionDayOracle: Clone {
    fn local_date(&self, ts_utc_sec: i64) -> NaiveDate;
    fn is_session_day(&self, ts_utc_sec: i64) -> bool;
}

/// Oracle: Monday–Friday in the venue **local** calendar (UTC weekend ≠ local weekend). No
/// exchange holidays — combine with a holiday set for production.
#[derive(Debug, Clone, Copy, Default)]
pub struct LocalWeekdayOracle {
    pub zone: Tz,
}

impl SessionDayOracle for LocalWeekdayOracle {
    fn local_date(&self, ts_utc_sec: i64) -> NaiveDate {
        utc_sec_to_local_date(self.zone, ts_utc_sec)
    }

    fn is_session_day(&self, ts_utc_sec: i64) -> bool {
        let d = self.local_date(ts_utc_sec);
        let wd = d.weekday();
        !matches!(
            wd,
            chrono::Weekday::Sat | chrono::Weekday::Sun
        )
    }
}

/// Wrap a closure for session membership (e.g. backed by a bitset or `exchange_calendars` bridge).
#[derive(Clone)]
pub struct FnSessionOracle<F> {
    pub zone: Tz,
    pub is_session: F,
}

impl<F> SessionDayOracle for FnSessionOracle<F>
where
    F: Fn(NaiveDate) -> bool + Clone,
{
    fn local_date(&self, ts_utc_sec: i64) -> NaiveDate {
        utc_sec_to_local_date(self.zone, ts_utc_sec)
    }

    fn is_session_day(&self, ts_utc_sec: i64) -> bool {
        (self.is_session)(self.local_date(ts_utc_sec))
    }
}

#[inline]
pub fn utc_sec_to_local_date(zone: Tz, ts_utc_sec: i64) -> NaiveDate {
    let utc = DateTime::from_timestamp(ts_utc_sec, 0).unwrap_or_else(|| Utc.timestamp_opt(0, 0).unwrap());
    utc.with_timezone(&zone).date_naive()
}

#[inline]
pub fn utc_sec_to_local_datetime(zone: Tz, ts_utc_sec: i64) -> chrono::NaiveDateTime {
    let utc = DateTime::from_timestamp(ts_utc_sec, 0).unwrap_or_else(|| Utc.timestamp_opt(0, 0).unwrap());
    utc.with_timezone(&zone).naive_local()
}

/// Map venue-local wall clock to UTC. **Spring forward** gaps → `None`. **Fall back** ambiguity →
/// earlier UTC instant (first occurrence of that local time).
#[inline]
pub fn utc_from_local_wall(zone: Tz, date: NaiveDate, time: NaiveTime) -> Option<DateTime<Utc>> {
    let naive = date.and_time(time);
    match zone.from_local_datetime(&naive) {
        LocalResult::Single(dt) => Some(dt.with_timezone(&Utc)),
        LocalResult::Ambiguous(earliest, _) => Some(earliest.with_timezone(&Utc)),
        LocalResult::None => None,
    }
}

/// Ordered UTC half-open intervals `[start, end)` for one local session day.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionUtcDay {
    pub local_date: NaiveDate,
    pub intervals_utc: Vec<(i64, i64)>,
}

fn template_to_utc_day(zone: Tz, local_date: NaiveDate, template: &SessionTemplate) -> SessionUtcDay {
    let mut intervals_utc = Vec::with_capacity(template.intervals_local.len());
    for w in &template.intervals_local {
        let t0 = w.start;
        let t1 = w.end;
        if let (Some(a), Some(b)) = (
            utc_from_local_wall(zone, local_date, t0),
            utc_from_local_wall(zone, local_date, t1),
        ) {
            let a_sec = a.timestamp();
            let b_sec = b.timestamp();
            if b_sec > a_sec {
                intervals_utc.push((a_sec, b_sec));
            }
        }
    }
    intervals_utc.sort_by_key(|x| x.0);
    SessionUtcDay {
        local_date,
        intervals_utc,
    }
}

/// Layered schedule + zone + session-day oracle.
#[derive(Debug, Clone)]
pub struct BusinessTimeClock<O: SessionDayOracle> {
    pub zone: Tz,
    pub schedule: LayeredScheduleNode,
    pub oracle: O,
}

impl<O: SessionDayOracle> BusinessTimeClock<O> {
    pub fn session_template_for_utc(&self, ts_utc_sec: i64) -> Option<SessionTemplate> {
        let d = self.oracle.local_date(ts_utc_sec);
        self.schedule.resolve_template(d)
    }

    pub fn utc_intervals_for_session_day(&self, local_date: NaiveDate) -> Option<SessionUtcDay> {
        let template = self.schedule.resolve_template(local_date)?;
        Some(template_to_utc_day(self.zone, local_date, &template))
    }

    /// Seconds from the **first** open of the session day containing `ts_utc_sec` to `ts_utc_sec`,
    /// clamped to open time. `None` if outside defined template or not a session day.
    pub fn business_seconds_since_session_open(&self, ts_utc_sec: i64) -> Option<i64> {
        if !self.oracle.is_session_day(ts_utc_sec) {
            return None;
        }
        let d = self.oracle.local_date(ts_utc_sec);
        let day = self.utc_intervals_for_session_day(d)?;
        if day.intervals_utc.is_empty() {
            return None;
        }
        let open = day.intervals_utc[0].0;
        if ts_utc_sec < open {
            return Some(0);
        }
        let mut acc: i64 = 0;
        for (a, b) in &day.intervals_utc {
            if ts_utc_sec >= *b {
                acc = acc.saturating_add(b - a);
            } else if ts_utc_sec > *a {
                acc = acc.saturating_add(ts_utc_sec - a);
                break;
            } else {
                break;
            }
        }
        Some(acc)
    }

    /// Total open seconds on the local session day of `ts_utc_sec` (0 if holiday / no template).
    pub fn business_seconds_in_session_day(&self, ts_utc_sec: i64) -> i64 {
        if !self.oracle.is_session_day(ts_utc_sec) {
            return 0;
        }
        let d = self.oracle.local_date(ts_utc_sec);
        let Some(day) = self.utc_intervals_for_session_day(d) else {
            return 0;
        };
        day.intervals_utc
            .iter()
            .map(|(a, b)| b - a)
            .sum()
    }

    /// Whether `ts_utc_sec` lies inside any defined open interval for that local day.
    pub fn is_within_business_hours(&self, ts_utc_sec: i64) -> bool {
        if !self.oracle.is_session_day(ts_utc_sec) {
            return false;
        }
        let d = self.oracle.local_date(ts_utc_sec);
        let Some(day) = self.utc_intervals_for_session_day(d) else {
            return false;
        };
        day.intervals_utc
            .iter()
            .any(|(a, b)| ts_utc_sec >= *a && ts_utc_sec < *b)
    }

    /// Add signed **business** seconds along open intervals (skips nights, weekends per oracle,
    /// holidays, and between-interval gaps). `None` if a DST gap makes a boundary undefined or
    /// overflow/underflow past representable range.
    pub fn add_business_seconds(&self, mut ts_utc_sec: i64, mut delta: i64) -> Option<i64> {
        const MAX_DAYS: i32 = 366 * 50;
        if delta == 0 {
            return Some(ts_utc_sec);
        }
        let step = if delta > 0 { 1i64 } else { -1i64 };
        let mut guard = 0i32;
        while delta != 0 {
            guard += 1;
            if guard > MAX_DAYS {
                return None;
            }
            if !self.oracle.is_session_day(ts_utc_sec) {
                ts_utc_sec = if step > 0 {
                    next_local_midnight_utc(self.zone, ts_utc_sec)?
                } else {
                    last_open_second_strictly_before(self, ts_utc_sec)?
                };
                continue;
            }
            let d = self.oracle.local_date(ts_utc_sec);
            let Some(day) = self.utc_intervals_for_session_day(d) else {
                ts_utc_sec = if step > 0 {
                    next_local_midnight_utc(self.zone, ts_utc_sec)?
                } else {
                    last_open_second_strictly_before(self, ts_utc_sec)?
                };
                continue;
            };
            if day.intervals_utc.is_empty() {
                ts_utc_sec = if step > 0 {
                    next_local_midnight_utc(self.zone, ts_utc_sec)?
                } else {
                    last_open_second_strictly_before(self, ts_utc_sec)?
                };
                continue;
            }
            if step > 0 {
                let pos = day
                    .intervals_utc
                    .iter()
                    .position(|(a, b)| ts_utc_sec >= *a && ts_utc_sec < *b);
                if let Some(i) = pos {
                    let (_a, b) = day.intervals_utc[i];
                    let room = (b - ts_utc_sec).min(delta);
                    ts_utc_sec += room;
                    delta -= room;
                    if delta == 0 {
                        return Some(ts_utc_sec);
                    }
                    if i + 1 < day.intervals_utc.len() {
                        ts_utc_sec = day.intervals_utc[i + 1].0;
                    } else {
                        ts_utc_sec = next_local_midnight_utc(self.zone, ts_utc_sec)?;
                    }
                } else {
                    let next_open = day
                        .intervals_utc
                        .iter()
                        .map(|x| x.0)
                        .find(|&o| o > ts_utc_sec);
                    if let Some(o) = next_open {
                        ts_utc_sec = o;
                    } else {
                        ts_utc_sec = next_local_midnight_utc(self.zone, ts_utc_sec)?;
                    }
                }
            } else {
                let first_open = day.intervals_utc[0].0;
                if ts_utc_sec < first_open {
                    ts_utc_sec = last_open_second_strictly_before(self, ts_utc_sec)?;
                    continue;
                }
                let pos = day
                    .intervals_utc
                    .iter()
                    .rposition(|(a, b)| ts_utc_sec >= *a && ts_utc_sec < *b);
                if let Some(i) = pos {
                    let (a, _b) = day.intervals_utc[i];
                    let room = (ts_utc_sec - a).min(-delta);
                    ts_utc_sec -= room;
                    delta += room;
                    if delta == 0 {
                        return Some(ts_utc_sec);
                    }
                    if i > 0 {
                        let prev_end = day.intervals_utc[i - 1].1;
                        ts_utc_sec = prev_end - 1;
                    } else {
                        ts_utc_sec = last_open_second_strictly_before(self, first_open)?;
                    }
                } else {
                    let last_end = day.intervals_utc.last().unwrap().1;
                    if ts_utc_sec >= last_end {
                        ts_utc_sec = last_end - 1;
                        continue;
                    }
                    let mut placed = false;
                    for w in day.intervals_utc.windows(2) {
                        let b0 = w[0].1;
                        let a1 = w[1].0;
                        if ts_utc_sec >= b0 && ts_utc_sec < a1 {
                            ts_utc_sec = b0 - 1;
                            placed = true;
                            break;
                        }
                    }
                    if !placed {
                        ts_utc_sec = last_open_second_strictly_before(self, first_open)?;
                    }
                }
            }
        }
        Some(ts_utc_sec)
    }
}

fn start_of_local_date_utc(zone: Tz, d: NaiveDate) -> Option<i64> {
    let midnight = NaiveTime::from_hms_opt(0, 0, 0)?;
    utc_from_local_wall(zone, d, midnight).map(|dt| dt.timestamp())
}

fn next_local_midnight_utc(zone: Tz, ts_utc_sec: i64) -> Option<i64> {
    let d = utc_sec_to_local_date(zone, ts_utc_sec);
    let next = d.succ_opt()?;
    start_of_local_date_utc(zone, next)
}

#[inline]
fn local_noon_utc(zone: Tz, d: NaiveDate) -> Option<i64> {
    utc_from_local_wall(
        zone,
        d,
        NaiveTime::from_hms_opt(12, 0, 0)?,
    )
    .map(|dt| dt.timestamp())
}

fn is_session_local_day<O: SessionDayOracle>(clock: &BusinessTimeClock<O>, d: NaiveDate) -> bool {
    let Some(ts) = local_noon_utc(clock.zone, d) else {
        return false;
    };
    clock.oracle.is_session_day(ts)
}

/// Greatest UTC instant `< exclusive_end` that lies in an open interval, if any.
fn last_open_second_strictly_before<O: SessionDayOracle>(
    clock: &BusinessTimeClock<O>,
    exclusive_end: i64,
) -> Option<i64> {
    const MAX_DAYS: i32 = 366 * 50;
    if exclusive_end <= i64::MIN + 1 {
        return None;
    }
    let mut d = utc_sec_to_local_date(clock.zone, exclusive_end - 1);
    for _ in 0..MAX_DAYS {
        if is_session_local_day(clock, d) {
            if let Some(day) = clock.utc_intervals_for_session_day(d) {
                for (a, b) in day.intervals_utc.iter().rev() {
                    let last = b.saturating_sub(1);
                    if last >= *a && last < exclusive_end {
                        return Some(last);
                    }
                }
            }
        }
        d = d.pred_opt()?;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bucket::TimeWindow;
    use crate::layered_schedule::LayeredScheduleNode;

    fn xnys_schedule_leaf() -> LayeredScheduleNode {
        let leaf = LayeredScheduleNode::Leaf(SessionTemplate {
            intervals_local: vec![TimeWindow::new(
                NaiveTime::from_hms_opt(9, 30, 0).unwrap(),
                NaiveTime::from_hms_opt(16, 0, 0).unwrap(),
            )],
        });
        leaf.validated().unwrap()
    }

    #[test]
    fn spring_forward_gap_skips_nonexistent_minute() {
        let zone = Tz::America__New_York;
        let d = NaiveDate::from_ymd_opt(2024, 3, 10).unwrap();
        let bad = NaiveTime::from_hms_opt(2, 30, 0).unwrap();
        assert!(utc_from_local_wall(zone, d, bad).is_none());
        let open = NaiveTime::from_hms_opt(9, 30, 0).unwrap();
        assert!(utc_from_local_wall(zone, d, open).is_some());
    }

    #[test]
    fn business_seconds_counts_rth_march_friday_2024() {
        let zone = Tz::America__New_York;
        let sch = xnys_schedule_leaf();
        let clock = BusinessTimeClock {
            zone,
            schedule: sch,
            oracle: LocalWeekdayOracle { zone },
        };
        let open = utc_from_local_wall(
            zone,
            NaiveDate::from_ymd_opt(2024, 3, 8).unwrap(),
            NaiveTime::from_hms_opt(9, 30, 0).unwrap(),
        )
        .unwrap()
        .timestamp();
        let mid = open + 3600;
        let s = clock.business_seconds_since_session_open(mid).unwrap();
        assert_eq!(s, 3600);
        assert!(clock.is_within_business_hours(mid));
    }

    #[test]
    fn add_business_seconds_forward_over_weekend() {
        let zone = Tz::America__New_York;
        let sch = xnys_schedule_leaf();
        let clock = BusinessTimeClock {
            zone,
            schedule: sch,
            oracle: LocalWeekdayOracle { zone },
        };
        let fri = NaiveDate::from_ymd_opt(2024, 3, 8).unwrap();
        let close = utc_from_local_wall(
            zone,
            fri,
            NaiveTime::from_hms_opt(16, 0, 0).unwrap(),
        )
        .unwrap()
        .timestamp();
        let t = clock.add_business_seconds(close - 1, 2).unwrap();
        let mon = NaiveDate::from_ymd_opt(2024, 3, 11).unwrap();
        let mon_open = utc_from_local_wall(
            zone,
            mon,
            NaiveTime::from_hms_opt(9, 30, 0).unwrap(),
        )
        .unwrap()
        .timestamp();
        assert_eq!(t, mon_open + 1);
    }
}
