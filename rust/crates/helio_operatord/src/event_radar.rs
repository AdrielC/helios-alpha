//! Minimized loopback projection and research-detector control. No execution authority.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;

#[derive(Deserialize, Serialize)]
struct RuleConfig {
    revision: u64,
    enabled: bool,
    poll_seconds: u64,
    lookback_seconds: u64,
    move_pp: f64,
    reset_pp: f64,
    cooldown_seconds: u64,
    max_spread_pp: f64,
    min_depth_usd: f64,
    retention_days: u64,
}
#[derive(Deserialize, Serialize)]
struct Quote {
    observed_ms: u64,
    received_ms: u64,
    changed_ms: Option<u64>,
    probability: f64,
    bid: f64,
    ask: f64,
    depth_usd: f64,
    spread_pp: f64,
}
#[derive(Deserialize, Serialize)]
struct Point {
    at_ms: u64,
    probability: f64,
}
#[derive(Deserialize, Serialize)]
struct Market {
    id: String,
    market_id: String,
    event_id: String,
    provider: String,
    title: String,
    #[serde(default)]
    resolution: String,
    url: String,
    topic: String,
    symbols: Vec<String>,
    closes_ms: u64,
    volume_24h: f64,
    volume_unit: String,
    state: String,
    delta_pp: Option<f64>,
    latest: Option<Quote>,
    history: Vec<Point>,
    samples: u64,
}
#[derive(Deserialize, Serialize)]
struct Trigger {
    id: String,
    market_id: String,
    event_id: String,
    provider: String,
    title: String,
    topic: String,
    symbols: Vec<String>,
    observed_ms: u64,
    received_ms: u64,
    baseline_observed_ms: u64,
    probability: f64,
    previous_probability: f64,
    delta_pp: f64,
    spread_pp: f64,
    depth_usd: f64,
    rule_revision: u64,
    expires_ms: u64,
    execution_authority: bool,
    delivered: bool,
    stream_sequence: Option<u64>,
}
#[derive(Deserialize, Serialize)]
struct Source {
    id: String,
    state: String,
    last_ok_ms: Option<u64>,
    markets: u64,
    quotes: u64,
    error: Option<String>,
}
#[derive(Deserialize, Serialize)]
struct Transport {
    state: String,
    subject: String,
    error: Option<String>,
}
#[derive(Deserialize, Serialize)]
struct Golem {
    state: String,
    last_ok_ms: Option<u64>,
    error: Option<String>,
}
#[derive(Deserialize, Serialize)]
struct Command {
    id: String,
    state: String,
}
#[derive(Deserialize, Serialize)]
struct News {
    news_id: String,
    source: String,
    title: String,
    url: String,
    published_ms: Option<u64>,
    observed_ms: u64,
    eligible: bool,
    intake_reason: String,
}
#[derive(Deserialize, Serialize)]
struct Snapshot {
    schema_version: u64,
    observed_ms: u64,
    execution_authority: bool,
    config: RuleConfig,
    markets: Vec<Market>,
    triggers: Vec<Trigger>,
    recorded_observations: u64,
    registered_events: u64,
    pending_signals: u64,
    sources: Vec<Source>,
    transport: Transport,
    golem: Golem,
    last_command: Option<Command>,
    news: Vec<News>,
}

fn project(value: Value, now: u64) -> Option<Value> {
    let data: Snapshot = serde_json::from_value(value).ok()?;
    if data.schema_version != 1
        || data.execution_authority
        || data.markets.len() > 40
        || data.triggers.len() > 30
        || data.news.len() > 12
        || data.sources.len() > 2
        || data.markets.iter().any(|m| {
            m.title.len() > 1200
                || m.resolution.len() > 16000
                || m.history.len() > 120
                || m.symbols.len() > 20
        })
        || data
            .triggers
            .iter()
            .any(|signal| signal.execution_authority)
    {
        return None;
    }
    let online = data.observed_ms <= now && now - data.observed_ms <= 15000;
    let mut result = serde_json::to_value(data).ok()?;
    result["online"] = json!(online);
    Some(result)
}

pub async fn status(root: Option<&Path>, now: u64) -> Value {
    let offline = json!({"schema_version":1,"online":false,"execution_authority":false});
    let Some(root) = root else {
        return offline;
    };
    let path = root.join("event-radar-status.json");
    let Ok(info) = tokio::fs::symlink_metadata(&path).await else {
        return offline;
    };
    if !info.is_file() || info.len() > 1_048_576 {
        return offline;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if info.permissions().mode() & 0o777 != 0o600 {
            return offline;
        }
    }
    let Ok(wire) = tokio::fs::read(&path).await else {
        return offline;
    };
    serde_json::from_slice(&wire)
        .ok()
        .and_then(|value| project(value, now))
        .unwrap_or(offline)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Update {
    pub enabled: bool,
    pub expected_revision: u64,
}

pub async fn request(root: &Path, update: Update, now: u64) -> Result<Value, ()> {
    let current = status(Some(root), now).await;
    if current["online"] != true || current["config"]["revision"] != update.expected_revision {
        return Err(());
    }
    let id = uuid::Uuid::new_v4().to_string();
    let wire = serde_json::to_vec(&json!({"id":id,"enabled":update.enabled,"expected_revision":update.expected_revision,"requested_ms":now})).map_err(|_| ())?;
    let temporary = root.join(format!(".event-radar-request-{id}"));
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    use tokio::io::AsyncWriteExt;
    let mut file = options.open(&temporary).await.map_err(|_| ())?;
    let result = async {
        file.write_all(&wire).await.map_err(|_| ())?;
        file.sync_all().await.map_err(|_| ())?;
        // Link is atomic and fails if another control request is already queued.
        tokio::fs::hard_link(&temporary, root.join("event-radar-request.json"))
            .await
            .map_err(|_| ())?;
        Ok(json!({"id":id,"state":"queued"}))
    }
    .await;
    let _ = tokio::fs::remove_file(temporary).await;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Value {
        json!({"schema_version":1,"observed_ms":100000,"execution_authority":false,
            "config":{"revision":1,"enabled":true,"poll_seconds":30,"lookback_seconds":600,
                "move_pp":5,"reset_pp":2,"cooldown_seconds":900,"max_spread_pp":5,
                "min_depth_usd":100,"retention_days":30,"private_field":"omit"},
            "markets":[],"triggers":[],"recorded_observations":12,"registered_events":2,
            "pending_signals":0,"sources":[],"transport":{"state":"ready","subject":"test"},
            "golem":{"state":"recorded"},"last_command":null,"news":[],"access_token":"omit"})
    }
    #[test]
    fn projection_strips_unknown_fields_and_ages_without_erasing_history() {
        let fresh = project(fixture(), 100001).unwrap();
        assert_eq!(fresh["online"], true);
        assert!(fresh.get("access_token").is_none());
        assert!(fresh["config"].get("private_field").is_none());
        let stale = project(fixture(), 116000).unwrap();
        assert_eq!(stale["online"], false);
        assert_eq!(stale["recorded_observations"], 12);
        let mut invalid = fixture();
        invalid["execution_authority"] = json!(true);
        assert!(project(invalid, 100001).is_none());
    }
    #[tokio::test]
    async fn detector_change_is_revision_guarded_atomic_and_never_overwrites_pending() {
        let root = std::env::temp_dir().join(format!("helios-event-test-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir(&root).await.unwrap();
        let path = root.join("event-radar-status.json");
        tokio::fs::write(&path, serde_json::to_vec(&fixture()).unwrap())
            .await
            .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            tokio::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                .await
                .unwrap();
        }
        assert!(request(
            &root,
            Update {
                enabled: false,
                expected_revision: 2
            },
            100001
        )
        .await
        .is_err());
        let ack = request(
            &root,
            Update {
                enabled: false,
                expected_revision: 1,
            },
            100001,
        )
        .await
        .unwrap();
        let pending: Value = serde_json::from_slice(
            &tokio::fs::read(root.join("event-radar-request.json"))
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(ack["id"], pending["id"]);
        assert_eq!(pending["enabled"], false);
        assert!(pending.get("execution_authority").is_none());
        assert!(request(
            &root,
            Update {
                enabled: true,
                expected_revision: 1
            },
            100002
        )
        .await
        .is_err());
        assert!(request(
            &root,
            Update {
                enabled: false,
                expected_revision: 1
            },
            116000
        )
        .await
        .is_err());
        tokio::fs::remove_dir_all(root).await.unwrap();
    }
    #[test]
    fn rejects_execution_authority_and_malformed_snapshots() {
        assert!(project(json!({"schema_version":1,"execution_authority":true}), 10).is_none());
        assert!(project(json!({"access_token":"secret"}), 10).is_none());
    }
}
