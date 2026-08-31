use crate::types::{
    CommandRequest, CommandStatus, EntityHistoryEvent, HistoryPage, OperationsSnapshot,
    SavedWorkspace,
};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use tokio::sync::{broadcast, RwLock};

const SNAPSHOT_SUBSCRIBERS: usize = 256;
const MAX_HISTORY_EVENTS: usize = 100_000;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("invalid operations snapshot: {0}")]
    InvalidSnapshot(String),
    #[error("replacement snapshot sequence must advance monotonically")]
    SequenceRegression,
    #[error("replacement snapshot changed the account identity")]
    AccountIdentityChanged,
    #[error("clock formatting failed")]
    Clock,
    #[error("workspace revision conflict")]
    WorkspaceConflict,
    #[error("workspace owner does not match the authenticated operator")]
    WorkspaceOwnerMismatch,
}

#[derive(Clone, Debug)]
pub struct CommandOutcome {
    pub status: CommandStatus,
    pub message: String,
}

#[derive(Clone, Debug, Error)]
pub enum CommandExecutionError {
    #[error("command execution is unavailable")]
    Unavailable,
    #[error("command failed validation: {0}")]
    Invalid(String),
    #[error("command infrastructure failed: {0}")]
    Infrastructure(String),
}

#[async_trait]
pub trait CommandExecutor: Send + Sync {
    async fn execute(
        &self,
        actor: &str,
        command: &CommandRequest,
        store: &OperatorStore,
    ) -> Result<CommandOutcome, CommandExecutionError>;
}

#[derive(Debug, Default)]
pub struct ReadOnlyCommandExecutor;

#[async_trait]
impl CommandExecutor for ReadOnlyCommandExecutor {
    async fn execute(
        &self,
        _actor: &str,
        _command: &CommandRequest,
        _store: &OperatorStore,
    ) -> Result<CommandOutcome, CommandExecutionError> {
        Ok(CommandOutcome {
            status: CommandStatus::Rejected,
            message: "No admitted broker command executor is attached".into(),
        })
    }
}

pub struct OperatorStore {
    snapshot: RwLock<OperationsSnapshot>,
    snapshots: broadcast::Sender<OperationsSnapshot>,
    history: RwLock<Vec<EntityHistoryEvent>>,
    workspaces: RwLock<HashMap<String, SavedWorkspace>>,
}

impl std::fmt::Debug for OperatorStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OperatorStore")
            .field("snapshot_subscribers", &self.snapshots.receiver_count())
            .finish_non_exhaustive()
    }
}

impl OperatorStore {
    pub fn new(snapshot: OperationsSnapshot) -> Result<Arc<Self>, StoreError> {
        snapshot.validate().map_err(StoreError::InvalidSnapshot)?;
        let (snapshots, _) = broadcast::channel(SNAPSHOT_SUBSCRIBERS);
        Ok(Arc::new(Self {
            snapshot: RwLock::new(snapshot),
            snapshots,
            history: RwLock::new(Vec::new()),
            workspaces: RwLock::new(HashMap::new()),
        }))
    }

    pub async fn snapshot(&self) -> OperationsSnapshot {
        self.snapshot.read().await.clone()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<OperationsSnapshot> {
        self.snapshots.subscribe()
    }

    pub async fn replace_snapshot(&self, next: OperationsSnapshot) -> Result<(), StoreError> {
        next.validate().map_err(StoreError::InvalidSnapshot)?;
        let mut current = self.snapshot.write().await;
        if next.sequence <= current.sequence {
            return Err(StoreError::SequenceRegression);
        }
        if next.context.account_id != current.context.account_id
            || next.context.organization_id != current.context.organization_id
        {
            return Err(StoreError::AccountIdentityChanged);
        }
        *current = next.clone();
        drop(current);
        let _ = self.snapshots.send(next);
        Ok(())
    }

    pub async fn mutate_snapshot<F>(&self, mutation: F) -> Result<OperationsSnapshot, StoreError>
    where
        F: FnOnce(&mut OperationsSnapshot) -> Result<(), StoreError>,
    {
        let mut snapshot = self.snapshot.write().await;
        let mut next = snapshot.clone();
        mutation(&mut next)?;
        next.sequence = snapshot.sequence.saturating_add(1);
        next.observed_at = now()?;
        next.validate().map_err(StoreError::InvalidSnapshot)?;
        *snapshot = next.clone();
        drop(snapshot);
        let _ = self.snapshots.send(next.clone());
        Ok(next)
    }

    pub async fn append_history(&self, mut event: EntityHistoryEvent) -> Result<u64, StoreError> {
        let mut history = self.history.write().await;
        let cursor = history
            .last()
            .map_or(1, |last| last.cursor.saturating_add(1));
        event.cursor = cursor;
        history.push(event);
        if history.len() > MAX_HISTORY_EVENTS {
            let excess = history.len() - MAX_HISTORY_EVENTS;
            history.drain(0..excess);
        }
        Ok(cursor)
    }

    pub async fn history(
        &self,
        entity_kind: &str,
        entity_id: &str,
        after: u64,
        limit: usize,
    ) -> HistoryPage {
        let history = self.history.read().await;
        let limit = limit.clamp(1, 500);
        let mut matching = history
            .iter()
            .filter(|event| {
                event.cursor > after
                    && event.entity_kind == entity_kind
                    && event.entity_id == entity_id
            })
            .cloned();
        let events: Vec<_> = matching.by_ref().take(limit).collect();
        let next_cursor = matching
            .next()
            .and_then(|_| events.last().map(|event| event.cursor));
        HistoryPage {
            schema_version: 1,
            events,
            next_cursor,
        }
    }

    pub async fn workspace(&self, id: &str) -> Option<SavedWorkspace> {
        self.workspaces.read().await.get(id).cloned()
    }

    pub async fn save_workspace(
        &self,
        actor: &str,
        mut workspace: SavedWorkspace,
        expected_revision: Option<u64>,
    ) -> Result<SavedWorkspace, StoreError> {
        if workspace.owner != actor {
            return Err(StoreError::WorkspaceOwnerMismatch);
        }
        let mut workspaces = self.workspaces.write().await;
        let current = workspaces.get(&workspace.workspace_id);
        match (current, expected_revision) {
            (None, None | Some(0)) => workspace.revision = 1,
            (Some(current), Some(expected)) if current.revision == expected => {
                workspace.revision = current.revision.saturating_add(1)
            }
            _ => return Err(StoreError::WorkspaceConflict),
        }
        workspace.updated_at = now()?;
        workspaces.insert(workspace.workspace_id.clone(), workspace.clone());
        Ok(workspace)
    }
}

pub fn now() -> Result<String, StoreError> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|_| StoreError::Clock)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::empty_snapshot;

    #[tokio::test]
    async fn replacement_is_monotonic_and_account_scoped() {
        let store = OperatorStore::new(empty_snapshot()).unwrap();
        let mut regressed = store.snapshot().await;
        assert!(matches!(
            store.replace_snapshot(regressed.clone()).await,
            Err(StoreError::SequenceRegression)
        ));
        regressed.sequence += 1;
        regressed.context.account_id = "other".into();
        assert!(matches!(
            store.replace_snapshot(regressed).await,
            Err(StoreError::AccountIdentityChanged)
        ));
    }

    #[tokio::test]
    async fn mutation_publishes_a_complete_advanced_snapshot() {
        let store = OperatorStore::new(empty_snapshot()).unwrap();
        let mut receiver = store.subscribe();
        let next = store
            .mutate_snapshot(|snapshot| {
                snapshot.risk.kill_switch_active = true;
                Ok(())
            })
            .await
            .unwrap();
        assert_eq!(next.sequence, 2);
        assert!(receiver.recv().await.unwrap().risk.kill_switch_active);
    }

    #[tokio::test]
    async fn workspace_writes_are_owner_and_revision_guarded() {
        let store = OperatorStore::new(empty_snapshot()).unwrap();
        let workspace = SavedWorkspace {
            schema_version: 1,
            workspace_id: "desk".into(),
            owner: "operator".into(),
            scope: "user".into(),
            name: "Execution".into(),
            revision: 0,
            updated_at: "ignored".into(),
            definition: serde_json::json!({"lanes": []}),
        };
        let created = store
            .save_workspace("operator", workspace.clone(), None)
            .await
            .unwrap();
        assert_eq!(created.revision, 1);
        assert!(matches!(
            store
                .save_workspace("operator", workspace.clone(), Some(0))
                .await,
            Err(StoreError::WorkspaceConflict)
        ));
        assert!(matches!(
            store.save_workspace("other", workspace, Some(1)).await,
            Err(StoreError::WorkspaceOwnerMismatch)
        ));
    }
}
