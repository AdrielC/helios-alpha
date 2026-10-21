//! Read-only, bounded projection of durable event and forecast records.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;

#[derive(Deserialize, Serialize)]
struct Contract {
    id: String,
    title: String,
    closes_ms: u64,
    state: String,
    probability: Option<f64>,
    observed_ms: Option<u64>,
    forecast_id: Option<String>,
    definition_id: String,
    topic: String,
    symbols: Vec<String>,
}
#[derive(Deserialize, Serialize)]
struct Event {
    id: String,
    source_id: String,
    provider: String,
    title: String,
    first_seen_ms: u64,
    last_seen_ms: u64,
    closes_ms: u64,
    state: String,
    topics: Vec<String>,
    symbols: Vec<String>,
    contracts: Vec<Contract>,
}
#[derive(Deserialize, Serialize)]
struct Version {
    id: String,
    version: u64,
    issued_ms: u64,
    observed_ms: u64,
    probability: f64,
    definition_id: String,
    rule_revision: u64,
}
#[derive(Deserialize, Serialize)]
struct Policy {
    revision: u64,
    max_cache_age_seconds: u64,
    max_spread_pp: f64,
    min_depth_usd: f64,
    forecast_interval_seconds: u64,
    forecast_min_interval_seconds: u64,
    forecast_move_pp: f64,
}
#[derive(Deserialize, Serialize)]
struct Forecast {
    id: String,
    event_key: String,
    provider: String,
    title: String,
    topic: String,
    symbols: Vec<String>,
    resolution: String,
    url: String,
    closes_ms: u64,
    state: String,
    kind: String,
    method: String,
    version_id: String,
    version: u64,
    definition_id: String,
    issued_ms: u64,
    observed_ms: u64,
    confirmed_ms: u64,
    probability: f64,
    current_probability: Option<f64>,
    current_observed_ms: Option<u64>,
    bid: f64,
    ask: f64,
    depth_usd: f64,
    observation_seq: u64,
    rule_revision: u64,
    policy: Policy,
    outcome: Option<f64>,
    score: Option<f64>,
    history: Vec<Version>,
}
#[derive(Deserialize, Serialize)]
struct Snapshot {
    schema_version: u64,
    observed_ms: u64,
    execution_authority: bool,
    events: Vec<Event>,
    forecasts: Vec<Forecast>,
    total_contracts: u64,
    total_events: u64,
    total_forecasts: u64,
    retained_versions: u64,
    retention_days: u64,
    display_limit: u64,
    issuance_interval_seconds: u64,
    issuance_move_pp: f64,
}
fn probability(value: f64) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}
fn project(value: Value, now: u64) -> Option<Value> {
    let data: Snapshot = serde_json::from_value(value).ok()?;
    if data.schema_version != 1
        || data.execution_authority
        || data.events.len() > 100
        || data.forecasts.len() > 100
        || data.events.iter().map(|e| e.contracts.len()).sum::<usize>() > 100
        || data.events.iter().any(|e| {
            e.title.len() > 1200
                || e.contracts
                    .iter()
                    .any(|c| c.title.len() > 1200 || c.probability.is_some_and(|p| !probability(p)))
        })
        || data.forecasts.iter().any(|f| {
            f.title.len() > 1200
                || f.resolution.len() > 16000
                || f.history.len() > 12
                || f.kind != "market_implied"
                || !probability(f.probability)
                || f.current_probability.is_some_and(|p| !probability(p))
                || !probability(f.bid)
                || !probability(f.ask)
                || f.bid > f.ask
                || f.history.iter().any(|v| !probability(v.probability))
        })
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
    let path = root.join("research-registry-status.json");
    let Ok(info) = tokio::fs::symlink_metadata(&path).await else {
        return offline;
    };
    if !info.is_file() || info.len() > 4_194_304 {
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
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Value {
        json!({"schema_version":1,"observed_ms":100000,"execution_authority":false,
            "events":[],"forecasts":[],"total_contracts":0,"total_events":0,"total_forecasts":0,
            "retained_versions":0,"retention_days":30,"display_limit":100,
            "issuance_interval_seconds":300,"issuance_move_pp":1,"access_token":"omit"})
    }
    #[test]
    fn projects_only_registry_fields_and_reports_offline_without_losing_history() {
        let fresh = project(fixture(), 100001).unwrap();
        assert_eq!(fresh["online"], true);
        assert!(fresh.get("access_token").is_none());
        assert_eq!(project(fixture(), 116000).unwrap()["online"], false);
        assert_eq!(project(fixture(), 99999).unwrap()["online"], false);
        let mut invalid = fixture();
        invalid["execution_authority"] = json!(true);
        assert!(project(invalid, 100001).is_none());
        assert!(project(json!({"schema_version":1}), 100001).is_none());
    }
    #[tokio::test]
    async fn reads_private_projection_and_rejects_symlink() {
        let root = std::env::temp_dir().join(format!("helios-registry-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir(&root).await.unwrap();
        assert_eq!(status(Some(&root), 100001).await["online"], false);
        let path = root.join("research-registry-status.json");
        tokio::fs::write(&path, serde_json::to_vec(&fixture()).unwrap())
            .await
            .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::{symlink, PermissionsExt};
            tokio::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                .await
                .unwrap();
            assert_eq!(status(Some(&root), 100001).await["online"], true);
            tokio::fs::rename(&path, root.join("source")).await.unwrap();
            symlink(root.join("source"), &path).unwrap();
            assert_eq!(status(Some(&root), 100001).await["online"], false);
        }
        tokio::fs::remove_dir_all(root).await.unwrap();
    }
}
