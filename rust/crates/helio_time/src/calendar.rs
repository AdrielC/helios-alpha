//! Minimal **trading-day** helpers for session alignment (UTC day index = Unix epoch day).
//! v1: configurable weekend skip; no exchange holidays.

use helio_scan::SessionDate;

/// Maps UTC epoch instants to [`SessionDate`] (day index) and adjacent sessions.
pub trait TradingCalendar {
    /// Next trading session strictly after `ts` (next session open after this instant).
    fn first_session_strictly_after_ts(&self, ts: i64) -> SessionDate;

    /// Earliest trading session whose calendar day contains or follows `ts`'s UTC day start.
    fn session_on_or_after_ts(&self, ts: i64) -> SessionDate;

    /// Latest trading session on or before `ts` (for impact end anchoring).
    fn session_on_or_before_ts(&self, ts: i64) -> SessionDate;

    /// Next trading session after `d` (by session day index).
    fn next_session_after(&self, d: SessionDate) -> SessionDate;

    /// Add `n` trading sessions to `d` (n >= 0).
    fn add_sessions(&self, d: SessionDate, n: u32) -> SessionDate;

    /// Inclusive count of trading sessions from `a` to `b` when stepping forward by [`TradingCalendar::next_session_after`].
    /// If `a` is after `b`, returns `0`.
    fn inclusive_session_count(&self, a: SessionDate, b: SessionDate) -> u32;

    /// Midpoint session in the inclusive range \[`a`, `b`\] (by session count, not calendar midpoint).
    fn mid_session_inclusive(&self, a: SessionDate, b: SessionDate) -> SessionDate;

    /// Previous trading session strictly before `d` (by session day index).
    fn prev_session_before(&self, d: SessionDate) -> SessionDate;

    /// Go back `n` trading sessions from `d` (`n == 0` returns `d`).
    fn sub_sessions(&self, d: SessionDate, n: u32) -> SessionDate;
}

/// Naive UTC **civil** day bucket: `floor_div(ts, 86_400)` into Unix epoch day indices.
///
/// This is **not** a trading-session label: it uses **UTC midnight** boundaries only. A venue
/// session that **opens the prior local evening** (or any non-UTC calendar) can belong to a
/// different “day” than this index. For session assignment use [`TradingCalendar`] (e.g.
/// [`TradingCalendar::session_on_or_after_ts`] / [`TradingCalendar::session_on_or_before_ts`])
/// after mapping wall time through your **session-date rule** (exchange calendar, `as_of`, etc.).
#[inline]
pub fn utc_naive_civil_day_index(ts: i64) -> i32 {
    ts.div_euclid(86_400) as i32
}

/// Same as [`utc_naive_civil_day_index`] — kept under this name for older call sites.
#[inline]
pub fn utc_calendar_day(ts: i64) -> i32 {
    utc_naive_civil_day_index(ts)
}

/// Monday=0 .. Sunday=6 (ISO-style) for the UTC calendar day containing instant `ts`.
#[inline]
pub fn utc_weekday_for_ts(ts: i64) -> i32 {
    let d = utc_calendar_day(ts);
    // 1970-01-01 UTC is Thursday; weekday Monday=0 => Thu=3.
    (d.rem_euclid(7) + 3).rem_euclid(7)
}

/// Skip Sat/Sun only (UTC). Weekday 5=Sat, 6=Sun in Monday=0 encoding.
#[derive(Debug, Clone, Copy, Default)]
pub struct SimpleWeekdayCalendar;

impl SimpleWeekdayCalendar {
    #[inline]
    fn is_weekend_day_index(day: i32) -> bool {
        let w = (day.rem_euclid(7) + 3).rem_euclid(7);
        w == 5 || w == 6 // Sat, Sun (Monday=0)
    }

    #[inline]
    fn forward_to_trading_day(day: i32) -> i32 {
        let mut d = day;
        while Self::is_weekend_day_index(d) {
            d += 1;
        }
        d
    }

    #[inline]
    fn backward_to_trading_day(day: i32) -> i32 {
        let mut d = day;
        while Self::is_weekend_day_index(d) {
            d -= 1;
        }
        d
    }
}

impl TradingCalendar for SimpleWeekdayCalendar {
    fn first_session_strictly_after_ts(&self, ts: i64) -> SessionDate {
        let d = utc_calendar_day(ts);
        let next_cal = d + 1;
        SessionDate(Self::forward_to_trading_day(next_cal))
    }

    fn session_on_or_after_ts(&self, ts: i64) -> SessionDate {
        SessionDate(Self::forward_to_trading_day(utc_calendar_day(ts)))
    }

    fn session_on_or_before_ts(&self, ts: i64) -> SessionDate {
        SessionDate(Self::backward_to_trading_day(utc_calendar_day(ts)))
    }

    fn next_session_after(&self, d: SessionDate) -> SessionDate {
        let mut n = d.0 + 1;
        n = Self::forward_to_trading_day(n);
        SessionDate(n)
    }

    fn add_sessions(&self, d: SessionDate, n: u32) -> SessionDate {
        let mut cur = d;
        for _ in 0..n {
            cur = self.next_session_after(cur);
        }
        cur
    }

    fn inclusive_session_count(&self, a: SessionDate, b: SessionDate) -> u32 {
        if a.0 > b.0 {
            return 0;
        }
        let mut cur = a;
        let mut c = 0u32;
        loop {
            c += 1;
            if cur == b {
                break;
            }
            cur = self.next_session_after(cur);
            if cur.0 > b.0 {
                break;
            }
        }
        c
    }

    fn mid_session_inclusive(&self, a: SessionDate, b: SessionDate) -> SessionDate {
        let (lo, hi) = if a.0 <= b.0 { (a, b) } else { (b, a) };
        let n = self.inclusive_session_count(lo, hi);
        if n == 0 {
            return lo;
        }
        let off = (n - 1) / 2;
        self.add_sessions(lo, off)
    }

    fn prev_session_before(&self, d: SessionDate) -> SessionDate {
        let mut n = d.0 - 1;
        n = Self::backward_to_trading_day(n);
        SessionDate(n)
    }

    fn sub_sessions(&self, d: SessionDate, n: u32) -> SessionDate {
        let mut cur = d;
        for _ in 0..n {
            cur = self.prev_session_before(cur);
        }
        cur
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thursday_ts_advances_to_friday_not_saturday() {
        let cal = SimpleWeekdayCalendar;
        // 1970-01-01 is Thursday UTC
        let thu = 0i64;
        assert_eq!(cal.session_on_or_after_ts(thu).0, 0);
        let fri = cal.first_session_strictly_after_ts(thu);
        assert_eq!(fri.0, 1); // Friday
    }

    #[test]
    fn sub_sessions_skips_weekends() {
        let cal = SimpleWeekdayCalendar;
        // Monday 1970-01-05 = UTC day index 4; two trading sessions back is Thursday (index 0).
        let mon = SessionDate(4);
        let thu = cal.sub_sessions(mon, 2);
        assert_eq!(thu.0, 0);
    }
}
