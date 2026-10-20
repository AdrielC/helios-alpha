//! Pure control state, persisted by one Golem agent per trading account.
//! No secrets, strategy source, network, or authority to bypass capital admission.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

use crate::{CryptoRiskLimits, PaperConfig, PaperState, PaperStep};

const MAX_REVISIONS: usize = 10_000;
const MAX_AUDIT: usize = 50_000;
const CAPS: [&str; 8] = [
    "status:read",
    "risk:read",
    "audit:read",
    "execution:pause",
    "risk:write",
    "runtime:write",
    "decisions:reserve",
    "execution:resume-paper",
];

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ControlStateError {
    #[error("control request is invalid")]
    Invalid,
    #[error("client lacks the required capability")]
    Forbidden,
    #[error("risk policy revision changed; read it again")]
    RevisionConflict,
    #[error("request id already belongs to different contents")]
    IdentityConflict,
    #[error("control state retention bound reached")]
    Capacity,
    #[error("daily decision request budget exhausted")]
    BudgetExhausted,
    #[error("control clock regressed")]
    ClockRegression,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlBootstrap {
    pub limits: CryptoRiskLimits,
    pub clients: BTreeMap<String, BTreeSet<String>>,
    pub decision_daily_calls: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RiskPolicyUpdate {
    pub expected_revision: u64,
    pub request_id: String,
    pub limits: CryptoRiskLimits,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyRevision {
    pub schema_version: u32,
    pub revision: u64,
    pub limits: CryptoRiskLimits,
    pub sha256: String,
    pub updated_ns: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RequestRecord {
    digest: String,
    revision_index: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlAudit {
    pub sequence: u64,
    pub created_ns: u64,
    pub actor: String,
    pub action: String,
    pub details: Value,
    pub previous_hash: String,
    pub hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlState {
    format_version: u32,
    bootstrap_digest: String,
    clients: BTreeMap<String, BTreeSet<String>>,
    revisions: Vec<PolicyRevision>,
    requests: BTreeMap<String, RequestRecord>,
    paused: bool,
    audit: Vec<ControlAudit>,
    runtime: BTreeMap<String, Value>,
    decision_daily_calls: u32,
    decision_day: u64,
    decision_reservations: BTreeSet<String>,
    #[serde(default)]
    paper: Option<PaperState>,
}

fn digest(value: &impl Serialize) -> String {
    hex::encode(Sha256::digest(
        serde_json::to_vec(value).expect("serializable control state"),
    ))
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 80
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

impl ControlState {
    pub fn new(config: ControlBootstrap, now_ns: u64) -> Result<Self, ControlStateError> {
        config
            .limits
            .validate()
            .map_err(|_| ControlStateError::Invalid)?;
        if config.clients.is_empty()
            || config.clients.len() > 64
            || config.clients.iter().any(|(id, caps)| {
                !valid_id(id) || caps.iter().any(|cap| !CAPS.contains(&cap.as_str()))
            })
            || config.decision_daily_calls > 100
        {
            return Err(ControlStateError::Invalid);
        }
        let bootstrap_digest = digest(&config);
        let revision = PolicyRevision {
            schema_version: 1,
            revision: 1,
            sha256: digest(&config.limits),
            limits: config.limits,
            updated_ns: now_ns,
        };
        let mut state = Self {
            format_version: 1,
            bootstrap_digest,
            clients: config.clients,
            revisions: vec![revision],
            requests: BTreeMap::new(),
            paused: false,
            audit: vec![],
            runtime: BTreeMap::new(),
            decision_daily_calls: config.decision_daily_calls,
            decision_day: now_ns / 86_400_000_000_000,
            decision_reservations: BTreeSet::new(),
            paper: None,
        };
        state.append_audit(
            "local-owner",
            "initialize",
            json!({"revision":1,"mode":"shadow"}),
            now_ns,
        );
        Ok(state)
    }

    pub fn bootstrap_matches(&self, config: &ControlBootstrap) -> bool {
        self.bootstrap_digest == digest(config)
    }

    fn require(&self, actor: &str, cap: &str) -> Result<(), ControlStateError> {
        if !self
            .clients
            .get(actor)
            .is_some_and(|caps| caps.contains(cap))
        {
            return Err(ControlStateError::Forbidden);
        }
        Ok(())
    }

    fn writable_at(&self, now_ns: u64) -> Result<(), ControlStateError> {
        if self.audit.len() >= MAX_AUDIT {
            return Err(ControlStateError::Capacity);
        }
        if self.audit.last().is_some_and(|a| a.created_ns > now_ns) {
            return Err(ControlStateError::ClockRegression);
        }
        Ok(())
    }

    fn append_audit(&mut self, actor: &str, action: &str, details: Value, now_ns: u64) {
        let previous_hash = self
            .audit
            .last()
            .map(|a| a.hash.clone())
            .unwrap_or("0".repeat(64));
        let hash = digest(&(now_ns, actor, action, &details, &previous_hash));
        self.audit.push(ControlAudit {
            sequence: self.audit.len() as u64 + 1,
            created_ns: now_ns,
            actor: actor.into(),
            action: action.into(),
            details,
            previous_hash,
            hash,
        });
    }

    pub fn capabilities(&self, actor: &str) -> Result<Vec<String>, ControlStateError> {
        self.clients
            .get(actor)
            .map(|caps| caps.iter().cloned().collect())
            .ok_or(ControlStateError::Forbidden)
    }

    pub fn policy(&self, actor: &str) -> Result<&PolicyRevision, ControlStateError> {
        self.require(actor, "risk:read")?;
        self.revisions.last().ok_or(ControlStateError::Invalid)
    }

    /// One invocation reads one coherent revision, execution state and caller permissions.
    pub fn settings(&self, actor: &str) -> Result<Value, ControlStateError> {
        let policy = self.policy(actor)?;
        let status = self.status(actor)?;
        Ok(json!({
            "schema_version": 1,
            "authority": "golem",
            "currency": "USD",
            "money_scale": 1_000_000,
            "policy": policy,
            "execution": {
                "mode": status["mode"], "paused": self.paused,
                "live_admitted": status["live_admitted"],
                "live_blockers": status["live_blockers"]
            },
            "decisions": status["decision_budget"],
            "paper_costs": status["paper"]["cost_assumptions"],
            "client": {"id": actor, "capabilities": self.capabilities(actor)?},
            "semantics": {
                "order_micros": "buy debit includes reserved fees; sell limit is gross notional",
                "max_daily_orders": "block new buys after this many total orders; held-asset sells remain allowed",
                "daily_loss_micros": "UTC-day equity loss trigger, including unrealized P&L",
                "drawdown_micros": "equity high-water drawdown trigger; survives restarts",
                "loss_triggers": "block buys and target risk reduction; not guaranteed loss ceilings",
                "permissions": "local process identity; not an OS sandbox or remote authentication"
            }
        }))
    }

    pub fn status(&self, actor: &str) -> Result<Value, ControlStateError> {
        self.require(actor, "status:read")?;
        Ok(json!({
            "schema_version": 1, "authority": "golem", "mode": if self.paper.is_some() {"paper"} else {"shadow"}, "paused": self.paused,
            "policy_revision": self.revisions.last().ok_or(ControlStateError::Invalid)?.revision,
            "live_admitted": false,
            "live_blockers": [
                "dedicated_broker_credentials_not_validated",
                "account_execution_recovery_not_certified",
                "strategy_evidence_not_admitted"
            ],
            "decision_budget": {
                "daily_call_limit": self.decision_daily_calls, "day_utc": self.decision_day,
                "reserved_calls": self.decision_reservations.len()
            },
            "runtime": self.runtime,
            "paper": self.paper.as_ref().map(|p| p.status())
        }))
    }

    pub fn update(
        &mut self,
        actor: &str,
        update: RiskPolicyUpdate,
        now_ns: u64,
    ) -> Result<PolicyRevision, ControlStateError> {
        self.require(actor, "risk:write")?;
        update
            .limits
            .validate()
            .map_err(|_| ControlStateError::Invalid)?;
        if !valid_id(&update.request_id) || !(3..=500).contains(&update.reason.len()) {
            return Err(ControlStateError::Invalid);
        }
        let request_key = format!("{}:{}{}", actor.len(), actor, update.request_id);
        let hash = digest(&update);
        if let Some(prior) = self.requests.get(&request_key) {
            if prior.digest != hash {
                return Err(ControlStateError::IdentityConflict);
            }
            return self
                .revisions
                .get(prior.revision_index)
                .cloned()
                .ok_or(ControlStateError::Invalid);
        }
        if self
            .revisions
            .last()
            .ok_or(ControlStateError::Invalid)?
            .revision
            != update.expected_revision
        {
            return Err(ControlStateError::RevisionConflict);
        }
        if self.revisions.len() >= MAX_REVISIONS {
            return Err(ControlStateError::Capacity);
        }
        self.writable_at(now_ns)?;
        let revision = PolicyRevision {
            schema_version: 1,
            revision: self.revisions.len() as u64 + 1,
            sha256: digest(&update.limits),
            limits: update.limits,
            updated_ns: now_ns,
        };
        self.requests.insert(
            request_key,
            RequestRecord {
                digest: hash,
                revision_index: self.revisions.len(),
            },
        );
        self.revisions.push(revision.clone());
        self.append_audit(
            actor,
            "risk.update",
            json!({"revision":revision.revision,"sha256":revision.sha256}),
            now_ns,
        );
        Ok(revision)
    }

    pub fn pause(&mut self, actor: &str, now_ns: u64) -> Result<Value, ControlStateError> {
        self.require(actor, "execution:pause")?;
        if !self.paused {
            self.writable_at(now_ns)?;
            self.paused = true;
            self.append_audit(actor, "execution.pause", json!({}), now_ns);
        }
        Ok(json!({"paused":true,"effect":"block new submissions; does not cancel existing orders"}))
    }

    pub fn audit(
        &self,
        actor: &str,
        after: u64,
        limit: usize,
    ) -> Result<Vec<ControlAudit>, ControlStateError> {
        self.require(actor, "audit:read")?;
        if limit == 0 || limit > 200 {
            return Err(ControlStateError::Invalid);
        }
        Ok(self
            .audit
            .iter()
            .filter(|a| a.sequence > after)
            .take(limit)
            .cloned()
            .collect())
    }

    pub fn resume_paper(&mut self, actor: &str, now_ns: u64) -> Result<Value, ControlStateError> {
        self.require(actor, "execution:resume-paper")?;
        if self.paper.is_none() {
            return Err(ControlStateError::Invalid);
        }
        self.writable_at(now_ns)?;
        if self.paused {
            self.paused = false;
            self.append_audit(actor, "execution.resume-paper", json!({}), now_ns);
        }
        Ok(json!({"paused":false,"mode":"paper","live_admitted":false}))
    }

    pub fn initialize_paper(
        &mut self,
        actor: &str,
        config: PaperConfig,
        now_ns: u64,
    ) -> Result<Value, ControlStateError> {
        self.require(actor, "runtime:write")?;
        if let Some(paper) = &self.paper {
            if !paper.config_matches(&config) {
                return Err(ControlStateError::IdentityConflict);
            }
            return Ok(paper.status());
        }
        self.writable_at(now_ns)?;
        let limits = &self
            .revisions
            .last()
            .ok_or(ControlStateError::Invalid)?
            .limits;
        self.paper = Some(PaperState::new(config, limits, now_ns / 1_000_000)?);
        self.append_audit(actor, "paper.initialize", json!({}), now_ns);
        Ok(self
            .paper
            .as_ref()
            .ok_or(ControlStateError::Invalid)?
            .status())
    }

    /// One-way activation after replaying history from before signal deadlines existed.
    pub fn enable_paper_signal_deadlines(
        &mut self,
        actor: &str,
        now_ns: u64,
    ) -> Result<Value, ControlStateError> {
        self.require(actor, "runtime:write")?;
        let paper = self.paper.as_ref().ok_or(ControlStateError::Invalid)?;
        if paper.signal_deadlines_enabled() {
            return Ok(paper.status());
        }
        self.writable_at(now_ns)?;
        self.paper
            .as_mut()
            .ok_or(ControlStateError::Invalid)?
            .enable_signal_deadlines();
        self.append_audit(actor, "paper.signal-deadline", json!({"version":1}), now_ns);
        Ok(self
            .paper
            .as_ref()
            .ok_or(ControlStateError::Invalid)?
            .status())
    }

    pub fn paper_step(
        &mut self,
        actor: &str,
        input: PaperStep,
        now_ns: u64,
    ) -> Result<Value, ControlStateError> {
        self.require(actor, "runtime:write")?;
        let revision = self.revisions.last().ok_or(ControlStateError::Invalid)?;
        self.paper
            .as_mut()
            .ok_or(ControlStateError::Invalid)?
            .apply(
                &input,
                &revision.limits,
                revision.revision,
                self.paused,
                now_ns / 1_000_000,
            )
    }

    pub fn update_paper_costs(
        &mut self,
        actor: &str,
        expected: PaperConfig,
        next: PaperConfig,
        now_ns: u64,
    ) -> Result<Value, ControlStateError> {
        self.require(actor, "risk:write")?;
        self.writable_at(now_ns)?;
        let limits = &self
            .revisions
            .last()
            .ok_or(ControlStateError::Invalid)?
            .limits;
        if next.minimum_order_micros > limits.order_micros {
            return Err(ControlStateError::Invalid);
        }
        let paper = self.paper.as_mut().ok_or(ControlStateError::Invalid)?;
        if paper.config_matches(&next) {
            return Ok(json!({"updated":true,"simulation":true}));
        }
        paper.update_costs(&expected, next.clone())?;
        self.append_audit(
            actor,
            "paper.costs",
            serde_json::to_value(next).map_err(|_| ControlStateError::Invalid)?,
            now_ns,
        );
        Ok(json!({"updated":true,"simulation":true}))
    }

    /// Seed an untouched simulator to a newly authorized allocation; never reset traded history.
    pub fn resize_untraded_paper(
        &mut self,
        actor: &str,
        expected_micros: u64,
        next_micros: u64,
        now_ns: u64,
    ) -> Result<Value, ControlStateError> {
        self.require(actor, "risk:write")?;
        self.writable_at(now_ns)?;
        let limits = &self
            .revisions
            .last()
            .ok_or(ControlStateError::Invalid)?
            .limits;
        let paper = self.paper.as_mut().ok_or(ControlStateError::Invalid)?;
        let changed = paper.resize_untraded(expected_micros, next_micros, limits)?;
        if changed {
            self.append_audit(
                actor,
                "paper.resize-untraded",
                json!({"previous_micros":expected_micros,"next_micros":next_micros}),
                now_ns,
            );
        }
        Ok(json!({"updated":true,"simulation":true}))
    }

    pub fn heartbeat(
        &mut self,
        actor: &str,
        component: &str,
        status: Value,
        now_ns: u64,
    ) -> Result<(), ControlStateError> {
        self.require(actor, "runtime:write")?;
        if !["crypto-research", "crypto-shadow", "crypto-execution"].contains(&component)
            || serde_json::to_vec(&status)
                .map_err(|_| ControlStateError::Invalid)?
                .len()
                > 4096
        {
            return Err(ControlStateError::Invalid);
        }
        if self
            .runtime
            .get(component)
            .and_then(|v| v["updated_ns"].as_u64())
            .is_some_and(|old| old > now_ns)
        {
            return Err(ControlStateError::ClockRegression);
        }
        self.runtime.insert(
            component.into(),
            json!({"updated_ns":now_ns,"status":status}),
        );
        Ok(())
    }

    /// Charge BEFORE a paid API call. Never refund ambiguous calls. A repeated reservation
    /// returns false: reconcile the existing Decisions journal instead of sending again.
    pub fn reserve_decision(
        &mut self,
        actor: &str,
        request_id: &str,
        now_ns: u64,
    ) -> Result<bool, ControlStateError> {
        self.require(actor, "decisions:reserve")?;
        if !valid_id(request_id) {
            return Err(ControlStateError::Invalid);
        }
        self.writable_at(now_ns)?;
        let day = now_ns / 86_400_000_000_000;
        if day < self.decision_day {
            return Err(ControlStateError::ClockRegression);
        }
        // A new day doesn't authorize replay of the same request: completed/pending IDs
        // still belong to the independent durable Decisions journal.
        if day > self.decision_day {
            self.decision_day = day;
            self.decision_reservations.clear();
        }
        if self.decision_reservations.contains(request_id) {
            return Ok(false);
        }
        if self.decision_reservations.len() >= self.decision_daily_calls as usize {
            return Err(ControlStateError::BudgetExhausted);
        }
        self.decision_reservations.insert(request_id.into());
        self.append_audit(
            actor,
            "decision.reserve",
            json!({"request_id":request_id,"day_utc":day}),
            now_ns,
        );
        Ok(true)
    }

    pub fn validate_snapshot(&self) -> Result<(), ControlStateError> {
        if self.format_version != 1
            || self.revisions.is_empty()
            || self.revisions.len() > MAX_REVISIONS
            || self.audit.len() > MAX_AUDIT
            || self.decision_daily_calls > 100
            || self.decision_reservations.len() > self.decision_daily_calls as usize
            || self.clients.is_empty()
            || self.clients.len() > 64
            || self.clients.iter().any(|(id, caps)| {
                !valid_id(id) || caps.iter().any(|cap| !CAPS.contains(&cap.as_str()))
            })
        {
            return Err(ControlStateError::Invalid);
        }
        for (i, revision) in self.revisions.iter().enumerate() {
            revision
                .limits
                .validate()
                .map_err(|_| ControlStateError::Invalid)?;
            if revision.schema_version != 1
                || revision.revision != i as u64 + 1
                || revision.sha256 != digest(&revision.limits)
            {
                return Err(ControlStateError::Invalid);
            }
        }
        let mut previous = "0".repeat(64);
        let mut previous_time = 0;
        for (i, entry) in self.audit.iter().enumerate() {
            if entry.sequence != i as u64 + 1
                || entry.previous_hash != previous
                || entry.created_ns < previous_time
                || entry.hash
                    != digest(&(
                        entry.created_ns,
                        entry.actor.as_str(),
                        entry.action.as_str(),
                        &entry.details,
                        &entry.previous_hash,
                    ))
            {
                return Err(ControlStateError::Invalid);
            }
            previous = entry.hash.clone();
            previous_time = entry.created_ns;
        }
        if self
            .requests
            .values()
            .any(|r| r.revision_index >= self.revisions.len())
        {
            return Err(ControlStateError::Invalid);
        }
        if let Some(paper) = &self.paper {
            paper.validate_snapshot()?;
        }
        Ok(())
    }
}
