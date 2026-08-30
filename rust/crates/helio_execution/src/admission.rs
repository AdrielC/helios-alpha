use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use thiserror::Error;

use crate::{ReadinessBlocker, ReadinessReport};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum EvidenceKind {
    AtomicCrashMatrix,
    VenueCalendarConformance,
    BrokerCertification,
    BrokerReconciliationFaultInjection,
    RiskLimitFaultInjection,
    CostCapacityCalibration,
    ObservabilityAlertDrill,
    IncidentResponseExercise,
    GolemRestartRecovery,
    DeploymentVerification,
    ShadowRun,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EvidenceArtifact {
    pub kind: EvidenceKind,
    pub artifact_id: String,
    pub sha256: String,
    pub environment: String,
    pub observed_at_ns: u64,
    pub expires_at_ns: u64,
    pub passed: bool,
}

#[derive(Debug, Clone, Default)]
pub struct EvidenceLedger {
    artifacts: BTreeMap<EvidenceKind, EvidenceArtifact>,
}

impl EvidenceLedger {
    pub fn record(&mut self, artifact: EvidenceArtifact) {
        self.artifacts.insert(artifact.kind, artifact);
    }

    pub fn get(&self, kind: EvidenceKind) -> Option<&EvidenceArtifact> {
        self.artifacts.get(&kind)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapitalAdmissionPolicy {
    version: String,
    environment: String,
    authorization_ttl_ns: u64,
    max_operational_snapshot_age_ns: u64,
    required_evidence: BTreeSet<EvidenceKind>,
}

impl CapitalAdmissionPolicy {
    pub fn production_default(version: impl Into<String>, authorization_ttl_ns: u64) -> Self {
        Self {
            version: version.into(),
            environment: "production".into(),
            authorization_ttl_ns,
            max_operational_snapshot_age_ns: authorization_ttl_ns,
            required_evidence: mandatory_production_evidence(),
        }
    }

    pub fn required_evidence(&self) -> &BTreeSet<EvidenceKind> {
        &self.required_evidence
    }
}

pub fn mandatory_production_evidence() -> BTreeSet<EvidenceKind> {
    BTreeSet::from([
        EvidenceKind::AtomicCrashMatrix,
        EvidenceKind::VenueCalendarConformance,
        EvidenceKind::BrokerCertification,
        EvidenceKind::BrokerReconciliationFaultInjection,
        EvidenceKind::RiskLimitFaultInjection,
        EvidenceKind::CostCapacityCalibration,
        EvidenceKind::ObservabilityAlertDrill,
        EvidenceKind::IncidentResponseExercise,
        EvidenceKind::GolemRestartRecovery,
        EvidenceKind::DeploymentVerification,
        EvidenceKind::ShadowRun,
    ])
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum AdmissionBlocker {
    InvalidProductionPolicy,
    MissingPolicyRequirement(EvidenceKind),
    Operations(ReadinessBlocker),
    MissingEvidence(EvidenceKind),
    EvidenceFailed(EvidenceKind),
    EvidenceExpired(EvidenceKind),
    EvidenceFromFuture(EvidenceKind),
    EvidenceEnvironmentMismatch(EvidenceKind),
    InvalidArtifactIdentity(EvidenceKind),
    InvalidSha256(EvidenceKind),
    AuthorizationExpiryOverflow,
    OperationalSnapshotFromFuture,
    OperationalSnapshotStale,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapitalAdmissionReport {
    pub policy_version: String,
    pub evaluated_at_ns: u64,
    pub authorization_expires_at_ns: Option<u64>,
    pub blockers: Vec<AdmissionBlocker>,
}

impl CapitalAdmissionReport {
    pub fn admitted(&self) -> bool {
        self.blockers.is_empty() && self.authorization_expires_at_ns.is_some()
    }
}

/// Unforgeable outside this crate because every field is private and no deserializer is exposed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CapitalAuthorization {
    authorization_id: String,
    policy_version: String,
    environment: String,
    issued_at_ns: u64,
    expires_at_ns: u64,
}

impl CapitalAuthorization {
    pub fn authorization_id(&self) -> &str {
        &self.authorization_id
    }

    pub const fn expires_at_ns(&self) -> u64 {
        self.expires_at_ns
    }

    pub fn permits(&self, environment: &str, now_ns: u64) -> bool {
        self.environment == environment
            && self.issued_at_ns <= now_ns
            && now_ns < self.expires_at_ns
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("capital admission denied")]
pub struct CapitalAdmissionDenied {
    pub report: CapitalAdmissionReport,
}

pub fn evaluate_capital_admission(
    policy: &CapitalAdmissionPolicy,
    evidence: &EvidenceLedger,
    readiness: &ReadinessReport,
    now_ns: u64,
) -> Result<CapitalAuthorization, CapitalAdmissionDenied> {
    let mut blockers: Vec<_> = readiness
        .blockers
        .iter()
        .cloned()
        .map(AdmissionBlocker::Operations)
        .collect();
    if policy.version.trim().is_empty()
        || policy.environment != "production"
        || policy.authorization_ttl_ns == 0
    {
        blockers.push(AdmissionBlocker::InvalidProductionPolicy);
    }
    match now_ns.checked_sub(readiness.observed_at_ns) {
        None => blockers.push(AdmissionBlocker::OperationalSnapshotFromFuture),
        Some(age) if age > policy.max_operational_snapshot_age_ns => {
            blockers.push(AdmissionBlocker::OperationalSnapshotStale);
        }
        Some(_) => {}
    }
    for kind in mandatory_production_evidence() {
        if !policy.required_evidence.contains(&kind) {
            blockers.push(AdmissionBlocker::MissingPolicyRequirement(kind));
        }
    }
    let requested_expiry = now_ns.checked_add(policy.authorization_ttl_ns);
    if requested_expiry.is_none() {
        blockers.push(AdmissionBlocker::AuthorizationExpiryOverflow);
    }
    let mut expires_at_ns = requested_expiry.unwrap_or(now_ns);

    for kind in &policy.required_evidence {
        let Some(artifact) = evidence.get(*kind) else {
            blockers.push(AdmissionBlocker::MissingEvidence(*kind));
            continue;
        };
        if artifact.artifact_id.trim().is_empty() {
            blockers.push(AdmissionBlocker::InvalidArtifactIdentity(*kind));
        }
        if !is_lower_hex_sha256(&artifact.sha256) {
            blockers.push(AdmissionBlocker::InvalidSha256(*kind));
        }
        if artifact.environment != policy.environment {
            blockers.push(AdmissionBlocker::EvidenceEnvironmentMismatch(*kind));
        }
        if !artifact.passed {
            blockers.push(AdmissionBlocker::EvidenceFailed(*kind));
        }
        if artifact.observed_at_ns > now_ns {
            blockers.push(AdmissionBlocker::EvidenceFromFuture(*kind));
        }
        if artifact.expires_at_ns <= now_ns || artifact.expires_at_ns <= artifact.observed_at_ns {
            blockers.push(AdmissionBlocker::EvidenceExpired(*kind));
        } else {
            expires_at_ns = expires_at_ns.min(artifact.expires_at_ns);
        }
    }
    blockers.sort();
    blockers.dedup();
    let report = CapitalAdmissionReport {
        policy_version: policy.version.clone(),
        evaluated_at_ns: now_ns,
        authorization_expires_at_ns: blockers.is_empty().then_some(expires_at_ns),
        blockers,
    };
    if !report.admitted() {
        return Err(CapitalAdmissionDenied { report });
    }
    Ok(CapitalAuthorization {
        authorization_id: format!("{}:{now_ns}:{expires_at_ns}", policy.version),
        policy_version: policy.version.clone(),
        environment: policy.environment.clone(),
        issued_at_ns: now_ns,
        expires_at_ns,
    })
}

fn is_lower_hex_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready() -> ReadinessReport {
        ReadinessReport {
            observed_at_ns: 100,
            blockers: Vec::new(),
        }
    }

    fn complete_ledger(policy: &CapitalAdmissionPolicy) -> EvidenceLedger {
        let mut ledger = EvidenceLedger::default();
        for kind in &policy.required_evidence {
            ledger.record(EvidenceArtifact {
                kind: *kind,
                artifact_id: format!("artifact-{kind:?}"),
                sha256: "a".repeat(64),
                environment: "production".into(),
                observed_at_ns: 90,
                expires_at_ns: 1_000,
                passed: true,
            });
        }
        ledger
    }

    #[test]
    fn missing_evidence_denies_capital() {
        let policy = CapitalAdmissionPolicy::production_default("admission-1", 100);
        let denial = evaluate_capital_admission(&policy, &EvidenceLedger::default(), &ready(), 100)
            .unwrap_err();
        assert_eq!(denial.report.blockers.len(), policy.required_evidence.len());
        assert!(!denial.report.admitted());
    }

    #[test]
    fn authorization_is_bounded_by_earliest_evidence_expiry() {
        let policy = CapitalAdmissionPolicy::production_default("admission-1", 10_000);
        let authorization =
            evaluate_capital_admission(&policy, &complete_ledger(&policy), &ready(), 100).unwrap();
        assert_eq!(authorization.expires_at_ns(), 1_000);
        assert!(authorization.permits("production", 999));
        assert!(!authorization.permits("production", 1_000));
        assert!(!authorization.permits("staging", 999));
    }

    #[test]
    fn failed_evidence_and_operational_blockers_both_survive() {
        let policy = CapitalAdmissionPolicy::production_default("admission-1", 100);
        let mut ledger = complete_ledger(&policy);
        let kind = EvidenceKind::ShadowRun;
        let mut failed = ledger.get(kind).unwrap().clone();
        failed.passed = false;
        ledger.record(failed);
        let readiness = ReadinessReport {
            observed_at_ns: 100,
            blockers: vec![ReadinessBlocker::KillSwitchActive],
        };
        let denial = evaluate_capital_admission(&policy, &ledger, &readiness, 100).unwrap_err();
        assert!(denial
            .report
            .blockers
            .contains(&AdmissionBlocker::EvidenceFailed(kind)));
        assert!(denial
            .report
            .blockers
            .contains(&AdmissionBlocker::Operations(
                ReadinessBlocker::KillSwitchActive
            )));
    }

    #[test]
    fn caller_cannot_weaken_the_production_evidence_set() {
        let mut policy = CapitalAdmissionPolicy::production_default("admission-1", 100);
        policy.required_evidence.clear();
        let denial = evaluate_capital_admission(&policy, &EvidenceLedger::default(), &ready(), 100)
            .unwrap_err();
        assert!(denial
            .report
            .blockers
            .iter()
            .any(|blocker| matches!(blocker, AdmissionBlocker::MissingPolicyRequirement(_))));
    }

    #[test]
    fn stale_operational_snapshot_cannot_authorize_capital() {
        let policy = CapitalAdmissionPolicy::production_default("admission-1", 100);
        let readiness = ReadinessReport {
            observed_at_ns: 0,
            blockers: Vec::new(),
        };
        let denial =
            evaluate_capital_admission(&policy, &complete_ledger(&policy), &readiness, 101)
                .unwrap_err();
        assert!(denial
            .report
            .blockers
            .contains(&AdmissionBlocker::OperationalSnapshotStale));
    }
}
