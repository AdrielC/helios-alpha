//! Optional **static** frequency helpers for optimized / const-generic call sites. The runtime system
//! uses [`crate::Frequency`]; this module is additive.

use crate::{FixedStep, FixedUnit, Frequency, SessionStep};

/// Compile-time sample count → [`Frequency::Samples`].
#[derive(Debug, Clone, Copy)]
pub struct Samples<const N: u32>;

impl<const N: u32> Samples<N> {
    #[inline]
    pub fn frequency() -> Frequency {
        Frequency::Samples(N)
    }
}

/// Marker for fixed wall-clock unit (typed path).
#[derive(Debug, Clone, Copy)]
pub struct Seconds;
#[derive(Debug, Clone, Copy)]
pub struct Minutes;
#[derive(Debug, Clone, Copy)]
pub struct Hours;
#[derive(Debug, Clone, Copy)]
pub struct Days;
#[derive(Debug, Clone, Copy)]
pub struct Weeks;

/// `N` units of `U` as [`Frequency::Fixed`]. Map marker → [`FixedUnit`] at conversion.
#[derive(Debug, Clone, Copy)]
pub struct Fixed<const N: u32, U>(std::marker::PhantomData<U>);

impl<const N: u32> Fixed<N, Seconds> {
    pub fn frequency() -> Frequency {
        Frequency::Fixed(FixedStep {
            n: N,
            unit: FixedUnit::Second,
        })
    }
}

impl<const N: u32> Fixed<N, Minutes> {
    pub fn frequency() -> Frequency {
        Frequency::Fixed(FixedStep {
            n: N,
            unit: FixedUnit::Minute,
        })
    }
}

impl<const N: u32> Fixed<N, Hours> {
    pub fn frequency() -> Frequency {
        Frequency::Fixed(FixedStep {
            n: N,
            unit: FixedUnit::Hour,
        })
    }
}

impl<const N: u32> Fixed<N, Days> {
    pub fn frequency() -> Frequency {
        Frequency::Fixed(FixedStep {
            n: N,
            unit: FixedUnit::Day,
        })
    }
}

impl<const N: u32> Fixed<N, Weeks> {
    pub fn frequency() -> Frequency {
        Frequency::Fixed(FixedStep {
            n: N,
            unit: FixedUnit::Week,
        })
    }
}

/// `N` sessions (same semantics as [`crate::SessionStep`]).
#[derive(Debug, Clone, Copy)]
pub struct Sessions<const N: u32>;

impl<const N: u32> Sessions<N> {
    pub fn frequency() -> Frequency {
        Frequency::Session(SessionStep { n: N })
    }
}
