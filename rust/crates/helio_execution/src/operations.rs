use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadinessPolicy {
    pub max_source_lag_ns: u64,
    pub max_checkpoint_age_ns: u64,
    pub max_outbox_age_ns: u64,
    pub max_clock_offset_ns: u64,
    pub require_calendar_coverage_until_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationalSnapshot {
    pub observed_at_ns: u64,
    pub source_lag_ns: u64,
    pub checkpoint_age_ns: u64,
    pub oldest_pending_outbox_age_ns: Option<u64>,
    pub pending_broker_reconciliations: u32,
    pub clock_offset_ns: u64,
    pub calendar_covered_until_ns: u64,
    pub kill_switch_active: bool,
    pub open_incident_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ReadinessBlocker {
    SourceLag,
    CheckpointStale,
    OutboxStale,
    BrokerReconciliationPending,
    ClockOffset,
    CalendarCoverage,
    KillSwitchActive,
    IncidentOpen,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadinessReport {
    pub observed_at_ns: u64,
    pub blockers: Vec<ReadinessBlocker>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperationalMetricName {
    SourceLagNs,
    CheckpointAgeNs,
    OldestPendingOutboxAgeNs,
    PendingBrokerReconciliations,
    ClockOffsetNs,
    CalendarCoveredUntilNs,
    KillSwitchActive,
    OpenIncidentCount,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationalMetric {
    pub name: OperationalMetricName,
    pub value: u64,
    pub observed_at_ns: u64,
}

pub trait ObservabilitySink {
    type Error;

    fn record(&mut self, metric: OperationalMetric) -> Result<(), Self::Error>;
}

/// Emit one complete, stable metric set suitable for an OpenTelemetry or Prometheus adapter.
pub fn emit_operational_metrics<Sink>(
    snapshot: &OperationalSnapshot,
    sink: &mut Sink,
) -> Result<usize, Sink::Error>
where
    Sink: ObservabilitySink,
{
    let values = [
        (OperationalMetricName::SourceLagNs, snapshot.source_lag_ns),
        (
            OperationalMetricName::CheckpointAgeNs,
            snapshot.checkpoint_age_ns,
        ),
        (
            OperationalMetricName::OldestPendingOutboxAgeNs,
            snapshot.oldest_pending_outbox_age_ns.unwrap_or(0),
        ),
        (
            OperationalMetricName::PendingBrokerReconciliations,
            u64::from(snapshot.pending_broker_reconciliations),
        ),
        (
            OperationalMetricName::ClockOffsetNs,
            snapshot.clock_offset_ns,
        ),
        (
            OperationalMetricName::CalendarCoveredUntilNs,
            snapshot.calendar_covered_until_ns,
        ),
        (
            OperationalMetricName::KillSwitchActive,
            u64::from(snapshot.kill_switch_active),
        ),
        (
            OperationalMetricName::OpenIncidentCount,
            u64::from(snapshot.open_incident_count),
        ),
    ];
    for (name, value) in values {
        sink.record(OperationalMetric {
            name,
            value,
            observed_at_ns: snapshot.observed_at_ns,
        })?;
    }
    Ok(values.len())
}

impl ReadinessReport {
    pub fn is_ready(&self) -> bool {
        self.blockers.is_empty()
    }
}

impl OperationalSnapshot {
    pub fn evaluate(&self, policy: &ReadinessPolicy) -> ReadinessReport {
        let mut blockers = Vec::new();
        if self.source_lag_ns > policy.max_source_lag_ns {
            blockers.push(ReadinessBlocker::SourceLag);
        }
        if self.checkpoint_age_ns > policy.max_checkpoint_age_ns {
            blockers.push(ReadinessBlocker::CheckpointStale);
        }
        if self
            .oldest_pending_outbox_age_ns
            .is_some_and(|age| age > policy.max_outbox_age_ns)
        {
            blockers.push(ReadinessBlocker::OutboxStale);
        }
        if self.pending_broker_reconciliations > 0 {
            blockers.push(ReadinessBlocker::BrokerReconciliationPending);
        }
        if self.clock_offset_ns > policy.max_clock_offset_ns {
            blockers.push(ReadinessBlocker::ClockOffset);
        }
        if self.calendar_covered_until_ns < policy.require_calendar_coverage_until_ns {
            blockers.push(ReadinessBlocker::CalendarCoverage);
        }
        if self.kill_switch_active {
            blockers.push(ReadinessBlocker::KillSwitchActive);
        }
        if self.open_incident_count > 0 {
            blockers.push(ReadinessBlocker::IncidentOpen);
        }
        ReadinessReport {
            observed_at_ns: self.observed_at_ns,
            blockers,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IncidentSeverity {
    Sev1,
    Sev2,
    Sev3,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IncidentStatus {
    Open,
    Acknowledged {
        at_ns: u64,
        by: String,
    },
    Resolved {
        at_ns: u64,
        by: String,
        resolution: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Incident {
    pub id: u64,
    pub opened_at_ns: u64,
    pub severity: IncidentSeverity,
    pub summary: String,
    pub status: IncidentStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum IncidentError {
    #[error("incident summary and actor must not be empty")]
    EmptyField,
    #[error("unknown incident {0}")]
    UnknownIncident(u64),
    #[error("incident {0} is already resolved")]
    AlreadyResolved(u64),
    #[error("incident identity overflowed")]
    IdentityOverflow,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IncidentJournal {
    next_id: u64,
    incidents: BTreeMap<u64, Incident>,
}

impl IncidentJournal {
    pub fn open(
        &mut self,
        opened_at_ns: u64,
        severity: IncidentSeverity,
        summary: impl Into<String>,
    ) -> Result<u64, IncidentError> {
        let summary = summary.into();
        if summary.trim().is_empty() {
            return Err(IncidentError::EmptyField);
        }
        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or(IncidentError::IdentityOverflow)?;
        self.incidents.insert(
            id,
            Incident {
                id,
                opened_at_ns,
                severity,
                summary,
                status: IncidentStatus::Open,
            },
        );
        Ok(id)
    }

    pub fn acknowledge(
        &mut self,
        id: u64,
        at_ns: u64,
        by: impl Into<String>,
    ) -> Result<(), IncidentError> {
        let by = by.into();
        if by.trim().is_empty() {
            return Err(IncidentError::EmptyField);
        }
        let incident = self
            .incidents
            .get_mut(&id)
            .ok_or(IncidentError::UnknownIncident(id))?;
        if matches!(incident.status, IncidentStatus::Resolved { .. }) {
            return Err(IncidentError::AlreadyResolved(id));
        }
        incident.status = IncidentStatus::Acknowledged { at_ns, by };
        Ok(())
    }

    pub fn resolve(
        &mut self,
        id: u64,
        at_ns: u64,
        by: impl Into<String>,
        resolution: impl Into<String>,
    ) -> Result<(), IncidentError> {
        let by = by.into();
        let resolution = resolution.into();
        if by.trim().is_empty() || resolution.trim().is_empty() {
            return Err(IncidentError::EmptyField);
        }
        let incident = self
            .incidents
            .get_mut(&id)
            .ok_or(IncidentError::UnknownIncident(id))?;
        if matches!(incident.status, IncidentStatus::Resolved { .. }) {
            return Err(IncidentError::AlreadyResolved(id));
        }
        incident.status = IncidentStatus::Resolved {
            at_ns,
            by,
            resolution,
        };
        Ok(())
    }

    pub fn open_count(&self) -> usize {
        self.incidents
            .values()
            .filter(|incident| !matches!(incident.status, IncidentStatus::Resolved { .. }))
            .count()
    }

    pub fn incidents(&self) -> impl Iterator<Item = &Incident> {
        self.incidents.values()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct MetricCollector(Vec<OperationalMetric>);

    impl ObservabilitySink for MetricCollector {
        type Error = std::convert::Infallible;

        fn record(&mut self, metric: OperationalMetric) -> Result<(), Self::Error> {
            self.0.push(metric);
            Ok(())
        }
    }

    #[test]
    fn readiness_reports_every_independent_blocker() {
        let snapshot = OperationalSnapshot {
            observed_at_ns: 100,
            source_lag_ns: 11,
            checkpoint_age_ns: 21,
            oldest_pending_outbox_age_ns: Some(31),
            pending_broker_reconciliations: 1,
            clock_offset_ns: 41,
            calendar_covered_until_ns: 99,
            kill_switch_active: true,
            open_incident_count: 1,
        };
        let report = snapshot.evaluate(&ReadinessPolicy {
            max_source_lag_ns: 10,
            max_checkpoint_age_ns: 20,
            max_outbox_age_ns: 30,
            max_clock_offset_ns: 40,
            require_calendar_coverage_until_ns: 100,
        });
        assert_eq!(report.blockers.len(), 8);
        assert!(!report.is_ready());

        let mut metrics = MetricCollector::default();
        assert_eq!(emit_operational_metrics(&snapshot, &mut metrics), Ok(8));
        assert_eq!(metrics.0.len(), 8);
        assert!(metrics.0.iter().any(|metric| {
            metric.name == OperationalMetricName::PendingBrokerReconciliations && metric.value == 1
        }));
    }

    #[test]
    fn incidents_require_explicit_resolution() {
        let mut journal = IncidentJournal::default();
        let id = journal
            .open(
                10,
                IncidentSeverity::Sev1,
                "broker acknowledgements stalled",
            )
            .unwrap();
        journal.acknowledge(id, 11, "on-call").unwrap();
        assert_eq!(journal.open_count(), 1);
        journal
            .resolve(id, 12, "on-call", "gateway isolated and reconciled")
            .unwrap();
        assert_eq!(journal.open_count(), 0);
        assert_eq!(
            journal.resolve(id, 13, "on-call", "again"),
            Err(IncidentError::AlreadyResolved(id))
        );
    }
}
