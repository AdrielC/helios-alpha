use std::time::{SystemTime, UNIX_EPOCH};

use golem_rust::{agent_definition, agent_implementation};
use helio_crypto::{ControlBootstrap, ControlState, PaperConfig, PaperStep, RiskPolicyUpdate};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[agent_definition(snapshotting = "periodic(30s)")]
pub trait CryptoControlAgent {
    fn new(account_id: String) -> Self;
    fn initialize(&mut self, config_json: String) -> Result<String, String>;
    /// The trusted local MCP gateway binds client identity. This method is not a public API.
    fn command(
        &mut self,
        client: String,
        action: String,
        payload_json: String,
    ) -> Result<String, String>;
}

struct CryptoControlAgentImpl {
    account_id: String,
    control: Option<ControlState>,
}

#[derive(Serialize, Deserialize)]
struct Snapshot {
    format_version: u32,
    account_id: String,
    control: Option<ControlState>,
}

fn now_ns() -> Result<u64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "system clock invalid".to_owned())?
        .as_nanos()
        .try_into()
        .map_err(|_| "system clock overflow".into())
}

#[agent_implementation]
impl CryptoControlAgent for CryptoControlAgentImpl {
    fn new(account_id: String) -> Self {
        Self {
            account_id,
            control: None,
        }
    }

    fn initialize(&mut self, config_json: String) -> Result<String, String> {
        if config_json.len() > 64 * 1024 || self.account_id.is_empty() || self.account_id.len() > 80
        {
            return Err("invalid control bootstrap".into());
        }
        let config: ControlBootstrap = serde_json::from_str(&config_json)
            .map_err(|_| "invalid control bootstrap".to_owned())?;
        if let Some(control) = &self.control {
            if !control.bootstrap_matches(&config) {
                return Err("control already initialized with different configuration".into());
            }
        } else {
            self.control = Some(ControlState::new(config, now_ns()?).map_err(|e| e.to_string())?);
        }
        Ok("{\"initialized\":true}".into())
    }

    fn command(
        &mut self,
        client: String,
        action: String,
        payload_json: String,
    ) -> Result<String, String> {
        if client.len() > 80 || action.len() > 40 || payload_json.len() > 64 * 1024 {
            return Err("control request exceeds bound".into());
        }
        let state = self.control.as_mut().ok_or("control is not initialized")?;
        let payload: Value =
            serde_json::from_str(&payload_json).map_err(|_| "invalid control payload")?;
        let result = match action.as_str() {
            "capabilities" => {
                serde_json::to_value(state.capabilities(&client).map_err(|e| e.to_string())?)
            }
            "policy" => serde_json::to_value(state.policy(&client).map_err(|e| e.to_string())?),
            "settings" => Ok(state.settings(&client).map_err(|e| e.to_string())?),
            "status" => Ok(state.status(&client).map_err(|e| e.to_string())?),
            "pause" => Ok(state.pause(&client, now_ns()?).map_err(|e| e.to_string())?),
            "resume_paper" => Ok(state
                .resume_paper(&client, now_ns()?)
                .map_err(|e| e.to_string())?),
            "initialize_paper" => {
                let config: PaperConfig =
                    serde_json::from_value(payload).map_err(|_| "invalid paper configuration")?;
                Ok(state
                    .initialize_paper(&client, config, now_ns()?)
                    .map_err(|e| e.to_string())?)
            }
            "enable_paper_signal_deadlines" => Ok(state
                .enable_paper_signal_deadlines(&client, now_ns()?)
                .map_err(|e| e.to_string())?),
            "paper_step" => {
                let input: PaperStep =
                    serde_json::from_value(payload).map_err(|_| "invalid paper input")?;
                Ok(state
                    .paper_step(&client, input, now_ns()?)
                    .map_err(|e| e.to_string())?)
            }
            "update_policy" => {
                let update: RiskPolicyUpdate =
                    serde_json::from_value(payload).map_err(|_| "invalid policy update")?;
                serde_json::to_value(
                    state
                        .update(&client, update, now_ns()?)
                        .map_err(|e| e.to_string())?,
                )
            }
            "update_paper_costs" => {
                let expected: PaperConfig = serde_json::from_value(payload["expected"].clone())
                    .map_err(|_| "invalid expected paper costs")?;
                let next: PaperConfig = serde_json::from_value(payload["next"].clone())
                    .map_err(|_| "invalid new paper costs")?;
                Ok(state
                    .update_paper_costs(&client, expected, next, now_ns()?)
                    .map_err(|e| e.to_string())?)
            }
            "resize_untraded_paper" => {
                let expected = payload["expected_micros"]
                    .as_u64()
                    .ok_or("invalid old allocation")?;
                let next = payload["next_micros"]
                    .as_u64()
                    .ok_or("invalid new allocation")?;
                Ok(state
                    .resize_untraded_paper(&client, expected, next, now_ns()?)
                    .map_err(|e| e.to_string())?)
            }
            "audit" => {
                let after = payload["after"].as_u64().ok_or("invalid audit cursor")?;
                let limit = payload["limit"].as_u64().ok_or("invalid audit limit")?;
                let limit: usize = limit.try_into().map_err(|_| "invalid audit limit")?;
                serde_json::to_value(
                    state
                        .audit(&client, after, limit)
                        .map_err(|e| e.to_string())?,
                )
            }
            "heartbeat" => {
                let component = payload["component"].as_str().ok_or("component missing")?;
                state
                    .heartbeat(&client, component, payload["status"].clone(), now_ns()?)
                    .map_err(|e| e.to_string())?;
                Ok(json!({"recorded":true}))
            }
            "reserve_decision" => {
                let id = payload["request_id"].as_str().ok_or("request id missing")?;
                let allowed = state
                    .reserve_decision(&client, id, now_ns()?)
                    .map_err(|e| e.to_string())?;
                Ok(json!({"send_allowed":allowed}))
            }
            _ => return Err("unknown control action".into()),
        }
        .map_err(|_| "control serialization failed")?;
        serde_json::to_string(&result).map_err(|_| "control serialization failed".into())
    }

    async fn save_snapshot(&self) -> Result<Vec<u8>, String> {
        serde_json::to_vec(&Snapshot {
            format_version: 1,
            account_id: self.account_id.clone(),
            control: self.control.clone(),
        })
        .map_err(|_| "control snapshot failed".into())
    }

    async fn load_snapshot(&mut self, bytes: Vec<u8>) -> Result<(), String> {
        if bytes.len() > 64 * 1024 * 1024 {
            return Err("control snapshot exceeds bound".into());
        }
        let snapshot: Snapshot =
            serde_json::from_slice(&bytes).map_err(|_| "invalid control snapshot")?;
        if snapshot.format_version != 1 || snapshot.account_id != self.account_id {
            return Err("control snapshot identity/version mismatch".into());
        }
        if let Some(state) = &snapshot.control {
            state
                .validate_snapshot()
                .map_err(|_| "control snapshot invariant failed")?;
        }
        self.control = snapshot.control;
        Ok(())
    }
}
