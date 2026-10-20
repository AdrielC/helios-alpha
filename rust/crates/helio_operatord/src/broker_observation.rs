//! Read-only projection of the private connected-broker observation.
//! This never populates paper inventory or provides execution authority.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::Path;

#[derive(Debug, Deserialize, Serialize)]
pub struct Holding {
    asset: String,
    quantity: String,
    sellable_quantity: String,
    quantity_in_open_sells: String,
    managed_by_helios: bool,
    #[serde(default)]
    cost_basis: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Quote {
    symbol: String,
    bid: String,
    ask: String,
    #[serde(default)]
    mark: Option<String>,
    quoted_ms: u64,
    book_valid: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Portfolio {
    currency: String,
    total_value: String,
    cash: String,
    crypto_value: String,
    crypto_buying_power: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Account {
    crypto: String,
}

#[derive(Debug, Deserialize)]
struct StoredObservation {
    schema_version: u8,
    provider: String,
    source: String,
    execution_authority: bool,
    observed_ms: u64,
    imported_ms: u64,
    received_ms: std::collections::BTreeMap<String, u64>,
    account: Account,
    positions: Vec<Holding>,
    portfolio: Portfolio,
    #[serde(default)]
    quotes: Vec<Quote>,
    #[serde(default)]
    open_orders: Vec<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct BrokerObservation {
    pub state: &'static str,
    pub execution_authority: bool,
    pub observation: Option<ObservationView>,
}

#[derive(Debug, Serialize)]
pub struct ObservationView {
    provider: String,
    source: String,
    observed_ms: u64,
    positions_observed_ms: u64,
    read_at_ms: u64,
    account: Account,
    positions: Vec<Holding>,
    portfolio: Portfolio,
    quotes: Vec<Quote>,
    open_order_count: usize,
}

impl BrokerObservation {
    pub fn unavailable(state: &'static str) -> Self {
        Self {
            state,
            execution_authority: false,
            observation: None,
        }
    }
}

pub async fn read(path: Option<&Path>, now_ms: u64) -> BrokerObservation {
    let Some(path) = path else {
        return BrokerObservation::unavailable("not_connected");
    };
    let result = async {
        let metadata = tokio::fs::symlink_metadata(path).await.map_err(|_| ())?;
        if !metadata.is_file() || metadata.len() > 1024 * 1024 {
            return Err(());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o777 != 0o600 {
                return Err(());
            }
        }
        let raw = tokio::fs::read(path).await.map_err(|_| ())?;
        decode(&raw, now_ms)
    }
    .await;
    result.unwrap_or_else(|_| BrokerObservation::unavailable("unavailable"))
}

// Match the importer's sorted, ASCII-escaped Python JSON for integrity checks.
fn canonical(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let sorted: std::collections::BTreeMap<_, _> = map.iter().collect();
            format!(
                "{{{}}}",
                sorted
                    .into_iter()
                    .map(|(key, value)| format!(
                        "{}:{}",
                        canonical(&Value::String(key.clone())),
                        canonical(value)
                    ))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }
        Value::Array(values) => format!(
            "[{}]",
            values.iter().map(canonical).collect::<Vec<_>>().join(",")
        ),
        _ => serde_json::to_string(value)
            .expect("JSON value")
            .chars()
            .map(|ch| {
                if ch.is_ascii() {
                    ch.to_string()
                } else {
                    ch.encode_utf16(&mut [0; 2])
                        .iter()
                        .map(|unit| format!("\\u{unit:04x}"))
                        .collect()
                }
            })
            .collect(),
    }
}

fn amount(value: &str) -> bool {
    let unsigned = value.strip_prefix('-').unwrap_or(value);
    let parts: Vec<_> = unsigned.split('.').collect();
    (1..=2).contains(&parts.len())
        && parts
            .iter()
            .all(|part| (1..=18).contains(&part.len()) && part.bytes().all(|b| b.is_ascii_digit()))
}

fn decode(raw: &[u8], now_ms: u64) -> Result<BrokerObservation, ()> {
    if raw.len() > 1024 * 1024 {
        return Err(());
    }
    let mut value: Value = serde_json::from_slice(raw).map_err(|_| ())?;
    let hash = value
        .as_object_mut()
        .ok_or(())?
        .remove("sha256")
        .ok_or(())?;
    if hash.as_str() != Some(hex::encode(Sha256::digest(canonical(&value))).as_str()) {
        return Err(());
    }
    let stored: StoredObservation = serde_json::from_value(value).map_err(|_| ())?;
    let positions_observed_ms = *stored.received_ms.get("positions").ok_or(())?;
    if stored.schema_version != 1
        || stored.provider != "robinhood"
        || stored.source != "operator_import_of_connected_mcp_responses"
        || stored.execution_authority
        || stored.observed_ms > now_ms
        || stored.imported_ms > now_ms
        || stored.received_ms.values().any(|t| *t > now_ms)
        || stored.observed_ms != *stored.received_ms.values().min().ok_or(())?
        || stored.portfolio.currency != "USD"
        || !stored.account.crypto.starts_with("••••")
        || stored.account.crypto.chars().count() != 8
        || stored.positions.len() > 100
        || stored.positions.iter().any(|p| {
            p.managed_by_helios
                || p.asset.is_empty()
                || p.asset.len() > 12
                || !p
                    .asset
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
                || [&p.quantity, &p.sellable_quantity, &p.quantity_in_open_sells]
                    .iter()
                    .any(|v| !amount(v) || v.starts_with('-'))
                || p.cost_basis
                    .as_ref()
                    .is_some_and(|v| !amount(v) || v.starts_with('-'))
        })
        || stored.quotes.len() > 100
        || stored.open_orders.len() > 100
        || stored.quotes.iter().any(|q| {
            q.symbol.len() > 20
                || q.quoted_ms > now_ms + 1000
                || [&q.bid, &q.ask]
                    .iter()
                    .any(|v| !amount(v) || v.starts_with('-'))
                || q.mark
                    .as_ref()
                    .is_some_and(|v| !amount(v) || v.starts_with('-'))
        })
        || [
            &stored.portfolio.total_value,
            &stored.portfolio.cash,
            &stored.portfolio.crypto_value,
        ]
        .iter()
        .any(|v| !amount(v))
        || stored
            .portfolio
            .crypto_buying_power
            .as_ref()
            .is_some_and(|v| !amount(v))
    {
        return Err(());
    }
    Ok(BrokerObservation {
        state: if now_ms - stored.observed_ms > 60_000 {
            "stale"
        } else {
            "available"
        },
        execution_authority: false,
        observation: Some(ObservationView {
            provider: stored.provider,
            source: stored.source,
            observed_ms: stored.observed_ms,
            positions_observed_ms,
            read_at_ms: now_ms,
            account: stored.account,
            positions: stored.positions,
            portfolio: stored.portfolio,
            quotes: stored.quotes,
            open_order_count: stored.open_orders.len(),
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Value {
        serde_json::json!({"schema_version":1,"provider":"robinhood",
            "source":"operator_import_of_connected_mcp_responses","execution_authority":false,
            "observed_ms":1000,"imported_ms":1100,"received_ms":{"positions":1000},
            "account":{"crypto":"••••1234","brokerage":"must-not-be-returned"},
            "account_key":"must-not-be-returned","previews":[{"id":"not-a-real-order"}],
            "positions":[{"asset":"BTC","quantity":"0.12345678","sellable_quantity":"0.10000000",
                "quantity_in_open_sells":"0.02345678","managed_by_helios":false}],
            "portfolio":{"currency":"USD","total_value":"150","cash":"100","crypto_value":"50","crypto_buying_power":null}})
    }
    fn sign(mut value: Value) -> Vec<u8> {
        value["sha256"] = hex::encode(Sha256::digest(canonical(&value))).into();
        serde_json::to_vec(&value).unwrap()
    }
    #[test]
    fn quantities_remain_exact_and_only_display_fields_leave_gateway() {
        let value = serde_json::to_value(decode(&sign(fixture()), 1200).unwrap()).unwrap();
        assert_eq!(value["state"], "available");
        assert_eq!(
            value["observation"]["positions"][0]["quantity"],
            "0.12345678"
        );
        assert!(!value.to_string().contains("must-not-be-returned"));
        assert!(!value.to_string().contains("not-a-real-order"));
        assert_eq!(value["execution_authority"], false);
    }
    #[test]
    fn quotes_and_complete_basis_are_projected_without_order_details() {
        let mut data = fixture();
        data["positions"][0]["cost_basis"] = "12.34".into();
        data["quotes"] = serde_json::json!([{"symbol":"BTCUSD","bid":"100","ask":"102","mark":"101","quoted_ms":1100,"book_valid":true}]);
        data["open_orders"] = serde_json::json!([{"id":"private-order-id"}]);
        let view = serde_json::to_value(decode(&sign(data.clone()), 1200).unwrap()).unwrap();
        assert_eq!(view["observation"]["quotes"][0]["mark"], "101");
        assert_eq!(view["observation"]["positions"][0]["cost_basis"], "12.34");
        assert_eq!(view["observation"]["open_order_count"], 1);
        assert!(!view.to_string().contains("private-order-id"));
        data["quotes"][0]["mark"] = "NaN".into();
        assert!(decode(&sign(data), 1200).is_err());
    }

    #[test]
    fn stale_data_keeps_holdings_but_future_or_tampered_data_is_rejected() {
        assert_eq!(decode(&sign(fixture()), 61_001).unwrap().state, "stale");
        assert!(decode(&sign(fixture()), 900).is_err());
        let mut signed: Value = serde_json::from_slice(&sign(fixture())).unwrap();
        signed["positions"][0]["quantity"] = "99".into();
        assert!(decode(&serde_json::to_vec(&signed).unwrap(), 1200).is_err());
        let mut invalid = fixture();
        invalid["positions"][0]["quantity"] = "NaN".into();
        assert!(decode(&sign(invalid), 1200).is_err());
    }
    #[test]
    fn canonical_matches_python_ascii_encoding() {
        assert_eq!(
            canonical(&serde_json::json!({"z":"••😀","a":1})),
            "{\"a\":1,\"z\":\"\\u2022\\u2022\\ud83d\\ude00\"}"
        );
    }
    #[tokio::test]
    async fn missing_connection_is_not_an_empty_account() {
        let missing = read(None, 1200).await;
        assert_eq!(missing.state, "not_connected");
        assert!(missing.observation.is_none());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn file_reads_follow_updates_empty_accounts_and_permission_failures() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let directory =
            std::env::temp_dir().join(format!("helios-holdings-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("account.json");
        std::fs::write(&path, sign(fixture())).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(
            read(Some(&path), 1200)
                .await
                .observation
                .unwrap()
                .positions
                .len(),
            1
        );
        let mut updated = fixture();
        updated["positions"][0]["asset"] = "ETH".into();
        updated["positions"][0]["quantity"] = "1.987654321".into();
        std::fs::write(&path, sign(updated.clone())).unwrap();
        let refreshed = read(Some(&path), 1200).await.observation.unwrap();
        assert_eq!(refreshed.positions[0].asset, "ETH");
        assert_eq!(refreshed.positions[0].quantity, "1.987654321");
        updated["positions"] = serde_json::json!([]);
        std::fs::write(&path, sign(updated)).unwrap();
        assert!(read(Some(&path), 1200)
            .await
            .observation
            .unwrap()
            .positions
            .is_empty());
        let link = directory.join("link.json");
        symlink(&path, &link).unwrap();
        assert_eq!(read(Some(&link), 1200).await.state, "unavailable");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(read(Some(&path), 1200).await.state, "unavailable");
        std::fs::remove_dir_all(directory).unwrap();
    }
}
