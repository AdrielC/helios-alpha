//! JSON config for [`crate::replay_event_shock_cli`] / demo runs (no CLI parser dependency).

use serde::{Deserialize, Serialize};

/// Serializable replay parameters (all optional fields use serde defaults).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventShockReplayConfig {
    pub events_path: String,
    pub bars_path: String,
    #[serde(default = "default_events_format")]
    pub events_format: String,
    #[serde(default = "default_strategy")]
    pub strategy: String,
    #[serde(default)]
    pub out_dir: String,
    #[serde(default)]
    pub as_of_epoch_sec: Option<i64>,
    #[serde(default)]
    pub min_lead_secs: i64,
    #[serde(default = "max_i64")]
    pub max_lead_secs: i64,
    #[serde(default = "default_control_seed")]
    pub control_seed: u64,
    #[serde(default)]
    pub skip_replay_verify: bool,
    /// [`crate::ExecutionEntryTiming`] as string: `next_session_open` | `entry_session_open`.
    #[serde(default = "default_execution_entry")]
    pub execution_entry: String,
}

fn default_events_format() -> String {
    "csv".into()
}

fn default_strategy() -> String {
    "xlu-spy-3".into()
}

fn max_i64() -> i64 {
    i64::MAX
}

fn default_control_seed() -> u64 {
    42
}

fn default_execution_entry() -> String {
    "next_session_open".into()
}

impl Default for EventShockReplayConfig {
    fn default() -> Self {
        Self {
            events_path: String::new(),
            bars_path: String::new(),
            events_format: default_events_format(),
            strategy: default_strategy(),
            out_dir: "event_shock_out".into(),
            as_of_epoch_sec: None,
            min_lead_secs: 0,
            max_lead_secs: max_i64(),
            control_seed: default_control_seed(),
            skip_replay_verify: false,
            execution_entry: default_execution_entry(),
        }
    }
}

impl EventShockReplayConfig {
    pub fn from_json_str(s: &str) -> Result<Self, String> {
        serde_json::from_str(s).map_err(|e| e.to_string())
    }

    pub fn from_json_path(path: &std::path::Path) -> Result<Self, String> {
        let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        Self::from_json_str(&raw)
    }
}
