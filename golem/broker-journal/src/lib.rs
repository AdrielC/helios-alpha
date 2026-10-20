//! Durable bookkeeping, with no broker credentials, HTTP requests, or execution authority.
//! The authenticated local gateway binds `client`; these RPCs are not a public client API.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{SystemTime, UNIX_EPOCH};

use golem_rust::{agent_definition, agent_implementation};
use helio_crypto::{
    CryptoAccountCoverage, CryptoAccountingSeed, CryptoAllocationValuationInput,
    CryptoBrokerJournal, CryptoOrderIntent, CryptoOrderObservation, CryptoPreparedOrderScope,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const MAX_REQUEST_BYTES: usize = 64 * 1024;
const MAX_SNAPSHOT_BYTES: usize = 9 * 1024 * 1024;

#[agent_definition(snapshotting = "every(10)")]
pub trait CryptoBrokerJournalAgent {
    fn new(account_key: String) -> Self;
    fn initialize(&mut self, bootstrap_json: String) -> Result<String, String>;
    fn command(
        &mut self,
        client: String,
        action: String,
        payload_json: String,
    ) -> Result<String, String>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
enum Capability {
    #[serde(rename = "journal:read")]
    Read,
    #[serde(rename = "journal:prepare")]
    Prepare,
    #[serde(rename = "journal:dispatch")]
    Dispatch,
    #[serde(rename = "journal:cancel")]
    Cancel,
    #[serde(rename = "journal:reconcile")]
    Reconcile,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Bootstrap {
    schema_version: u32,
    clients: BTreeMap<String, BTreeSet<Capability>>,
}

impl Bootstrap {
    fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1
            || self.clients.is_empty()
            || self.clients.len() > 32
            || self.clients.iter().any(|(name, capabilities)| {
                name.is_empty()
                    || name.len() > 80
                    || !name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
                    || capabilities.is_empty()
            })
        {
            return Err("invalid journal bootstrap".into());
        }
        Ok(())
    }

    fn capabilities(&self, client: &str) -> Result<&BTreeSet<Capability>, String> {
        self.clients
            .get(client)
            .ok_or_else(|| "journal client is not registered".into())
    }

    fn require(&self, client: &str, capability: Capability) -> Result<(), String> {
        if !self.capabilities(client)?.contains(&capability) {
            return Err("journal capability denied".into());
        }
        Ok(())
    }
}

struct CryptoBrokerJournalAgentImpl {
    account_key: String,
    bootstrap: Option<Bootstrap>,
    journal: Option<CryptoBrokerJournal>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    schema_version: u32,
    account_key: String,
    bootstrap: Option<Bootstrap>,
    journal: Option<Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OrderId {
    client_order_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Symbol {
    symbol: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CoverageInput {
    client_order_id: String,
    coverage: CryptoAccountCoverage,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReconcileTerminalInput {
    client_order_id: String,
    observation_digest: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PreparedValuationInput {
    scope: CryptoPreparedOrderScope,
    valuation: CryptoAllocationValuationInput,
}

fn decode<T: DeserializeOwned>(value: Value) -> Result<T, String> {
    serde_json::from_value(value).map_err(|_| "invalid journal payload".into())
}

fn encode<T: Serialize>(value: T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|_| "journal serialization failed".into())
}

fn response(value: Value) -> Result<String, String> {
    serde_json::to_string(&json!({
        "schema_version": 1,
        "execution_authority": false,
        "result": value,
    }))
    .map_err(|_| "journal serialization failed".into())
}

fn now_ms() -> Result<u64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "journal system clock invalid")?
        .as_millis()
        .try_into()
        .map_err(|_| "journal system clock overflow".into())
}

#[agent_implementation]
impl CryptoBrokerJournalAgent for CryptoBrokerJournalAgentImpl {
    fn new(account_key: String) -> Self {
        Self {
            account_key,
            bootstrap: None,
            journal: None,
        }
    }

    fn initialize(&mut self, bootstrap_json: String) -> Result<String, String> {
        if bootstrap_json.len() > MAX_REQUEST_BYTES {
            return Err("journal bootstrap exceeds bound".into());
        }
        let bootstrap: Bootstrap = serde_json::from_str(&bootstrap_json)
            .map_err(|_| "invalid journal bootstrap".to_owned())?;
        bootstrap.validate()?;
        if let Some(existing) = &self.bootstrap {
            if existing != &bootstrap {
                return Err("journal is already initialized with different permissions".into());
            }
        } else {
            let journal =
                CryptoBrokerJournal::new(self.account_key.clone()).map_err(|e| e.to_string())?;
            self.journal = Some(journal);
            self.bootstrap = Some(bootstrap);
        }
        response(json!({"initialized": true}))
    }

    fn command(
        &mut self,
        client: String,
        action: String,
        payload_json: String,
    ) -> Result<String, String> {
        if client.len() > 80 || action.len() > 40 || payload_json.len() > MAX_REQUEST_BYTES {
            return Err("journal request exceeds bound".into());
        }
        let bootstrap = self
            .bootstrap
            .as_ref()
            .ok_or("journal is not initialized")?;
        // An unknown client cannot probe records, parser errors, or journal state.
        bootstrap.capabilities(&client)?;
        let permission = match action.as_str() {
            "capabilities"
            | "status"
            | "record"
            | "account_state"
            | "risk_state"
            | "risk_account_snapshot"
            | "prepared_allocation"
            | "prepared_risk_account_snapshot" => Capability::Read,
            "prepare" => Capability::Prepare,
            "mark_dispatch_started" => Capability::Dispatch,
            "request_cancel" | "plan_recovery" => Capability::Cancel,
            "observe"
            | "cover_account"
            | "initialize_accounting"
            | "reconcile_terminal"
            | "mark_allocation"
            | "mark_prepared_allocation" => Capability::Reconcile,
            _ => return Err("unknown journal action".into()),
        };
        if action != "capabilities" {
            bootstrap.require(&client, permission)?;
        }
        let payload: Value =
            serde_json::from_str(&payload_json).map_err(|_| "invalid journal payload")?;
        let journal = self.journal.as_mut().ok_or("journal is not initialized")?;
        let result = match action.as_str() {
            "capabilities" => {
                let _: Empty = decode(payload)?;
                encode(bootstrap.capabilities(&client)?)?
            }
            "status" => {
                let _: Empty = decode(payload)?;
                let bytes = journal.save_snapshot().map_err(|e| e.to_string())?;
                let unresolved: Vec<_> = journal
                    .records()
                    .filter(|record| record.reservation_held())
                    .map(|record| {
                        json!({
                            "client_order_id": record.intent.client_order_id,
                            "phase": record.phase,
                            "revision": record.revision,
                        })
                    })
                    .collect();
                json!({
                    "account_key": self.account_key,
                    "record_count": journal.records().count(),
                    "reservations": journal.reservations().map_err(|e| e.to_string())?,
                    "unresolved": unresolved,
                    "snapshot_digest": hex::encode(Sha256::digest(bytes)),
                })
            }
            "record" => {
                let input: OrderId = decode(payload)?;
                encode(
                    journal
                        .record(&input.client_order_id)
                        .map_err(|e| e.to_string())?,
                )?
            }
            "account_state" => {
                let _: Empty = decode(payload)?;
                let balance = journal.allocation_balance().map_err(|e| e.to_string())?;
                json!({"initialized": balance.is_some(), "balance": balance})
            }
            "risk_state" => {
                let _: Empty = decode(payload)?;
                encode(
                    journal
                        .allocation_risk_state(now_ms()?)
                        .map_err(|e| e.to_string())?,
                )?
            }
            "risk_account_snapshot" => {
                let input: Symbol = decode(payload)?;
                encode(
                    journal
                        .valued_account(&input.symbol, now_ms()?)
                        .map_err(|e| e.to_string())?,
                )?
            }
            "mark_allocation" => {
                let input: CryptoAllocationValuationInput = decode(payload)?;
                encode(
                    journal
                        .mark_allocation(input, now_ms()?)
                        .map_err(|e| e.to_string())?,
                )?
            }
            "prepared_allocation" => {
                let input: OrderId = decode(payload)?;
                encode(
                    journal
                        .prepared_allocation(&input.client_order_id, now_ms()?)
                        .map_err(|e| e.to_string())?,
                )?
            }
            "prepared_risk_account_snapshot" => {
                let input: CryptoPreparedOrderScope = decode(payload)?;
                encode(
                    journal
                        .prepared_valued_account(&input, now_ms()?)
                        .map_err(|e| e.to_string())?,
                )?
            }
            "mark_prepared_allocation" => {
                let input: PreparedValuationInput = decode(payload)?;
                encode(
                    journal
                        .mark_prepared_allocation(input.scope, input.valuation, now_ms()?)
                        .map_err(|e| e.to_string())?,
                )?
            }
            "initialize_accounting" => {
                let input: CryptoAccountingSeed = decode(payload)?;
                encode(
                    journal
                        .initialize_accounting(input, now_ms()?)
                        .map_err(|e| e.to_string())?,
                )?
            }
            "reconcile_terminal" => {
                let input: ReconcileTerminalInput = decode(payload)?;
                encode(
                    journal
                        .reconcile_terminal(
                            &input.client_order_id,
                            &input.observation_digest,
                            now_ms()?,
                        )
                        .map_err(|e| e.to_string())?,
                )?
            }
            "prepare" => {
                let input: CryptoOrderIntent = decode(payload)?;
                encode(
                    journal
                        .prepare(input, now_ms()?)
                        .map_err(|e| e.to_string())?,
                )?
            }
            "mark_dispatch_started" => {
                let input: OrderId = decode(payload)?;
                encode(
                    journal
                        .mark_dispatch_started(&input.client_order_id, now_ms()?)
                        .map_err(|e| e.to_string())?,
                )?
            }
            "request_cancel" => {
                let input: OrderId = decode(payload)?;
                encode(
                    journal
                        .request_cancel(&input.client_order_id, now_ms()?)
                        .map_err(|e| e.to_string())?,
                )?
            }
            "plan_recovery" => {
                let _: Empty = decode(payload)?;
                encode(
                    journal
                        .plan_recovery(now_ms()?)
                        .map_err(|e| e.to_string())?,
                )?
            }
            "observe" => {
                let input: CryptoOrderObservation = decode(payload)?;
                encode(
                    journal
                        .observe(input, now_ms()?)
                        .map_err(|e| e.to_string())?,
                )?
            }
            "cover_account" => {
                let input: CoverageInput = decode(payload)?;
                encode(
                    journal
                        .cover_account(&input.client_order_id, input.coverage, now_ms()?)
                        .map_err(|e| e.to_string())?,
                )?
            }
            _ => return Err("unknown journal action".into()),
        };
        response(result)
    }

    async fn save_snapshot(&self) -> Result<Vec<u8>, String> {
        let journal = self
            .journal
            .as_ref()
            .map(|state| {
                let bytes = state.save_snapshot().map_err(|e| e.to_string())?;
                serde_json::from_slice(&bytes).map_err(|_| "invalid journal snapshot".to_owned())
            })
            .transpose()?;
        let bytes = serde_json::to_vec(&Snapshot {
            schema_version: 1,
            account_key: self.account_key.clone(),
            bootstrap: self.bootstrap.clone(),
            journal,
        })
        .map_err(|_| "journal snapshot serialization failed")?;
        if bytes.len() > MAX_SNAPSHOT_BYTES {
            return Err("journal snapshot exceeds bound".into());
        }
        Ok(bytes)
    }

    async fn load_snapshot(&mut self, bytes: Vec<u8>) -> Result<(), String> {
        if bytes.len() > MAX_SNAPSHOT_BYTES {
            return Err("journal snapshot exceeds bound".into());
        }
        let snapshot: Snapshot =
            serde_json::from_slice(&bytes).map_err(|_| "invalid journal snapshot")?;
        if snapshot.schema_version != 1
            || snapshot.account_key != self.account_key
            || snapshot.bootstrap.is_some() != snapshot.journal.is_some()
        {
            return Err("journal snapshot identity/version mismatch".into());
        }
        if let Some(bootstrap) = &snapshot.bootstrap {
            bootstrap.validate()?;
        }
        let journal = snapshot
            .journal
            .map(|value| {
                let bytes = serde_json::to_vec(&value).map_err(|_| "invalid journal snapshot")?;
                CryptoBrokerJournal::restore_snapshot(&self.account_key, &bytes)
                    .map_err(|e| e.to_string())
            })
            .transpose()?;
        // Validate all state before mutating the current agent.
        self.bootstrap = snapshot.bootstrap;
        self.journal = journal;
        Ok(())
    }
}
