//! Optional Tokio service adapters.
//!
//! The preferred shared service is a bounded actor: one task owns the engine and callers clone a
//! typed handle. This avoids a global mutex and applies backpressure at the mailbox. A mutex-backed
//! adapter is also available for embedding, but it never performs external I/O while locked.

use std::future::Future;
use std::sync::Arc;

use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio::task::{JoinError, JoinHandle};

use crate::{
    HypothesisEngine, HypothesisEvent, HypothesisInput, HypothesisModel, HypothesisSnapshot,
};

/// ZIO-style capability surface for typed service injection.
///
/// Consumers should depend on this trait, not on a global singleton. Implementations remain
/// statically dispatched unless an application explicitly chooses a trait object boundary.
pub trait HypothesisService {
    type Input;
    type Event;
    type Error;

    fn process(
        &self,
        input: Self::Input,
    ) -> impl Future<Output = Result<Vec<Self::Event>, Self::Error>> + Send;
}

/// Events and the exact post-transition snapshot produced by one serialized service command.
///
/// The pair prevents another in-process caller from interleaving between transition and snapshot.
/// A durable driver must still atomically store the snapshot, source position, and output outbox
/// before it acknowledges the source.
#[derive(Debug, Clone, PartialEq)]
pub struct HypothesisProcessBatch<Event, Snapshot> {
    pub events: Vec<Event>,
    pub snapshot: Snapshot,
}

/// Typed capability for callers that need a post-transition persistence boundary.
pub trait SnapshottingHypothesisService: HypothesisService {
    type Snapshot;

    fn process_and_snapshot(
        &self,
        input: Self::Input,
    ) -> impl Future<Output = Result<HypothesisProcessBatch<Self::Event, Self::Snapshot>, Self::Error>>
           + Send;

    fn snapshot(&self) -> impl Future<Output = Result<Self::Snapshot, Self::Error>> + Send;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum HypothesisServiceConfigError {
    #[error("hypothesis service mailbox capacity must be positive")]
    ZeroMailboxCapacity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum HypothesisServiceError {
    #[error("hypothesis service is closed")]
    Closed,
    #[error("hypothesis service worker stopped before replying")]
    ResponseDropped,
}

#[derive(Debug, Error)]
pub enum HypothesisServiceJoinError {
    #[error("hypothesis service task no longer owns its join handle")]
    MissingJoinHandle,
    #[error("hypothesis service task failed: {0}")]
    Join(#[from] JoinError),
}

type InputFor<K, M, R> = HypothesisInput<K, <M as HypothesisModel<K>>::Evidence, R>;
type EventFor<K, M, R> =
    HypothesisEvent<K, <M as HypothesisModel<K>>::Output, R, <M as HypothesisModel<K>>::Error>;
type SnapshotFor<K, M, R> = HypothesisSnapshot<K, <M as HypothesisModel<K>>::State, R>;
type ProcessBatchFor<K, M, R> = HypothesisProcessBatch<EventFor<K, M, R>, SnapshotFor<K, M, R>>;

enum ServiceCommand<K, Model, Reason>
where
    Model: HypothesisModel<K>,
{
    Process {
        input: InputFor<K, Model, Reason>,
        reply: oneshot::Sender<Vec<EventFor<K, Model, Reason>>>,
    },
    ProcessAndSnapshot {
        input: InputFor<K, Model, Reason>,
        reply: oneshot::Sender<ProcessBatchFor<K, Model, Reason>>,
    },
    Snapshot {
        reply: oneshot::Sender<SnapshotFor<K, Model, Reason>>,
    },
    Shutdown,
}

/// Cloneable, bounded handle to a single-owner hypothesis engine task.
pub struct HypothesisServiceHandle<K, Model, Reason>
where
    Model: HypothesisModel<K>,
{
    sender: mpsc::Sender<ServiceCommand<K, Model, Reason>>,
}

impl<K, Model, Reason> Clone for HypothesisServiceHandle<K, Model, Reason>
where
    Model: HypothesisModel<K>,
{
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
        }
    }
}

impl<K, Model, Reason> HypothesisServiceHandle<K, Model, Reason>
where
    K: Clone + Ord + Send + 'static,
    Model: HypothesisModel<K> + Send + 'static,
    Model::Evidence: Send + 'static,
    Model::State: Clone + Send + 'static,
    Model::Output: Send + 'static,
    Model::Error: Send + 'static,
    Reason: Clone + Send + 'static,
{
    pub async fn snapshot(&self) -> Result<SnapshotFor<K, Model, Reason>, HypothesisServiceError>
    where
        K: Serialize + DeserializeOwned,
        Model::State: Serialize + DeserializeOwned,
        Reason: Serialize + DeserializeOwned,
    {
        let (reply, response) = oneshot::channel();
        self.sender
            .send(ServiceCommand::Snapshot { reply })
            .await
            .map_err(|_| HypothesisServiceError::Closed)?;
        response
            .await
            .map_err(|_| HypothesisServiceError::ResponseDropped)
    }

    pub async fn shutdown(&self) -> Result<(), HypothesisServiceError> {
        self.sender
            .send(ServiceCommand::Shutdown)
            .await
            .map_err(|_| HypothesisServiceError::Closed)
    }
}

impl<K, Model, Reason> HypothesisService for HypothesisServiceHandle<K, Model, Reason>
where
    K: Clone + Ord + Send + 'static,
    Model: HypothesisModel<K> + Send + 'static,
    Model::Evidence: Send + 'static,
    Model::State: Clone + Send + 'static,
    Model::Output: Send + 'static,
    Model::Error: Send + 'static,
    Reason: Clone + Send + 'static,
{
    type Input = InputFor<K, Model, Reason>;
    type Event = EventFor<K, Model, Reason>;
    type Error = HypothesisServiceError;

    fn process(
        &self,
        input: Self::Input,
    ) -> impl Future<Output = Result<Vec<Self::Event>, Self::Error>> + Send {
        let sender = self.sender.clone();
        async move {
            let (reply, response) = oneshot::channel();
            sender
                .send(ServiceCommand::Process { input, reply })
                .await
                .map_err(|_| HypothesisServiceError::Closed)?;
            response
                .await
                .map_err(|_| HypothesisServiceError::ResponseDropped)
        }
    }
}

impl<K, Model, Reason> SnapshottingHypothesisService for HypothesisServiceHandle<K, Model, Reason>
where
    K: Clone + Ord + Send + Serialize + DeserializeOwned + 'static,
    Model: HypothesisModel<K> + Send + 'static,
    Model::Evidence: Send + 'static,
    Model::State: Clone + Send + Serialize + DeserializeOwned + 'static,
    Model::Output: Send + 'static,
    Model::Error: Send + 'static,
    Reason: Clone + Send + Serialize + DeserializeOwned + 'static,
{
    type Snapshot = SnapshotFor<K, Model, Reason>;

    fn process_and_snapshot(
        &self,
        input: Self::Input,
    ) -> impl Future<Output = Result<HypothesisProcessBatch<Self::Event, Self::Snapshot>, Self::Error>>
           + Send {
        let sender = self.sender.clone();
        async move {
            let (reply, response) = oneshot::channel();
            sender
                .send(ServiceCommand::ProcessAndSnapshot { input, reply })
                .await
                .map_err(|_| HypothesisServiceError::Closed)?;
            response
                .await
                .map_err(|_| HypothesisServiceError::ResponseDropped)
        }
    }

    fn snapshot(&self) -> impl Future<Output = Result<Self::Snapshot, Self::Error>> + Send {
        HypothesisServiceHandle::snapshot(self)
    }
}

/// Owned worker task. Dropping this value aborts the task so it cannot leak silently.
pub struct HypothesisServiceTask<K, Model, Reason>
where
    Model: HypothesisModel<K>,
{
    join: Option<JoinHandle<HypothesisEngine<K, Model, Reason>>>,
}

pub type SpawnedHypothesisService<K, Model, Reason> = (
    HypothesisServiceHandle<K, Model, Reason>,
    HypothesisServiceTask<K, Model, Reason>,
);

impl<K, Model, Reason> HypothesisServiceTask<K, Model, Reason>
where
    Model: HypothesisModel<K>,
{
    pub async fn join(
        mut self,
    ) -> Result<HypothesisEngine<K, Model, Reason>, HypothesisServiceJoinError> {
        let join = self
            .join
            .take()
            .ok_or(HypothesisServiceJoinError::MissingJoinHandle)?;
        Ok(join.await?)
    }
}

impl<K, Model, Reason> Drop for HypothesisServiceTask<K, Model, Reason>
where
    Model: HypothesisModel<K>,
{
    fn drop(&mut self) {
        if let Some(join) = &self.join {
            join.abort();
        }
    }
}

/// Spawn a bounded actor whose worker exclusively owns the mutable engine.
pub fn spawn_hypothesis_service<K, Model, Reason>(
    mut engine: HypothesisEngine<K, Model, Reason>,
    mailbox_capacity: usize,
) -> Result<SpawnedHypothesisService<K, Model, Reason>, HypothesisServiceConfigError>
where
    K: Clone + Ord + Send + Serialize + DeserializeOwned + 'static,
    Model: HypothesisModel<K> + Send + 'static,
    Model::Evidence: Send + 'static,
    Model::State: Clone + Send + Serialize + DeserializeOwned + 'static,
    Model::Output: Send + 'static,
    Model::Error: Send + 'static,
    Reason: Clone + Send + Serialize + DeserializeOwned + 'static,
{
    if mailbox_capacity == 0 {
        return Err(HypothesisServiceConfigError::ZeroMailboxCapacity);
    }
    let (sender, mut receiver) = mpsc::channel(mailbox_capacity);
    let join = tokio::spawn(async move {
        while let Some(command) = receiver.recv().await {
            match command {
                ServiceCommand::Process { input, reply } => {
                    if !reply.is_closed() {
                        let events = engine.process(input);
                        let _ = reply.send(events);
                    }
                }
                ServiceCommand::ProcessAndSnapshot { input, reply } => {
                    if !reply.is_closed() {
                        let events = engine.process(input);
                        let snapshot = engine.snapshot();
                        let _ = reply.send(HypothesisProcessBatch { events, snapshot });
                    }
                }
                ServiceCommand::Snapshot { reply } => {
                    if !reply.is_closed() {
                        let _ = reply.send(engine.snapshot());
                    }
                }
                ServiceCommand::Shutdown => break,
            }
        }
        engine
    });
    Ok((
        HypothesisServiceHandle { sender },
        HypothesisServiceTask { join: Some(join) },
    ))
}

/// Mutex-backed adapter for an already shared application context.
///
/// The lock is held only while the pure transition runs and its events are copied out. Publish,
/// checkpoint storage, and other external I/O must happen after `process` returns.
pub struct SharedHypothesisEngine<K, Model, Reason>
where
    Model: HypothesisModel<K>,
{
    inner: Arc<Mutex<HypothesisEngine<K, Model, Reason>>>,
}

impl<K, Model, Reason> Clone for SharedHypothesisEngine<K, Model, Reason>
where
    Model: HypothesisModel<K>,
{
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<K, Model, Reason> SharedHypothesisEngine<K, Model, Reason>
where
    Model: HypothesisModel<K>,
{
    pub fn new(engine: HypothesisEngine<K, Model, Reason>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(engine)),
        }
    }
}

impl<K, Model, Reason> HypothesisService for SharedHypothesisEngine<K, Model, Reason>
where
    K: Clone + Ord + Send + 'static,
    Model: HypothesisModel<K> + Send + 'static,
    Model::Evidence: Send + 'static,
    Model::State: Clone + Send + 'static,
    Model::Output: Send + 'static,
    Model::Error: Send + 'static,
    Reason: Clone + Send + 'static,
{
    type Input = InputFor<K, Model, Reason>;
    type Event = EventFor<K, Model, Reason>;
    type Error = std::convert::Infallible;

    fn process(
        &self,
        input: Self::Input,
    ) -> impl Future<Output = Result<Vec<Self::Event>, Self::Error>> + Send {
        let inner = Arc::clone(&self.inner);
        async move {
            let mut engine = inner.lock().await;
            Ok(engine.process(input))
        }
    }
}

impl<K, Model, Reason> SnapshottingHypothesisService for SharedHypothesisEngine<K, Model, Reason>
where
    K: Clone + Ord + Send + Serialize + DeserializeOwned + 'static,
    Model: HypothesisModel<K> + Send + 'static,
    Model::Evidence: Send + 'static,
    Model::State: Clone + Send + Serialize + DeserializeOwned + 'static,
    Model::Output: Send + 'static,
    Model::Error: Send + 'static,
    Reason: Clone + Send + Serialize + DeserializeOwned + 'static,
{
    type Snapshot = SnapshotFor<K, Model, Reason>;

    fn process_and_snapshot(
        &self,
        input: Self::Input,
    ) -> impl Future<Output = Result<HypothesisProcessBatch<Self::Event, Self::Snapshot>, Self::Error>>
           + Send {
        let inner = Arc::clone(&self.inner);
        async move {
            let mut engine = inner.lock().await;
            let events = engine.process(input);
            let snapshot = engine.snapshot();
            Ok(HypothesisProcessBatch { events, snapshot })
        }
    }

    fn snapshot(&self) -> impl Future<Output = Result<Self::Snapshot, Self::Error>> + Send {
        let inner = Arc::clone(&self.inner);
        async move {
            let engine = inner.lock().await;
            Ok(engine.snapshot())
        }
    }
}

#[cfg(test)]
mod tests {
    use helio_time::{AvailableAt, EffectiveAt};
    use serde::{Deserialize, Serialize};

    use super::*;
    use crate::{
        CausalEvidence, HypothesisConfig, HypothesisModel, HypothesisTransition,
        KeyedHypothesisMachine,
    };

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    struct State(u64);

    #[derive(Debug, Clone, Copy)]
    struct Model;

    impl HypothesisModel<u64> for Model {
        type Evidence = u64;
        type State = State;
        type Output = u64;
        type Error = std::convert::Infallible;

        fn open(
            &self,
            _key: &u64,
            evidence: CausalEvidence<Self::Evidence>,
        ) -> Result<HypothesisTransition<Self::State, Self::Output>, Self::Error> {
            Ok(HypothesisTransition::new(State(evidence.payload)).emit(evidence.payload))
        }

        fn update(
            &self,
            _key: &u64,
            state: &Self::State,
            evidence: CausalEvidence<Self::Evidence>,
        ) -> Result<HypothesisTransition<Self::State, Self::Output>, Self::Error> {
            let next = State(state.0 + evidence.payload);
            Ok(HypothesisTransition::new(next).emit(next.0))
        }

        fn on_timer(
            &self,
            _key: &u64,
            state: &Self::State,
            _timer_id: crate::TimerId,
            _at: AvailableAt,
        ) -> Result<HypothesisTransition<Self::State, Self::Output>, Self::Error> {
            Ok(HypothesisTransition::new(*state))
        }

        fn validate(&self, _key: &u64, _state: &Self::State) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    type TestEngine = HypothesisEngine<u64, Model, String>;

    fn engine() -> TestEngine {
        let machine = KeyedHypothesisMachine::try_new(
            Model,
            HypothesisConfig::try_new(8, 8, 2, 8, 8).unwrap(),
        )
        .unwrap();
        HypothesisEngine::new(machine)
    }

    fn open(key: u64, value: u64) -> HypothesisInput<u64, u64, String> {
        HypothesisInput::Open {
            key,
            evidence: CausalEvidence::new(0, EffectiveAt(1), AvailableAt(2), value),
        }
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    #[test]
    fn bounded_actor_owns_state_and_returns_it_on_shutdown() {
        runtime().block_on(async {
            let (service, task) = spawn_hypothesis_service(engine(), 4).unwrap();
            let events = service.process(open(7, 11)).await.unwrap();
            assert!(events.iter().any(|event| matches!(
                event,
                HypothesisEvent::ModelOutput {
                    key: 7,
                    output: 11,
                    ..
                }
            )));
            let snapshot = service.snapshot().await.unwrap();
            assert_eq!(snapshot.active.get(&7).unwrap().model_state, State(11));

            service.shutdown().await.unwrap();
            let engine = task.join().await.unwrap();
            assert_eq!(engine.state().get(&7).unwrap().model_state, State(11));
        });
    }

    #[test]
    fn cloned_handles_share_one_typed_capability() {
        runtime().block_on(async {
            let (service, task) = spawn_hypothesis_service(engine(), 2).unwrap();
            let clone = service.clone();
            service.process(open(1, 3)).await.unwrap();
            clone.process(open(2, 5)).await.unwrap();
            let snapshot = clone.snapshot().await.unwrap();
            assert_eq!(snapshot.active.len(), 2);
            service.shutdown().await.unwrap();
            task.join().await.unwrap();
        });
    }

    #[test]
    fn process_and_snapshot_returns_one_non_interleaved_state_boundary() {
        runtime().block_on(async {
            let (service, task) = spawn_hypothesis_service(engine(), 2).unwrap();
            let batch = service.process_and_snapshot(open(9, 17)).await.unwrap();
            assert!(batch.events.iter().any(|event| matches!(
                event,
                HypothesisEvent::ModelOutput {
                    key: 9,
                    output: 17,
                    ..
                }
            )));
            assert_eq!(
                batch.snapshot.active.get(&9).unwrap().model_state,
                State(17)
            );
            service.shutdown().await.unwrap();
            task.join().await.unwrap();
        });
    }

    #[test]
    fn mutex_adapter_releases_a_typed_event_batch() {
        runtime().block_on(async {
            let service = SharedHypothesisEngine::new(engine());
            let events = service.process(open(3, 13)).await.unwrap();
            assert!(matches!(
                events.last(),
                Some(HypothesisEvent::ModelOutput { output: 13, .. })
            ));
        });
    }

    #[test]
    fn zero_mailbox_capacity_is_rejected() {
        assert!(matches!(
            spawn_hypothesis_service(engine(), 0),
            Err(HypothesisServiceConfigError::ZeroMailboxCapacity)
        ));
    }
}
