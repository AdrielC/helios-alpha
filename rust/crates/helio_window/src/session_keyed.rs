//! **Session-keyed** trailing windows: keep samples whose [`SessionDate`](helio_scan::SessionDate) lies in
//! the inclusive trading range `[sub_sessions(now, n-1), now]` using a [`TradingCalendar`](helio_time::TradingCalendar).

use helio_scan::SessionDate;
use helio_time::TradingCalendar;
use std::collections::VecDeque;
use std::marker::PhantomData;

use crate::agg::EvictingWindowAggregator;

#[derive(Debug, Clone)]
pub struct SessionKeyedSample<T> {
    pub session: SessionDate,
    pub value: T,
}

/// Trailing **n** trading sessions (inclusive of the current session key), eviction keyed by session index.
#[derive(Debug, Clone)]
pub struct SessionKeyedRollingState<T, A, C: TradingCalendar + Copy> {
    calendar: C,
    trailing_sessions: u32,
    deque: VecDeque<SessionKeyedSample<T>>,
    agg: A,
    _p: PhantomData<C>,
}

impl<T: Clone, A: EvictingWindowAggregator<T>, C: TradingCalendar + Copy>
    SessionKeyedRollingState<T, A, C>
{
    pub fn new(calendar: C, trailing_sessions: u32, agg: A) -> Option<Self> {
        if trailing_sessions == 0 {
            return None;
        }
        Some(Self {
            calendar,
            trailing_sessions,
            deque: VecDeque::new(),
            agg,
            _p: PhantomData,
        })
    }

    fn earliest_allowed(&self, current: SessionDate) -> SessionDate {
        self.calendar
            .sub_sessions(current, self.trailing_sessions.saturating_sub(1))
    }

    pub fn push(&mut self, session: SessionDate, value: T) {
        let lo = self.earliest_allowed(session);
        while let Some(front) = self.deque.front() {
            if front.session.0 >= lo.0 {
                break;
            }
            let old = self.deque.pop_front().expect("front exists");
            self.agg.evict(&old.value);
        }
        self.deque.push_back(SessionKeyedSample {
            session,
            value: value.clone(),
        });
        self.agg.insert(&value);
    }

    pub fn summary(&self) -> A::Summary {
        self.agg.snapshot()
    }

    pub fn len(&self) -> usize {
        self.deque.len()
    }

    pub fn clear(&mut self) {
        self.deque.clear();
        self.agg.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agg::SumCountMeanAggregator;
    use helio_time::SimpleWeekdayCalendar;

    #[test]
    fn trailing_three_sessions_drops_old_week() {
        let cal = SimpleWeekdayCalendar;
        let mut w =
            SessionKeyedRollingState::new(cal, 3, SumCountMeanAggregator::default()).unwrap();
        let mon = SessionDate(4);
        let tue = cal.next_session_after(mon);
        let wed = cal.next_session_after(tue);
        let thu = cal.next_session_after(wed);
        w.push(mon, 1.0);
        w.push(tue, 2.0);
        w.push(wed, 4.0);
        assert_eq!(w.len(), 3);
        w.push(thu, 8.0);
        // mon falls outside trailing 3 from thu
        assert_eq!(w.len(), 3);
        let s = w.summary();
        assert_eq!(s.count, 3);
        assert!((s.sum - 14.0).abs() < 1e-9);
    }
}
