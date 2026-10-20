//! Loopback-only control of the read-only account poller. Never invokes trading.
use serde_json::{json, Value};
use std::path::Path;

#[derive(serde::Deserialize, serde::Serialize)]
struct StrategyObservation {
    observed_ms: u64,
    mode: String,
    paused: bool,
    live_admitted: bool,
    live_blockers: Vec<String>,
    paper_orders: u64,
    paper_positions: u64,
    model_calls_reserved: u64,
    model_daily_limit: u64,
    books: Vec<StrategyBook>,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct StrategyBook {
    id: String,
    engine: String,
    state: String,
    mode: String,
    updated_ms: u64,
    feed_connected: bool,
    active_news: u64,
    signals: Vec<StrategySignal>,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct StrategySignal { symbol: String, reason: String }

#[derive(serde::Deserialize, serde::Serialize)]
struct AccountPermissions {
    observed_ms: u64,
    options_level: String,
    approval_required: bool,
}

fn recent(timestamp: u64, now: u64, max_age: u64) -> bool {
    timestamp <= now && now - timestamp <= max_age
}

fn project_strategy(value: &Value, now: u64) -> Value {
    let Ok(data) = serde_json::from_value::<StrategyObservation>(value.clone()) else { return Value::Null; };
    if !recent(data.observed_ms, now, 15_000) || data.books.len() > 20 || data.live_blockers.len() > 20 {
        return Value::Null;
    }
    // Typed reserialization excludes arbitrary source, account IDs and credentials.
    serde_json::to_value(data).unwrap_or(Value::Null)
}

async fn private_json(path: &Path) -> Option<Value> {
    let metadata = tokio::fs::symlink_metadata(path).await.ok()?;
    if !metadata.is_file() || metadata.len() > 32_768 {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o777 != 0o600 {
            return None;
        }
    }
    serde_json::from_slice(&tokio::fs::read(path).await.ok()?).ok()
}

/// Local display identity is independent of command authentication and risk admission.
pub async fn operator_profile(root: Option<&Path>) -> Value {
    let fallback = json!({"display_name":"Operator", "role":"operator"});
    let Some(root) = root else { return fallback; };
    let Some(value) = private_json(&root.join("operator-profile.json")).await else { return fallback; };
    let Some(name) = value["display_name"].as_str() else { return fallback; };
    if value["schema_version"] != 1 || value["role"] != "owner"
        || name.trim().is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
        return fallback;
    }
    json!({"display_name":name.trim(), "role":"owner"})
}

pub async fn status(root: Option<&Path>, now_ms: u64) -> Value {
    let offline = json!({"state":"offline", "execution_authority":false});
    let Some(root) = root else {
        return offline;
    };
    let Some(value) = private_json(&root.join("broker-sync-status.json")).await else {
        return offline;
    };
    let heartbeat = value["heartbeat_ms"].as_u64().unwrap_or(0);
    if heartbeat > now_ms || now_ms - heartbeat > 10_000 || value["execution_authority"] != false {
        return offline;
    }
    let state = value["state"].as_str().unwrap_or("offline");
    if ![
        "connected",
        "syncing",
        "authorizing",
        "authorization_required",
        "degraded",
        "offline",
    ]
    .contains(&state)
    {
        return offline;
    }
    // Project known non-secret fields only. OAuth tokens and client credentials
    // live in different files and can never pass through this endpoint.
    let mut result = json!({"state":state, "execution_authority":false});
    for key in [
        "interval_ms",
        "last_success_ms",
        "last_attempt_ms",
        "next_poll_ms",
        "heartbeat_ms",
    ] {
        result[key] = value[key].as_u64().map(Value::from).unwrap_or(Value::Null);
    }
    result["strategy"] = project_strategy(&value["strategy"], now_ms);
    if let Ok(permissions) = serde_json::from_value::<AccountPermissions>(value["account_permissions"].clone()) {
        if recent(permissions.observed_ms, now_ms, 60_000)
            && ["option_level_2", "option_level_3", "none"].contains(&permissions.options_level.as_str()) {
            result["account_permissions"] = serde_json::to_value(permissions).unwrap_or(Value::Null);
        }
    }
    if state == "authorizing" {
        if let Some(url) = value["authorization_url"].as_str() {
            let valid = url.parse::<http::Uri>().ok().is_some_and(|uri| {
                uri.scheme_str() == Some("https")
                    && uri
                        .host()
                        .is_some_and(|h| h == "robinhood.com" || h.ends_with(".robinhood.com"))
            });
            if valid && url.len() < 8192 {
                result["authorization_url"] = json!(url);
            }
        }
    }
    result
}

pub async fn request(root: &Path, action: &str, now_ms: u64) -> Result<(), ()> {
    if !["connect", "refresh"].contains(&action) {
        return Err(());
    }
    let current = status(Some(root), now_ms).await;
    if current["state"] == "offline" {
        return Err(());
    }
    let id = uuid::Uuid::new_v4().to_string();
    let temporary = root.join(format!(".broker-sync-request-{id}"));
    let wire = serde_json::to_vec(&json!({"id":id, "action":action, "requested_ms":now_ms}))
        .map_err(|_| ())?;
    // Create-new + atomic rename: never follow a request-file symlink.
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let file = options.open(&temporary).await.map_err(|_| ())?;
    drop(file);
    let outcome = async {
        tokio::fs::write(&temporary, wire).await.map_err(|_| ())?;
        tokio::fs::rename(&temporary, root.join("broker-sync-request.json"))
            .await
            .map_err(|_| ())
    }
    .await;
    let _ = tokio::fs::remove_file(temporary).await;
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strategy_projection_expires_and_never_exposes_extra_fields() {
        let source = json!({"observed_ms":1000,"mode":"paper","paused":false,
            "live_admitted":false,"live_blockers":[],"paper_orders":0,"paper_positions":0,
            "model_calls_reserved":0,"model_daily_limit":12,"books":[{
                "id":"worker","engine":"custom","state":"running","mode":"paper",
                "updated_ms":900,"feed_connected":true,"active_news":0,"signals":[],
                "strategy_source":"secret"}],"access_token":"secret"});
        let output = project_strategy(&source, 1500);
        assert_eq!(output["books"][0]["engine"], "custom");
        assert!(!output.to_string().contains("secret"));
        assert!(project_strategy(&source, 16001).is_null());
        assert!(project_strategy(&source, 999).is_null());
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn status_excludes_secrets_and_requests_expire_with_the_worker() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("helios-sync-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("broker-sync-status.json");
        let value = json!({"state":"connected", "heartbeat_ms":1000, "execution_authority":false,
            "last_success_ms":990, "interval_ms":30000, "access_token":"never-return", "authorization_url":"https://robinhood.com/oauth?state=hidden-when-connected"});
        std::fs::write(&path, value.to_string()).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let projected = status(Some(&root), 1200).await;
        assert_eq!(projected["state"], "connected");
        assert!(!projected.to_string().contains("never-return"));
        assert!(projected["authorization_url"].is_null());
        assert!(request(&root, "refresh", 1200).await.is_ok());
        let requested = private_json(&root.join("broker-sync-request.json"))
            .await
            .unwrap();
        assert_eq!(requested["action"], "refresh");
        assert_eq!(status(Some(&root), 12000).await["state"], "offline");
        assert!(request(&root, "refresh", 12000).await.is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn owner_profile_is_private_display_metadata_without_command_authority() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let root = std::env::temp_dir().join(format!("helios-profile-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("operator-profile.json");
        assert_eq!(operator_profile(Some(&root)).await["role"], "operator");
        let value = json!({"schema_version":1,"display_name":" Test Owner ","role":"owner",
            "execution_authority":true,"access_token":"never-return"});
        std::fs::write(&path, value.to_string()).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(operator_profile(Some(&root)).await,
            json!({"display_name":"Test Owner","role":"owner"}));
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(operator_profile(Some(&root)).await["role"], "operator");
        let target = root.join("target.json");
        std::fs::rename(&path, &target).unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
        symlink(&target, &path).unwrap();
        assert_eq!(operator_profile(Some(&root)).await["role"], "operator");
        std::fs::remove_file(&path).unwrap();
        for name in ["", " ", "line\nbreak", &"a".repeat(81)] {
            std::fs::write(&path, json!({"schema_version":1,"role":"owner","display_name":name}).to_string()).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            assert_eq!(operator_profile(Some(&root)).await["role"], "operator");
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn offline_and_stale_workers_cannot_accept_refreshes() {
        assert_eq!(status(None, 100).await["state"], "offline");
        assert!(request(Path::new("/does-not-exist"), "refresh", 100)
            .await
            .is_err());
        assert!(request(Path::new("/does-not-exist"), "place_order", 100)
            .await
            .is_err());
    }
}
