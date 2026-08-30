use helio_time::{AvailableAt, EffectiveAt};
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

/// Stable identity for one logical timer within a hypothesis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TimerId(pub u64);

/// One payload with the two clocks needed for causal inference.
///
/// `effective_at` says when the underlying phenomenon occurred. `available_at` says when the
/// machine was allowed to use it. `sequence` is assigned by the normalized evidence ingress and
/// must increase without gaps within one hypothesis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CausalEvidence<E> {
    pub sequence: u64,
    pub effective_at: EffectiveAt,
    pub available_at: AvailableAt,
    pub payload: E,
}

impl<E> CausalEvidence<E> {
    pub const fn new(
        sequence: u64,
        effective_at: EffectiveAt,
        available_at: AvailableAt,
        payload: E,
    ) -> Self {
        Self {
            sequence,
            effective_at,
            available_at,
            payload,
        }
    }
}

/// An effect requested by a domain model after one atomic transition.
#[derive(Debug, Clone, PartialEq)]
pub enum HypothesisEffect<O> {
    Emit(O),
    Schedule { timer_id: TimerId, at: AvailableAt },
    Cancel { timer_id: TimerId },
    Complete,
}

/// A complete proposed model transition.
///
/// The runtime validates every effect before replacing live state, so a rejected transition cannot
/// partially schedule timers or publish model output. Four effects stay inline before `SmallVec`
/// needs heap storage.
#[derive(Debug, Clone, PartialEq)]
#[must_use]
pub struct HypothesisTransition<S, O> {
    next_state: S,
    effects: SmallVec<[HypothesisEffect<O>; 4]>,
}

impl<S, O> HypothesisTransition<S, O> {
    pub fn new(next_state: S) -> Self {
        Self {
            next_state,
            effects: SmallVec::new(),
        }
    }

    pub fn emit(mut self, output: O) -> Self {
        self.effects.push(HypothesisEffect::Emit(output));
        self
    }

    pub fn schedule(mut self, timer_id: TimerId, at: AvailableAt) -> Self {
        self.effects
            .push(HypothesisEffect::Schedule { timer_id, at });
        self
    }

    pub fn cancel(mut self, timer_id: TimerId) -> Self {
        self.effects.push(HypothesisEffect::Cancel { timer_id });
        self
    }

    pub fn complete(mut self) -> Self {
        self.effects.push(HypothesisEffect::Complete);
        self
    }

    pub fn effects(&self) -> &[HypothesisEffect<O>] {
        &self.effects
    }

    pub(crate) fn into_parts(self) -> (S, SmallVec<[HypothesisEffect<O>; 4]>) {
        (self.next_state, self.effects)
    }
}

/// Domain-specific conditional inference injected into the generic keyed runtime.
///
/// Implementations may keep a branch graph, posterior parameters, pending external request IDs,
/// or any other serializable state in `State`. External physics, ML, or data-service work should be
/// represented as `Output`; its response returns later as another `Evidence` value.
pub trait HypothesisModel<K> {
    type Evidence;
    type State: Clone;
    type Output;
    type Error;

    fn open(
        &self,
        key: &K,
        evidence: CausalEvidence<Self::Evidence>,
    ) -> Result<HypothesisTransition<Self::State, Self::Output>, Self::Error>;

    fn update(
        &self,
        key: &K,
        state: &Self::State,
        evidence: CausalEvidence<Self::Evidence>,
    ) -> Result<HypothesisTransition<Self::State, Self::Output>, Self::Error>;

    fn on_timer(
        &self,
        key: &K,
        state: &Self::State,
        timer_id: TimerId,
        at: AvailableAt,
    ) -> Result<HypothesisTransition<Self::State, Self::Output>, Self::Error>;

    /// Validate model-owned state loaded from an external snapshot.
    fn validate(&self, key: &K, state: &Self::State) -> Result<(), Self::Error>;
}
