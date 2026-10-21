//! Public research projection, with explicit field selection and no broker authority.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::Path};

#[derive(Deserialize, Serialize)]
struct Source {
    source: String,
    state: String,
    checked_ms: u64,
}
#[derive(Deserialize, Serialize)]
struct Counts {
    evidence: u64,
    quotes: u64,
    decisions: u64,
}
#[derive(Deserialize, Serialize)]
struct Storage {
    id: String,
    source: String,
    available_ms: u64,
    period: String,
    release_date: String,
    storage_bcf: f64,
    change_bcf: f64,
    seasonal_average_bcf: f64,
    revision: bool,
}
#[derive(Deserialize, Serialize)]
struct Weather {
    id: String,
    source: String,
    available_ms: u64,
    published_ms: u64,
    kind: String,
    period: String,
    daily: BTreeMap<String, u64>,
    total: u64,
    weighting: String,
}
#[derive(Deserialize, Serialize)]
struct Signal {
    kind: String,
    value: f64,
    dates: Vec<String>,
    available_ms: u64,
    published_ms: u64,
    evidence_ids: Vec<String>,
    candidate: bool,
    baseline_samples: u64,
    mean: Option<f64>,
    stddev: Option<f64>,
    z_score: Option<f64>,
    ewma: Option<f64>,
}
#[derive(Deserialize, Serialize)]
struct Premium {
    contract_id: String,
    observed_ms: u64,
    available_ms: u64,
    bid_cents: u64,
    ask_cents: u64,
    bid_size: u64,
    ask_size: u64,
    kind: String,
    strike_cents: u64,
    expiry: String,
    multiplier: u64,
    premium_cents: u64,
    spread_cost_cents: u64,
    fresh: bool,
    regular_session: bool,
}
#[derive(Deserialize, Serialize)]
struct Hypothesis {
    id: String,
    signal: String,
    requires: String,
}
#[derive(Deserialize, Serialize)]
struct ExitProfile {
    id: String,
    holding_ms: u64,
    take_profit_bps: u64,
    stop_loss_bps: u64,
}
#[derive(Deserialize, Serialize)]
struct Backtest {
    state: String,
    profitability_established: bool,
    blockers: Vec<String>,
}
#[derive(Deserialize, Serialize)]
struct Snapshot {
    schema_version: u64,
    observed_ms: u64,
    execution_authority: bool,
    mode: String,
    config_digest: String,
    counts: Counts,
    sources: Vec<Source>,
    storage: Option<Storage>,
    weather: Vec<Weather>,
    signals: Vec<Signal>,
    premiums: Vec<Premium>,
    hypotheses: Vec<Hypothesis>,
    decision_enabled: bool,
    backtest: Backtest,
    exit_profiles: Vec<ExitProfile>,
    max_quote_age_ms: u64,
}
fn project(value: Value, now: u64) -> Option<Value> {
    let mut data: Snapshot = serde_json::from_value(value).ok()?;
    if data.schema_version != 1
        || data.execution_authority
        || data.mode != "research"
        || data.sources.len() > 4
        || data.weather.len() > 2
        || data.signals.len() > 2
        || data.premiums.len() > 12
        || data.hypotheses.len() > 10
        || data.exit_profiles.len() > 10
        || !(1..=60000).contains(&data.max_quote_age_ms)
        || data.backtest.profitability_established
        || data.premiums.iter().any(|q| {
            q.bid_cents > q.ask_cents
                || q.multiplier == 0
                || q.observed_ms > q.available_ms
                || q.ask_cents.checked_mul(q.multiplier) != Some(q.premium_cents)
                || (q.ask_cents - q.bid_cents).checked_mul(q.multiplier)
                    != Some(q.spread_cost_cents)
        })
    {
        return None;
    }
    let online = data.observed_ms <= now && now - data.observed_ms <= 15000;
    for q in &mut data.premiums {
        q.fresh = q.fresh
            && online
            && q.observed_ms <= now
            && now - q.observed_ms <= data.max_quote_age_ms;
    }
    let mut result = serde_json::to_value(data).ok()?;
    result["online"] = json!(online);
    Some(result)
}
pub async fn status(root: Option<&Path>, now: u64) -> Value {
    let offline = json!({"schema_version":1,"online":false,"execution_authority":false});
    let Some(root) = root else {
        return offline;
    };
    let path = root.join("natgas-status.json");
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
    let Ok(wire) = tokio::fs::read(path).await else {
        return offline;
    };
    serde_json::from_slice(&wire)
        .ok()
        .and_then(|v| project(v, now))
        .unwrap_or(offline)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Value {
        json!({"schema_version":1,"observed_ms":100,"execution_authority":false,
          "mode":"research","config_digest":"test","counts":{"evidence":0,"quotes":0,"decisions":0},
          "sources":[],"storage":null,"weather":[],"signals":[],"premiums":[],"hypotheses":[],
          "exit_profiles":[],"max_quote_age_ms":10000,
          "decision_enabled":false,"backtest":{"state":"collecting_evidence",
          "profitability_established":false,"blockers":[]},"access_token":"must disappear"})
    }
    #[test]
    fn bounds_authority_freshness_and_private_fields() {
        let mut v = fixture();
        assert!(project(v.clone(), 101)
            .unwrap()
            .get("access_token")
            .is_none());
        assert_eq!(project(v.clone(), 15101).unwrap()["online"], false);
        assert_eq!(project(v.clone(), 99).unwrap()["online"], false);
        v["execution_authority"] = json!(true);
        assert!(project(v, 101).is_none());
    }
}
