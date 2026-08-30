//! Versioned, finite-coverage venue schedules imported from an authoritative calendar source.
//!
//! This deliberately does not fall back to weekday arithmetic. A query outside the imported
//! coverage is a typed error so an execution service cannot invent a session.

use helio_scan::SessionDate;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VenueScheduleMetadata {
    pub schema_version: u32,
    pub venue: String,
    pub timezone: String,
    pub source: String,
    pub source_version: String,
    pub source_sha256: String,
    pub generated_at_utc: i64,
    pub valid_from_utc: i64,
    pub valid_until_utc: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct VenueBreak {
    pub start_utc: i64,
    pub end_utc: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VenueSession {
    pub label: SessionDate,
    pub open_utc: i64,
    pub close_utc: i64,
    #[serde(default)]
    pub breaks: Vec<VenueBreak>,
}

impl VenueSession {
    pub fn is_open_at(&self, timestamp: i64) -> bool {
        timestamp >= self.open_utc
            && timestamp < self.close_utc
            && !self
                .breaks
                .iter()
                .any(|pause| timestamp >= pause.start_utc && timestamp < pause.end_utc)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VenueSchedule {
    pub metadata: VenueScheduleMetadata,
    pub sessions: Vec<VenueSession>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum VenueScheduleError {
    #[error("venue schedule metadata field {0} must not be empty")]
    EmptyMetadata(&'static str),
    #[error("venue schedule schema version {0} is unsupported")]
    UnsupportedSchema(u32),
    #[error("venue schedule source digest must be 64 lowercase hexadecimal characters")]
    InvalidDigest,
    #[error("venue schedule content does not match its source digest")]
    DigestMismatch,
    #[error("venue schedule content could not be serialized for digest verification")]
    DigestSerialization,
    #[error("venue schedule coverage must be a non-empty half-open UTC interval")]
    InvalidCoverage,
    #[error("venue schedule must contain at least one session")]
    EmptySessions,
    #[error("venue session labels and UTC opens must be strictly increasing")]
    UnorderedSessions,
    #[error("venue session {0:?} has an invalid open/close interval")]
    InvalidSession(SessionDate),
    #[error("venue session {0:?} falls outside declared coverage")]
    SessionOutsideCoverage(SessionDate),
    #[error("venue session {0:?} contains an invalid or overlapping break")]
    InvalidBreak(SessionDate),
    #[error("timestamp {timestamp} lies outside venue schedule coverage [{start}, {end})")]
    TimestampOutsideCoverage {
        timestamp: i64,
        start: i64,
        end: i64,
    },
    #[error("session {0:?} is not present in the imported venue schedule")]
    UnknownSession(SessionDate),
    #[error("session {0:?} has no following session inside imported coverage")]
    NoFollowingSession(SessionDate),
}

impl VenueSchedule {
    pub fn try_new(
        metadata: VenueScheduleMetadata,
        sessions: Vec<VenueSession>,
    ) -> Result<Self, VenueScheduleError> {
        validate_metadata(&metadata)?;
        if compute_source_sha256(&metadata, &sessions)? != metadata.source_sha256 {
            return Err(VenueScheduleError::DigestMismatch);
        }
        if sessions.is_empty() {
            return Err(VenueScheduleError::EmptySessions);
        }
        let mut previous: Option<&VenueSession> = None;
        for session in &sessions {
            if session.open_utc >= session.close_utc {
                return Err(VenueScheduleError::InvalidSession(session.label));
            }
            if session.open_utc < metadata.valid_from_utc
                || session.close_utc > metadata.valid_until_utc
            {
                return Err(VenueScheduleError::SessionOutsideCoverage(session.label));
            }
            if let Some(prior) = previous {
                if prior.label >= session.label
                    || prior.open_utc >= session.open_utc
                    || prior.close_utc > session.open_utc
                {
                    return Err(VenueScheduleError::UnorderedSessions);
                }
            }
            validate_breaks(session)?;
            previous = Some(session);
        }
        Ok(Self { metadata, sessions })
    }

    pub fn validate(&self) -> Result<(), VenueScheduleError> {
        Self::try_new(self.metadata.clone(), self.sessions.clone()).map(|_| ())
    }

    pub fn session(&self, label: SessionDate) -> Result<&VenueSession, VenueScheduleError> {
        self.sessions
            .binary_search_by_key(&label, |session| session.label)
            .map(|index| &self.sessions[index])
            .map_err(|_| VenueScheduleError::UnknownSession(label))
    }

    pub fn next_session(&self, label: SessionDate) -> Result<&VenueSession, VenueScheduleError> {
        let index = self
            .sessions
            .binary_search_by_key(&label, |session| session.label)
            .map_err(|_| VenueScheduleError::UnknownSession(label))?;
        self.sessions
            .get(index + 1)
            .ok_or(VenueScheduleError::NoFollowingSession(label))
    }

    pub fn active_session_at(
        &self,
        timestamp: i64,
    ) -> Result<Option<&VenueSession>, VenueScheduleError> {
        self.require_coverage(timestamp)?;
        let index = self
            .sessions
            .partition_point(|session| session.open_utc <= timestamp);
        if index == 0 {
            return Ok(None);
        }
        let session = &self.sessions[index - 1];
        Ok(session.is_open_at(timestamp).then_some(session))
    }

    pub fn session_on_or_after(
        &self,
        timestamp: i64,
    ) -> Result<Option<&VenueSession>, VenueScheduleError> {
        self.require_coverage(timestamp)?;
        let index = self
            .sessions
            .partition_point(|session| session.close_utc <= timestamp);
        Ok(self.sessions.get(index))
    }

    pub fn require_coverage(&self, timestamp: i64) -> Result<(), VenueScheduleError> {
        if timestamp < self.metadata.valid_from_utc || timestamp >= self.metadata.valid_until_utc {
            Err(VenueScheduleError::TimestampOutsideCoverage {
                timestamp,
                start: self.metadata.valid_from_utc,
                end: self.metadata.valid_until_utc,
            })
        } else {
            Ok(())
        }
    }
}

/// Canonical SHA-256 over the imported source fields and sessions, excluding the digest itself.
/// Python's exporter uses the same sorted, whitespace-free JSON representation.
pub fn compute_source_sha256(
    metadata: &VenueScheduleMetadata,
    sessions: &[VenueSession],
) -> Result<String, VenueScheduleError> {
    let payload = json!({
        "schema_version": metadata.schema_version,
        "venue": metadata.venue.as_str(),
        "timezone": metadata.timezone.as_str(),
        "source": metadata.source.as_str(),
        "source_version": metadata.source_version.as_str(),
        "generated_at_utc": metadata.generated_at_utc,
        "valid_from_utc": metadata.valid_from_utc,
        "valid_until_utc": metadata.valid_until_utc,
        "sessions": sessions,
    });
    let canonical =
        serde_json::to_vec(&payload).map_err(|_| VenueScheduleError::DigestSerialization)?;
    Ok(format!("{:x}", Sha256::digest(canonical)))
}

fn validate_metadata(metadata: &VenueScheduleMetadata) -> Result<(), VenueScheduleError> {
    if metadata.schema_version != 1 {
        return Err(VenueScheduleError::UnsupportedSchema(
            metadata.schema_version,
        ));
    }
    for (name, value) in [
        ("venue", metadata.venue.as_str()),
        ("timezone", metadata.timezone.as_str()),
        ("source", metadata.source.as_str()),
        ("source_version", metadata.source_version.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(VenueScheduleError::EmptyMetadata(name));
        }
    }
    if metadata.source_sha256.len() != 64
        || !metadata
            .source_sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(VenueScheduleError::InvalidDigest);
    }
    if metadata.valid_from_utc >= metadata.valid_until_utc {
        return Err(VenueScheduleError::InvalidCoverage);
    }
    Ok(())
}

fn validate_breaks(session: &VenueSession) -> Result<(), VenueScheduleError> {
    let mut previous_end = session.open_utc;
    for pause in &session.breaks {
        if pause.start_utc < session.open_utc
            || pause.start_utc >= pause.end_utc
            || pause.end_utc > session.close_utc
            || pause.start_utc < previous_end
        {
            return Err(VenueScheduleError::InvalidBreak(session.label));
        }
        previous_end = pause.end_utc;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata() -> VenueScheduleMetadata {
        VenueScheduleMetadata {
            schema_version: 1,
            venue: "XNYS".into(),
            timezone: "America/New_York".into(),
            source: "exchange_calendars".into(),
            source_version: "4.13.2".into(),
            source_sha256: "a".repeat(64),
            generated_at_utc: 1_790_000_000,
            valid_from_utc: 1_793_059_200,
            valid_until_utc: 1_793_664_000,
        }
    }

    fn sessions() -> Vec<VenueSession> {
        vec![
            VenueSession {
                label: SessionDate(20_417),
                open_utc: 1_793_118_600,
                close_utc: 1_793_142_000,
                breaks: vec![],
            },
            VenueSession {
                label: SessionDate(20_419),
                open_utc: 1_793_291_400,
                close_utc: 1_793_304_000,
                breaks: vec![],
            },
        ]
    }

    fn schedule() -> VenueSchedule {
        let sessions = sessions();
        let mut metadata = metadata();
        metadata.source_sha256 = compute_source_sha256(&metadata, &sessions).unwrap();
        VenueSchedule::try_new(metadata, sessions).unwrap()
    }

    #[test]
    fn finite_schedule_preserves_holiday_gap_and_early_close() {
        let schedule = schedule();
        assert_eq!(
            schedule.next_session(SessionDate(20_417)).unwrap().label,
            SessionDate(20_419)
        );
        assert_eq!(
            schedule.session(SessionDate(20_419)).unwrap().close_utc
                - schedule.session(SessionDate(20_419)).unwrap().open_utc,
            12_600
        );
    }

    #[test]
    fn out_of_coverage_and_closed_hours_fail_closed() {
        let schedule = schedule();
        assert!(matches!(
            schedule.active_session_at(1_800_000_000),
            Err(VenueScheduleError::TimestampOutsideCoverage { .. })
        ));
        assert_eq!(schedule.active_session_at(1_793_100_000).unwrap(), None);
        assert_eq!(
            schedule
                .active_session_at(1_793_120_000)
                .unwrap()
                .unwrap()
                .label,
            SessionDate(20_417)
        );
    }

    #[test]
    fn corrupt_schedule_is_rejected_before_use() {
        let mut sessions = schedule().sessions;
        sessions[1].open_utc = sessions[0].close_utc - 1;
        let mut metadata = metadata();
        metadata.source_sha256 = compute_source_sha256(&metadata, &sessions).unwrap();
        assert_eq!(
            VenueSchedule::try_new(metadata, sessions),
            Err(VenueScheduleError::UnorderedSessions)
        );
    }

    #[test]
    fn changed_content_is_rejected_when_digest_is_not_regenerated() {
        let mut schedule = schedule();
        schedule.sessions[0].close_utc -= 1;
        assert_eq!(
            VenueSchedule::try_new(schedule.metadata, schedule.sessions),
            Err(VenueScheduleError::DigestMismatch)
        );
    }
}
