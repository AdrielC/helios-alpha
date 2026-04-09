use serde::{Deserialize, Serialize};

/// Wall-clock step unit for [`FixedStep`]. **Not** calendar months/years (see [`CalendarUnit`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FixedUnit {
    Second,
    Minute,
    Hour,
    Day,
    Week,
}

/// `n` × `unit` on a **fixed** timeline (e.g. 3 × `Day` = 72h in UTC math for labeling — still not “3 calendar days”).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FixedStep {
    pub n: u32,
    pub unit: FixedUnit,
}

/// Calendar-relative step (exchange calendar / civil calendar — interpretation is upstream).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CalendarUnit {
    Day,
    Week,
    Month,
    Quarter,
    Year,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CalendarStep {
    pub n: u32,
    pub unit: CalendarUnit,
}

/// Step of `n` **sessions** (business days, RTH, etc. — defined by calendar provider).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionStep {
    pub n: u32,
}

/// First-class frequency: **semantic category is preserved** (do not collapse to a single duration type).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Frequency {
    /// Last *n* observations (sample-count window).
    Samples(u32),
    Fixed(FixedStep),
    Calendar(CalendarStep),
    Session(SessionStep),
}

impl Frequency {
    /// Sample-count window size, if this frequency is [`Frequency::Samples`].
    pub fn as_samples(&self) -> Option<u32> {
        match self {
            Frequency::Samples(n) => Some(*n),
            _ => None,
        }
    }
}
