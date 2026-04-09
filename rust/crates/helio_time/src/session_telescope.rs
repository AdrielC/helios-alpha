//! Hierarchical **telescope** schedules: disjoint range bands at each tree level (dates, years,
//! months, …) mapping to sub-trees or leaf session templates. Evaluated in a venue **IANA time
//! zone** so DST and calendar boundaries follow exchange-local rules.

use chrono::{Datelike, NaiveDate, NaiveTime};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Half-open `[start, end)` on the **Gregorian local calendar** (venue time zone context).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalDateRange {
    pub start: NaiveDate,
    pub end: NaiveDate,
}

impl LocalDateRange {
    pub fn contains(self, d: NaiveDate) -> bool {
        d >= self.start && d < self.end
    }
}

/// Half-open `[start, end)` in **calendar years** (`local_date.year()`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct YearRange {
    pub start_year: i32,
    pub end_year: i32,
}

impl YearRange {
    pub fn contains(self, d: NaiveDate) -> bool {
        let y = d.year();
        y >= self.start_year && y < self.end_year
    }
}

/// Half-open `[start, end)` in **calendar months** (1 = January). Does not wrap across years; use
/// separate [`LocalDateRange`] branches for patterns like “December–January”.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonthRange {
    pub start_month: u32,
    pub end_month: u32,
}

impl MonthRange {
    pub fn contains(self, d: NaiveDate) -> bool {
        let m = d.month();
        m >= self.start_month && m < self.end_month
    }
}

/// One level of the telescope: **disjoint** sibling ranges, each pointing at a sub-tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TelescopeLevel {
    /// Explicit local calendar date bands (finest control, including historical regimes).
    Dates {
        bands: Vec<(LocalDateRange, TelescopeNode)>,
    },
    Years {
        bands: Vec<(YearRange, TelescopeNode)>,
    },
    Months {
        bands: Vec<(MonthRange, TelescopeNode)>,
    },
}

/// Leaf: **local** wall-clock intervals on a session day (exchange time zone). Half-open
/// `[open, close)` in local time; gaps (e.g. lunch) use multiple disjoint intervals.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionTemplate {
    /// Sorted, disjoint local intervals (validated on build when using [`TelescopeNode::validated`]).
    pub intervals_local: Vec<(NaiveTime, NaiveTime)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TelescopeNode {
    Level(Box<TelescopeLevel>),
    Leaf(SessionTemplate),
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TelescopeBuildError {
    #[error("telescope: empty band list")]
    EmptyBands,
    #[error("telescope: overlapping or unsorted ranges at a level")]
    NonDisjointRanges,
    #[error("telescope: invalid month range (expect 1..=12 and start < end)")]
    InvalidMonthRange,
    #[error("telescope: invalid year range (start_year < end_year required)")]
    InvalidYearRange,
    #[error("telescope: invalid date range (start < end required)")]
    InvalidDateRange,
    #[error("telescope: invalid local time interval (start < end required)")]
    InvalidLocalInterval,
    #[error("telescope: overlapping local intervals in a session template")]
    OverlappingLocalIntervals,
}

fn validate_month_range(m: MonthRange) -> Result<(), TelescopeBuildError> {
    if !(1..=12).contains(&m.start_month) || !(1..=12).contains(&m.end_month) {
        return Err(TelescopeBuildError::InvalidMonthRange);
    }
    if m.start_month >= m.end_month {
        return Err(TelescopeBuildError::InvalidMonthRange);
    }
    Ok(())
}

fn validate_year_range(y: YearRange) -> Result<(), TelescopeBuildError> {
    if y.start_year >= y.end_year {
        return Err(TelescopeBuildError::InvalidYearRange);
    }
    Ok(())
}

fn validate_date_range(r: LocalDateRange) -> Result<(), TelescopeBuildError> {
    if r.start >= r.end {
        return Err(TelescopeBuildError::InvalidDateRange);
    }
    Ok(())
}

fn intervals_disjoint_sorted(iv: &[(NaiveTime, NaiveTime)]) -> Result<(), TelescopeBuildError> {
    for w in iv.windows(2) {
        let (a0, a1) = w[0];
        let (b0, b1) = w[1];
        if a0 >= a1 || b0 >= b1 {
            return Err(TelescopeBuildError::InvalidLocalInterval);
        }
        if a1 > b0 {
            return Err(TelescopeBuildError::OverlappingLocalIntervals);
        }
    }
    if let Some((s, e)) = iv.first() {
        if *s >= *e {
            return Err(TelescopeBuildError::InvalidLocalInterval);
        }
    }
    Ok(())
}

impl TelescopeNode {
    /// Recursively validate disjointness and ordering. Children are validated eagerly.
    pub fn validated(self) -> Result<Self, TelescopeBuildError> {
        match &self {
            TelescopeNode::Leaf(leaf) => {
                intervals_disjoint_sorted(&leaf.intervals_local)?;
            }
            TelescopeNode::Level(lvl) => match lvl.as_ref() {
                TelescopeLevel::Dates { bands } => {
                    if bands.is_empty() {
                        return Err(TelescopeBuildError::EmptyBands);
                    }
                    let mut prev_start: Option<NaiveDate> = None;
                    let mut prev_end: Option<NaiveDate> = None;
                    for (r, child) in bands {
                        validate_date_range(*r)?;
                        if let Some(ps) = prev_start {
                            if r.start <= ps {
                                return Err(TelescopeBuildError::NonDisjointRanges);
                            }
                        }
                        if let Some(p) = prev_end {
                            if r.start < p {
                                return Err(TelescopeBuildError::NonDisjointRanges);
                            }
                        }
                        prev_start = Some(r.start);
                        prev_end = Some(r.end);
                        child.clone().validated()?;
                    }
                }
                TelescopeLevel::Years { bands } => {
                    if bands.is_empty() {
                        return Err(TelescopeBuildError::EmptyBands);
                    }
                    let mut prev_start: Option<i32> = None;
                    let mut prev_end: Option<i32> = None;
                    for (y, child) in bands {
                        validate_year_range(*y)?;
                        if let Some(ps) = prev_start {
                            if y.start_year <= ps {
                                return Err(TelescopeBuildError::NonDisjointRanges);
                            }
                        }
                        if let Some(p) = prev_end {
                            if y.start_year < p {
                                return Err(TelescopeBuildError::NonDisjointRanges);
                            }
                        }
                        prev_start = Some(y.start_year);
                        prev_end = Some(y.end_year);
                        child.clone().validated()?;
                    }
                }
                TelescopeLevel::Months { bands } => {
                    if bands.is_empty() {
                        return Err(TelescopeBuildError::EmptyBands);
                    }
                    let mut prev_start: Option<u32> = None;
                    let mut prev_end: Option<u32> = None;
                    for (m, child) in bands {
                        validate_month_range(*m)?;
                        if let Some(ps) = prev_start {
                            if m.start_month <= ps {
                                return Err(TelescopeBuildError::NonDisjointRanges);
                            }
                        }
                        if let Some(p) = prev_end {
                            if m.start_month < p {
                                return Err(TelescopeBuildError::NonDisjointRanges);
                            }
                        }
                        prev_start = Some(m.start_month);
                        prev_end = Some(m.end_month);
                        child.clone().validated()?;
                    }
                }
            },
        }
        Ok(self)
    }

    /// Resolve the session template for `local_date` (venue calendar). Returns `None` if no band
    /// matches at some level (caller may treat as “no defined hours”).
    pub fn resolve_template(&self, local_date: NaiveDate) -> Option<SessionTemplate> {
        match self {
            TelescopeNode::Leaf(t) => Some(t.clone()),
            TelescopeNode::Level(lvl) => match lvl.as_ref() {
                TelescopeLevel::Dates { bands } => {
                    let i = bands.partition_point(|(r, _)| r.start <= local_date);
                    let idx = i.checked_sub(1)?;
                    let (r, child) = bands.get(idx)?;
                    if r.contains(local_date) {
                        child.resolve_template(local_date)
                    } else {
                        None
                    }
                }
                TelescopeLevel::Years { bands } => {
                    let y = local_date.year();
                    let i = bands.partition_point(|(yr, _)| yr.start_year <= y);
                    let idx = i.checked_sub(1)?;
                    let (yr, child) = bands.get(idx)?;
                    if yr.contains(local_date) {
                        child.resolve_template(local_date)
                    } else {
                        None
                    }
                }
                TelescopeLevel::Months { bands } => {
                    let m = local_date.month();
                    let i = bands.partition_point(|(mr, _)| mr.start_month <= m);
                    let idx = i.checked_sub(1)?;
                    let (mr, child) = bands.get(idx)?;
                    if mr.contains(local_date) {
                        child.resolve_template(local_date)
                    } else {
                        None
                    }
                }
            },
        }
    }
}

/// Sort date bands by start for [`TelescopeLevel::Dates`].
pub fn sort_date_bands(bands: &mut [(LocalDateRange, TelescopeNode)]) {
    bands.sort_by_key(|(r, _)| r.start);
}

/// Sort year bands by start year.
pub fn sort_year_bands(bands: &mut [(YearRange, TelescopeNode)]) {
    bands.sort_by_key(|(y, _)| y.start_year);
}

/// Sort month bands by start month.
pub fn sort_month_bands(bands: &mut [(MonthRange, TelescopeNode)]) {
    bands.sort_by_key(|(m, _)| m.start_month);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(open_h: u32, open_m: u32, close_h: u32, close_m: u32) -> TelescopeNode {
        TelescopeNode::Leaf(SessionTemplate {
            intervals_local: vec![(
                NaiveTime::from_hms_opt(open_h, open_m, 0).unwrap(),
                NaiveTime::from_hms_opt(close_h, close_m, 0).unwrap(),
            )],
        })
    }

    #[test]
    fn date_bands_resolve() {
        let mut bands = vec![
            (
                LocalDateRange {
                    start: NaiveDate::from_ymd_opt(2020, 1, 1).unwrap(),
                    end: NaiveDate::from_ymd_opt(2022, 1, 1).unwrap(),
                },
                leaf(9, 30, 16, 0),
            ),
            (
                LocalDateRange {
                    start: NaiveDate::from_ymd_opt(2022, 1, 1).unwrap(),
                    end: NaiveDate::from_ymd_opt(2030, 1, 1).unwrap(),
                },
                leaf(10, 0, 16, 0),
            ),
        ];
        sort_date_bands(&mut bands);
        let root = TelescopeNode::Level(Box::new(TelescopeLevel::Dates { bands }));
        let root = root.validated().unwrap();
        let t2021 = NaiveDate::from_ymd_opt(2021, 6, 15).unwrap();
        let t2023 = NaiveDate::from_ymd_opt(2023, 6, 15).unwrap();
        assert_eq!(
            root.resolve_template(t2021).unwrap().intervals_local[0].0,
            NaiveTime::from_hms_opt(9, 30, 0).unwrap()
        );
        assert_eq!(
            root.resolve_template(t2023).unwrap().intervals_local[0].0,
            NaiveTime::from_hms_opt(10, 0, 0).unwrap()
        );
    }

    #[test]
    fn unsorted_date_starts_rejected() {
        let bands = vec![
            (
                LocalDateRange {
                    start: NaiveDate::from_ymd_opt(2022, 1, 1).unwrap(),
                    end: NaiveDate::from_ymd_opt(2030, 1, 1).unwrap(),
                },
                leaf(10, 0, 16, 0),
            ),
            (
                LocalDateRange {
                    start: NaiveDate::from_ymd_opt(2020, 1, 1).unwrap(),
                    end: NaiveDate::from_ymd_opt(2022, 1, 1).unwrap(),
                },
                leaf(9, 30, 16, 0),
            ),
        ];
        let root = TelescopeNode::Level(Box::new(TelescopeLevel::Dates { bands }));
        assert_eq!(
            root.validated(),
            Err(TelescopeBuildError::NonDisjointRanges)
        );
    }

    #[test]
    fn overlapping_date_bands_rejected() {
        let bands = vec![
            (
                LocalDateRange {
                    start: NaiveDate::from_ymd_opt(2020, 1, 1).unwrap(),
                    end: NaiveDate::from_ymd_opt(2022, 1, 1).unwrap(),
                },
                leaf(9, 30, 16, 0),
            ),
            (
                LocalDateRange {
                    start: NaiveDate::from_ymd_opt(2021, 6, 1).unwrap(),
                    end: NaiveDate::from_ymd_opt(2023, 1, 1).unwrap(),
                },
                leaf(10, 0, 16, 0),
            ),
        ];
        let root = TelescopeNode::Level(Box::new(TelescopeLevel::Dates { bands }));
        assert_eq!(
            root.validated(),
            Err(TelescopeBuildError::NonDisjointRanges)
        );
    }
}
