//! **Layered schedule**: at each tree level, **disjoint** half-open bands ([`HalfOpenRange`]) over an
//! ordered key (local dates, years, months), each mapping to a child node or a leaf template. Use with
//! [`crate::business_time_clock::BusinessTimeClock`] for venue-local hours across historical regimes.

use chrono::{Datelike, NaiveDate, NaiveTime};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::bucket::TimeWindow;
use crate::half_open::{
    pick_band_for_key, sort_bands_by_start, validate_disjoint_sorted, DisjointBandsError,
    HalfOpenRange,
};

/// Local calendar date band `[start, end)` in the venue zone (see [`HalfOpenRange`]).
pub type LocalDateBand = HalfOpenRange<NaiveDate>;

/// Calendar year band `[start_year, end_year)` on `local_date.year()`.
pub type YearBand = HalfOpenRange<i32>;

/// Month-of-year band `[start_month, end_month)` with 1 = January. Use `end == 13` for “through
/// December” (`chrono::Datelike::month()` is 1..=12). Does not wrap across years — use
/// [`LocalDateBand`] for Dec–Jan spans.
pub type MonthBand = HalfOpenRange<u32>;

/// One level of the tree: disjoint sibling bands, each pointing at a sub-tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScheduleLayer {
    LocalDates {
        bands: Vec<(LocalDateBand, LayeredScheduleNode)>,
    },
    Years {
        bands: Vec<(YearBand, LayeredScheduleNode)>,
    },
    Months {
        bands: Vec<(MonthBand, LayeredScheduleNode)>,
    },
}

/// Leaf: **local** wall-clock segments on a session day. Gaps (lunch, auction) = multiple windows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionTemplate {
    /// Sorted, disjoint half-open local intervals (validated via [`LayeredScheduleNode::validated`]).
    pub intervals_local: Vec<TimeWindow<NaiveTime>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LayeredScheduleNode {
    Layer(Box<ScheduleLayer>),
    Leaf(SessionTemplate),
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum LayeredScheduleBuildError {
    #[error("schedule: empty band list")]
    EmptyBands,
    #[error("schedule: bands overlap, are unsorted, or have empty ranges")]
    InvalidBands(#[from] DisjointBandsError),
    #[error("schedule: invalid month band (months must be in 1..=12)")]
    InvalidMonthBand,
    #[error("schedule: invalid local time window")]
    InvalidLocalWindow,
    #[error("schedule: overlapping local intervals in session template")]
    OverlappingLocalIntervals,
}

fn validate_month_band(m: &MonthBand) -> Result<(), LayeredScheduleBuildError> {
    if m.start >= m.end {
        return Err(LayeredScheduleBuildError::InvalidMonthBand);
    }
    // `month()` is 1..=12; half-open `[s,e)` uses e up to 13 for “through December”.
    if m.start < 1 || m.start > 12 || m.end > 13 || m.end < 2 {
        return Err(LayeredScheduleBuildError::InvalidMonthBand);
    }
    Ok(())
}

fn intervals_disjoint_sorted(w: &[TimeWindow<NaiveTime>]) -> Result<(), LayeredScheduleBuildError> {
    for win in w {
        if win.start >= win.end {
            return Err(LayeredScheduleBuildError::InvalidLocalWindow);
        }
    }
    for a in w.windows(2) {
        let x = &a[0];
        let y = &a[1];
        if x.end > y.start {
            return Err(LayeredScheduleBuildError::OverlappingLocalIntervals);
        }
    }
    Ok(())
}

impl LayeredScheduleNode {
    /// Recursively validate disjointness and ordering. Children are validated eagerly.
    pub fn validated(self) -> Result<Self, LayeredScheduleBuildError> {
        match &self {
            LayeredScheduleNode::Leaf(leaf) => {
                intervals_disjoint_sorted(&leaf.intervals_local)?;
            }
            LayeredScheduleNode::Layer(lvl) => match lvl.as_ref() {
                ScheduleLayer::LocalDates { bands } => {
                    validate_disjoint_sorted(bands)?;
                    for (_, c) in bands {
                        c.clone().validated()?;
                    }
                }
                ScheduleLayer::Years { bands } => {
                    validate_disjoint_sorted(bands)?;
                    for (_, c) in bands {
                        c.clone().validated()?;
                    }
                }
                ScheduleLayer::Months { bands } => {
                    validate_disjoint_sorted(bands)?;
                    for (m, c) in bands {
                        validate_month_band(m)?;
                        c.clone().validated()?;
                    }
                }
            },
        }
        Ok(self)
    }

    /// Resolve the session template for `local_date` (venue calendar). Returns `None` if no band matches.
    pub fn resolve_template(&self, local_date: NaiveDate) -> Option<SessionTemplate> {
        match self {
            LayeredScheduleNode::Leaf(t) => Some(t.clone()),
            LayeredScheduleNode::Layer(lvl) => match lvl.as_ref() {
                ScheduleLayer::LocalDates { bands } => {
                    let idx = pick_band_for_key(bands, &local_date)?;
                    bands[idx].1.resolve_template(local_date)
                }
                ScheduleLayer::Years { bands } => {
                    let y = local_date.year();
                    let idx = pick_band_for_key(bands, &y)?;
                    bands[idx].1.resolve_template(local_date)
                }
                ScheduleLayer::Months { bands } => {
                    let m = local_date.month();
                    let idx = pick_band_for_key(bands, &m)?;
                    bands[idx].1.resolve_template(local_date)
                }
            },
        }
    }
}

/// Sort bands by start (convenience before [`LayeredScheduleNode::validated`]).
pub fn sort_local_date_bands(bands: &mut [(LocalDateBand, LayeredScheduleNode)]) {
    sort_bands_by_start(bands);
}

pub fn sort_year_bands(bands: &mut [(YearBand, LayeredScheduleNode)]) {
    sort_bands_by_start(bands);
}

pub fn sort_month_bands(bands: &mut [(MonthBand, LayeredScheduleNode)]) {
    sort_bands_by_start(bands);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(open_h: u32, open_m: u32, close_h: u32, close_m: u32) -> LayeredScheduleNode {
        LayeredScheduleNode::Leaf(SessionTemplate {
            intervals_local: vec![TimeWindow::new(
                NaiveTime::from_hms_opt(open_h, open_m, 0).unwrap(),
                NaiveTime::from_hms_opt(close_h, close_m, 0).unwrap(),
            )],
        })
    }

    #[test]
    fn date_bands_resolve() {
        let mut bands = vec![
            (
                HalfOpenRange::try_new(
                    NaiveDate::from_ymd_opt(2020, 1, 1).unwrap(),
                    NaiveDate::from_ymd_opt(2022, 1, 1).unwrap(),
                )
                .unwrap(),
                leaf(9, 30, 16, 0),
            ),
            (
                HalfOpenRange::try_new(
                    NaiveDate::from_ymd_opt(2022, 1, 1).unwrap(),
                    NaiveDate::from_ymd_opt(2030, 1, 1).unwrap(),
                )
                .unwrap(),
                leaf(10, 0, 16, 0),
            ),
        ];
        sort_local_date_bands(&mut bands);
        let root = LayeredScheduleNode::Layer(Box::new(ScheduleLayer::LocalDates { bands }));
        let root = root.validated().unwrap();
        let t2021 = NaiveDate::from_ymd_opt(2021, 6, 15).unwrap();
        let t2023 = NaiveDate::from_ymd_opt(2023, 6, 15).unwrap();
        assert_eq!(
            root.resolve_template(t2021).unwrap().intervals_local[0].start,
            NaiveTime::from_hms_opt(9, 30, 0).unwrap()
        );
        assert_eq!(
            root.resolve_template(t2023).unwrap().intervals_local[0].start,
            NaiveTime::from_hms_opt(10, 0, 0).unwrap()
        );
    }

    #[test]
    fn unsorted_date_starts_rejected() {
        let bands = vec![
            (
                HalfOpenRange::try_new(
                    NaiveDate::from_ymd_opt(2022, 1, 1).unwrap(),
                    NaiveDate::from_ymd_opt(2030, 1, 1).unwrap(),
                )
                .unwrap(),
                leaf(10, 0, 16, 0),
            ),
            (
                HalfOpenRange::try_new(
                    NaiveDate::from_ymd_opt(2020, 1, 1).unwrap(),
                    NaiveDate::from_ymd_opt(2022, 1, 1).unwrap(),
                )
                .unwrap(),
                leaf(9, 30, 16, 0),
            ),
        ];
        let root = LayeredScheduleNode::Layer(Box::new(ScheduleLayer::LocalDates { bands }));
        assert!(matches!(
            root.validated(),
            Err(LayeredScheduleBuildError::InvalidBands(
                DisjointBandsError::UnsortedOrDuplicateStart
            ))
        ));
    }

    #[test]
    fn overlapping_date_bands_rejected() {
        let bands = vec![
            (
                HalfOpenRange::try_new(
                    NaiveDate::from_ymd_opt(2020, 1, 1).unwrap(),
                    NaiveDate::from_ymd_opt(2022, 1, 1).unwrap(),
                )
                .unwrap(),
                leaf(9, 30, 16, 0),
            ),
            (
                HalfOpenRange::try_new(
                    NaiveDate::from_ymd_opt(2021, 6, 1).unwrap(),
                    NaiveDate::from_ymd_opt(2023, 1, 1).unwrap(),
                )
                .unwrap(),
                leaf(10, 0, 16, 0),
            ),
        ];
        let root = LayeredScheduleNode::Layer(Box::new(ScheduleLayer::LocalDates { bands }));
        assert!(matches!(
            root.validated(),
            Err(LayeredScheduleBuildError::InvalidBands(DisjointBandsError::Overlapping))
        ));
    }
}
