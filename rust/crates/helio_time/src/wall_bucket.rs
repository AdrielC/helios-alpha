//! **Wall-clock bucket grid** — generic over the **timeline coordinate** used to assign samples to
//! half-open buckets `[floor(t), next(floor(t)))`.
//!
//! Pair with **`TimeBucketEvent<G>`** in `helio_window` so payloads only expose “when” + “what to
//! aggregate”; the **bucket width** lives in `G`, not on each tick.

use serde::{de::DeserializeOwned, Deserialize, Serialize};

/// Defines **bucket width** and **floor** on a discrete timeline `T` (e.g. `i64` nanoseconds or seconds).
pub trait WallBucketGrid: Copy + Clone + Serialize + DeserializeOwned + 'static {
    /// Timeline coordinate (epoch offset in ns, sec, or another agreed unit).
    type T: Copy
        + Ord
        + Eq
        + std::hash::Hash
        + Serialize
        + DeserializeOwned
        + std::fmt::Debug;

    /// `floor(t / width) * width` — start of the half-open bucket containing `t`.
    fn bucket_start(&self, t: Self::T) -> Self::T;

    /// Exclusive end for the bucket that starts at `start` (for labels / availability helpers).
    fn bucket_end_exclusive(&self, start: Self::T) -> Self::T;

    /// True if width is positive and bucketing is defined.
    fn is_valid(&self) -> bool;
}

/// Fixed-width buckets on an **`i64` nanosecond** timeline (e.g. `PriceTick::t_ns`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NanosecondWallBucket {
    pub width_ns: i64,
}

impl WallBucketGrid for NanosecondWallBucket {
    type T = i64;

    fn bucket_start(&self, t: Self::T) -> Self::T {
        if self.width_ns <= 0 {
            return t;
        }
        t.div_euclid(self.width_ns) * self.width_ns
    }

    fn bucket_end_exclusive(&self, start: Self::T) -> Self::T {
        start.saturating_add(self.width_ns)
    }

    fn is_valid(&self) -> bool {
        self.width_ns > 0
    }
}

impl NanosecondWallBucket {
    pub fn ten_minutes() -> Self {
        Self {
            width_ns: 10 * 60 * 1_000_000_000,
        }
    }
}

/// Fixed-width buckets on an **`i64` second** timeline (epoch seconds — same unit as
/// [`crate::AvailableAt`](crate::AvailableAt).0 when interpreted as Unix seconds).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SecondWallBucket {
    pub width_sec: i64,
}

impl WallBucketGrid for SecondWallBucket {
    type T = i64;

    fn bucket_start(&self, t: Self::T) -> Self::T {
        if self.width_sec <= 0 {
            return t;
        }
        t.div_euclid(self.width_sec) * self.width_sec
    }

    fn bucket_end_exclusive(&self, start: Self::T) -> Self::T {
        start.saturating_add(self.width_sec)
    }

    fn is_valid(&self) -> bool {
        self.width_sec > 0
    }
}

impl SecondWallBucket {
    pub fn ten_minutes() -> Self {
        Self {
            width_sec: 10 * 60,
        }
    }
}
