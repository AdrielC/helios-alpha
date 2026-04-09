//! Named strategy presets for the vertical (same [`EventShock`], different exit + exposure).

use crate::{ExitPolicy, Exposure, Symbol};

/// Second strategy: **ITA vs SPY**, exit at **mid impact window** (session-count midpoint).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventShockStrategyPreset {
    /// XLU–SPY pair, fixed 5-session hold after entry.
    XluSpyPairFiveSession,
    /// ITA–SPY pair, exit at calendar mid-window session; controls use 5-session horizon (approx).
    DefenseSpyPairMidWindow,
}

impl EventShockStrategyPreset {
    pub fn exit_policy(self) -> ExitPolicy {
        match self {
            Self::XluSpyPairFiveSession => ExitPolicy::FixedHorizonSessions { n: 5 },
            Self::DefenseSpyPairMidWindow => ExitPolicy::MidImpactWindowSession,
        }
    }

    /// Horizon passed to `EventShockControlConfig` (fixed exit only uses `n` from exit policy; mid-window uses this for control length).
    pub fn control_horizon_sessions(self) -> u32 {
        match self {
            Self::XluSpyPairFiveSession => 5,
            Self::DefenseSpyPairMidWindow => 5,
        }
    }

    pub fn treatment_exposure(self) -> Exposure {
        match self {
            Self::XluSpyPairFiveSession => Exposure::Pair {
                long: Symbol("XLU".into()),
                short: Symbol("SPY".into()),
            },
            Self::DefenseSpyPairMidWindow => Exposure::Pair {
                long: Symbol("ITA".into()),
                short: Symbol("SPY".into()),
            },
        }
    }

    pub fn control_exposure_clone(&self) -> Exposure {
        self.treatment_exposure()
    }
}
