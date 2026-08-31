use golem_rust::{Schema, agent_definition, agent_implementation};
use serde::{Deserialize, Serialize};

const SNAPSHOT_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Schema, Serialize, Deserialize)]
pub struct ProjectionCursorStatus {
    pub account_id: String,
    pub projection_id: String,
    pub cursor: u64,
    pub last_event_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Schema, Serialize, Deserialize)]
pub struct AdvanceProjectionCursorInput {
    pub expected_cursor: u64,
    pub next_cursor: u64,
    pub event_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Schema, Serialize, Deserialize)]
pub struct ProjectionCursorReceipt {
    pub cursor: u64,
    pub event_id: String,
    pub replayed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Schema, Serialize, Deserialize)]
pub enum ProjectionCursorError {
    InvalidIdentity {
        detail: String,
    },
    EmptyEventIdentity,
    NonContiguousAdvance {
        expected: u64,
        proposed: u64,
    },
    CursorConflict {
        expected: u64,
        actual: u64,
    },
    ReplayIdentityConflict {
        cursor: u64,
        existing_event_id: String,
        proposed_event_id: String,
    },
}

#[agent_definition(snapshotting = "periodic(30s)")]
pub trait ProjectionCursorAgent {
    fn new(account_id: String, projection_id: String) -> Self;

    fn status(&self) -> Result<ProjectionCursorStatus, ProjectionCursorError>;

    /// Compare-and-set after a downstream durable publish acknowledgement.
    fn advance(
        &mut self,
        input: AdvanceProjectionCursorInput,
    ) -> Result<ProjectionCursorReceipt, ProjectionCursorError>;
}

struct ProjectionCursorAgentImpl {
    account_id: String,
    projection_id: String,
    cursor: u64,
    last_event_id: Option<String>,
    initialization_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AgentSnapshot {
    format_version: u32,
    account_id: String,
    projection_id: String,
    cursor: u64,
    last_event_id: Option<String>,
}

#[agent_implementation]
impl ProjectionCursorAgent for ProjectionCursorAgentImpl {
    fn new(account_id: String, projection_id: String) -> Self {
        let initialization_error = (account_id.trim().is_empty()
            || projection_id.trim().is_empty())
        .then(|| "account and projection identities must not be empty".into());
        Self {
            account_id,
            projection_id,
            cursor: 0,
            last_event_id: None,
            initialization_error,
        }
    }

    fn status(&self) -> Result<ProjectionCursorStatus, ProjectionCursorError> {
        self.validate_identity()?;
        Ok(ProjectionCursorStatus {
            account_id: self.account_id.clone(),
            projection_id: self.projection_id.clone(),
            cursor: self.cursor,
            last_event_id: self.last_event_id.clone(),
        })
    }

    fn advance(
        &mut self,
        input: AdvanceProjectionCursorInput,
    ) -> Result<ProjectionCursorReceipt, ProjectionCursorError> {
        self.validate_identity()?;
        if input.event_id.trim().is_empty() {
            return Err(ProjectionCursorError::EmptyEventIdentity);
        }
        if input.next_cursor == self.cursor {
            return match &self.last_event_id {
                Some(existing) if existing == &input.event_id => Ok(ProjectionCursorReceipt {
                    cursor: self.cursor,
                    event_id: existing.clone(),
                    replayed: true,
                }),
                Some(existing) => Err(ProjectionCursorError::ReplayIdentityConflict {
                    cursor: self.cursor,
                    existing_event_id: existing.clone(),
                    proposed_event_id: input.event_id,
                }),
                None => Err(ProjectionCursorError::CursorConflict {
                    expected: input.expected_cursor,
                    actual: self.cursor,
                }),
            };
        }
        if input.expected_cursor != self.cursor {
            return Err(ProjectionCursorError::CursorConflict {
                expected: input.expected_cursor,
                actual: self.cursor,
            });
        }
        let expected_next =
            self.cursor
                .checked_add(1)
                .ok_or(ProjectionCursorError::NonContiguousAdvance {
                    expected: self.cursor,
                    proposed: input.next_cursor,
                })?;
        if input.next_cursor != expected_next {
            return Err(ProjectionCursorError::NonContiguousAdvance {
                expected: expected_next,
                proposed: input.next_cursor,
            });
        }
        self.cursor = input.next_cursor;
        self.last_event_id = Some(input.event_id.clone());
        Ok(ProjectionCursorReceipt {
            cursor: self.cursor,
            event_id: input.event_id,
            replayed: false,
        })
    }

    async fn save_snapshot(&self) -> Result<Vec<u8>, String> {
        serde_json::to_vec(&AgentSnapshot {
            format_version: SNAPSHOT_FORMAT_VERSION,
            account_id: self.account_id.clone(),
            projection_id: self.projection_id.clone(),
            cursor: self.cursor,
            last_event_id: self.last_event_id.clone(),
        })
        .map_err(|error| format!("failed to encode projection cursor snapshot: {error}"))
    }

    async fn load_snapshot(&mut self, bytes: Vec<u8>) -> Result<(), String> {
        let snapshot: AgentSnapshot = serde_json::from_slice(&bytes)
            .map_err(|error| format!("failed to decode projection cursor snapshot: {error}"))?;
        if snapshot.format_version != SNAPSHOT_FORMAT_VERSION {
            return Err(format!(
                "unsupported projection cursor snapshot version {}; expected {}",
                snapshot.format_version, SNAPSHOT_FORMAT_VERSION
            ));
        }
        if snapshot.account_id != self.account_id || snapshot.projection_id != self.projection_id {
            return Err("projection cursor snapshot identity does not match agent identity".into());
        }
        if snapshot.cursor == 0 && snapshot.last_event_id.is_some()
            || snapshot.cursor > 0 && snapshot.last_event_id.is_none()
        {
            return Err("projection cursor snapshot state is inconsistent".into());
        }
        self.cursor = snapshot.cursor;
        self.last_event_id = snapshot.last_event_id;
        self.initialization_error = None;
        Ok(())
    }
}

impl ProjectionCursorAgentImpl {
    fn validate_identity(&self) -> Result<(), ProjectionCursorError> {
        self.initialization_error.as_ref().map_or(Ok(()), |detail| {
            Err(ProjectionCursorError::InvalidIdentity {
                detail: detail.clone(),
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_advance_is_contiguous_compare_and_set_and_replay_safe() {
        let mut cursor = ProjectionCursorAgentImpl::new("paper-account".into(), "nats".into());
        let input = AdvanceProjectionCursorInput {
            expected_cursor: 0,
            next_cursor: 1,
            event_id: "event-1".into(),
        };
        assert!(!cursor.advance(input.clone()).unwrap().replayed);
        assert!(cursor.advance(input).unwrap().replayed);
        assert!(matches!(
            cursor.advance(AdvanceProjectionCursorInput {
                expected_cursor: 0,
                next_cursor: 2,
                event_id: "event-2".into(),
            }),
            Err(ProjectionCursorError::CursorConflict { .. })
        ));
        assert_eq!(cursor.status().unwrap().cursor, 1);
    }
}
