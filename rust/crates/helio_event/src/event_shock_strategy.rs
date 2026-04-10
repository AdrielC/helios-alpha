//! Named strategy presets for the vertical (same [`EventShock`], different exit + exposure).

use crate::{ExitPolicy, Exposure, Symbol};

/// Second strategy: **ITA vs SPY**, exit at **mid impact window** (session-count midpoint).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventShockStrategyPreset {
    /// XLU–SPY pair, fixed 3-session hold after [`crate::AlignedEventShock::entry_session`].
    XluSpyPairThreeSession,
    /// XLU–SPY pair, fixed 5-session hold after entry.
    XluSpyPairFiveSession,
    /// ITA–SPY pair, exit at calendar mid-window session; controls use 5-session horizon (approx).
    DefenseSpyPairMidWindow,
}

impl EventShockStrategyPreset {
    /// CLI / report label (stable).
    pub fn cli_name(self) -> &'static str {
        match self {
            Self::XluSpyPairThreeSession => "xlu-spy-3",
            Self::XluSpyPairFiveSession => "xlu-spy-5",
            Self::DefenseSpyPairMidWindow => "defense-spy-mid",
        }
    }

    pub fn exit_policy(self) -> ExitPolicy {
        match self {
            Self::XluSpyPairThreeSession => ExitPolicy::FixedHorizonSessions { n: 3 },
            Self::XluSpyPairFiveSession => ExitPolicy::FixedHorizonSessions { n: 5 },
            Self::DefenseSpyPairMidWindow => ExitPolicy::MidImpactWindowSession,
        }
    }

    /// Horizon passed to `EventShockControlConfig` (fixed exit only uses `n` from exit policy; mid-window uses this for control length).
    pub fn control_horizon_sessions(self) -> u32 {
        match self {
            Self::XluSpyPairThreeSession => 3,
            Self::XluSpyPairFiveSession => 5,
            Self::DefenseSpyPairMidWindow => 5,
        }
    }

    pub fn treatment_exposure(self) -> Exposure {
        match self {
            Self::XluSpyPairThreeSession | Self::XluSpyPairFiveSession => Exposure::Pair {
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
