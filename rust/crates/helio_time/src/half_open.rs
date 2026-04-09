//! Generic **half-open** ranges `[start, end)` for any ordered type. Shared primitive for schedule
//! bands, tiling checks, and (with [`crate::bucket::TimeWindow`]) interval specs.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Left-closed, right-open interval `[start, end)` in an ordered domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct HalfOpenRange<T> {
    pub start: T,
    pub end: T,
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum HalfOpenBuildError {
    #[error("half-open range requires start < end")]
    EmptyOrInverted,
}

impl<T: Ord> HalfOpenRange<T> {
    /// Returns `Err` when `start >= end` (empty or inverted).
    pub fn try_new(start: T, end: T) -> Result<Self, HalfOpenBuildError> {
        if start < end {
            Ok(Self { start, end })
        } else {
            Err(HalfOpenBuildError::EmptyOrInverted)
        }
    }

    /// Membership in `[start, end)`.
    #[inline]
    pub fn contains(&self, x: &T) -> bool {
        *x >= self.start && *x < self.end
    }

    /// Whether this and `other` overlap as half-open intervals (empty intersection is allowed at a touch).
    #[inline]
    pub fn overlaps(&self, other: &Self) -> bool
    where
        T: Clone,
    {
        self.start < other.end && other.start < self.end
    }
}

/// Sort bands by [`HalfOpenRange::start`].
pub fn sort_bands_by_start<T: Ord, V>(bands: &mut [(HalfOpenRange<T>, V)]) {
    bands.sort_by(|a, b| a.0.start.cmp(&b.0.start));
}

/// Non-empty ranges, **strictly increasing** starts, and **pairwise disjoint** half-open spans.
pub fn validate_disjoint_sorted<T: Ord, V>(
    bands: &[(HalfOpenRange<T>, V)],
) -> Result<(), DisjointBandsError> {
    if bands.is_empty() {
        return Err(DisjointBandsError::EmptyBands);
    }
    let mut prev_start: Option<&T> = None;
    let mut prev_end: Option<&T> = None;
    for (r, _) in bands {
        if r.start >= r.end {
            return Err(DisjointBandsError::EmptyOrInvertedRange);
        }
        if let Some(ps) = prev_start {
            if r.start <= *ps {
                return Err(DisjointBandsError::UnsortedOrDuplicateStart);
            }
        }
        if let Some(pe) = prev_end {
            if r.start < *pe {
                return Err(DisjointBandsError::Overlapping);
            }
        }
        prev_start = Some(&r.start);
        prev_end = Some(&r.end);
    }
    Ok(())
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DisjointBandsError {
    #[error("band list is empty")]
    EmptyBands,
    #[error("half-open band has start >= end")]
    EmptyOrInvertedRange,
    #[error("bands are not sorted by strictly increasing start")]
    UnsortedOrDuplicateStart,
    #[error("half-open bands overlap")]
    Overlapping,
}

/// Index of the band that may contain `key` when `bands` are sorted by `start` ascending.
/// Returns `None` if `key` is before the first band start or no band contains `key`.
pub fn pick_band_for_key<T: Ord, V>(bands: &[(HalfOpenRange<T>, V)], key: &T) -> Option<usize> {
    let i = bands.partition_point(|(r, _)| r.start <= *key);
    let idx = i.checked_sub(1)?;
    let (r, _) = bands.get(idx)?;
    if r.contains(key) {
        Some(idx)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disjoint_validation() {
        let bands = [
            (HalfOpenRange::try_new(0, 2).unwrap(), ()),
            (HalfOpenRange::try_new(2, 5).unwrap(), ()),
        ];
        assert!(validate_disjoint_sorted(&bands).is_ok());
        let bad = [
            (HalfOpenRange::try_new(0, 3).unwrap(), ()),
            (HalfOpenRange::try_new(2, 4).unwrap(), ()),
        ];
        assert_eq!(
            validate_disjoint_sorted(&bad),
            Err(DisjointBandsError::Overlapping)
        );
    }

    #[test]
    fn pick_band() {
        let bands = vec![
            (HalfOpenRange::try_new(10, 20).unwrap(), 'a'),
            (HalfOpenRange::try_new(20, 30).unwrap(), 'b'),
        ];
        assert_eq!(pick_band_for_key(&bands, &15), Some(0));
        assert_eq!(pick_band_for_key(&bands, &20), Some(1));
        assert_eq!(pick_band_for_key(&bands, &5), None);
    }
}
