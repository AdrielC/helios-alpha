use helio_scan::{FallibleRestoreScan, Scan, SnapshottingScan};
use serde::{de::DeserializeOwned, Serialize};

use crate::{
    HypothesisEvent, HypothesisInput, HypothesisModel, HypothesisRestoreError, HypothesisSnapshot,
    HypothesisState, KeyedHypothesisMachine,
};

/// Single-owner service core for a keyed hypothesis machine.
///
/// This is the preferred execution shape inside one worker or partition. It owns mutable state, so
/// every transition is lock-free. Inject the engine into application code by concrete generic type
/// or behind a narrow application trait. Use the optional async service module only when multiple
/// tasks need a shared handle.
#[derive(Debug, Clone)]
pub struct HypothesisEngine<K, Model, Reason>
where
    Model: HypothesisModel<K>,
{
    machine: KeyedHypothesisMachine<K, Model, Reason>,
    state: HypothesisState<K, Model::State, Reason>,
}

pub type HypothesisEngineParts<K, Model, Reason> = (
    KeyedHypothesisMachine<K, Model, Reason>,
    HypothesisState<K, <Model as HypothesisModel<K>>::State, Reason>,
);

impl<K, Model, Reason> HypothesisEngine<K, Model, Reason>
where
    K: Clone + Ord,
    Model: HypothesisModel<K>,
    Model::State: Clone,
    Reason: Clone,
{
    pub fn new(machine: KeyedHypothesisMachine<K, Model, Reason>) -> Self {
        let state = machine.init();
        Self { machine, state }
    }

    pub fn process(
        &mut self,
        input: HypothesisInput<K, Model::Evidence, Reason>,
    ) -> Vec<HypothesisEvent<K, Model::Output, Reason, Model::Error>> {
        self.machine.step_collect(&mut self.state, input)
    }

    pub const fn machine(&self) -> &KeyedHypothesisMachine<K, Model, Reason> {
        &self.machine
    }

    pub const fn state(&self) -> &HypothesisState<K, Model::State, Reason> {
        &self.state
    }

    pub fn into_parts(self) -> HypothesisEngineParts<K, Model, Reason> {
        (self.machine, self.state)
    }
}

impl<K, Model, Reason> HypothesisEngine<K, Model, Reason>
where
    K: Clone + Ord + Serialize + DeserializeOwned,
    Model: HypothesisModel<K>,
    Model::State: Clone + Serialize + DeserializeOwned,
    Reason: Clone + Serialize + DeserializeOwned,
{
    pub fn try_from_snapshot(
        machine: KeyedHypothesisMachine<K, Model, Reason>,
        snapshot: HypothesisSnapshot<K, Model::State, Reason>,
    ) -> Result<Self, HypothesisRestoreError<K, Model::Error>> {
        let state = machine.try_restore(snapshot)?;
        Ok(Self { machine, state })
    }

    pub fn snapshot(&self) -> HypothesisSnapshot<K, Model::State, Reason> {
        self.machine.snapshot(&self.state)
    }
}
