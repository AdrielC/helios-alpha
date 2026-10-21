//! Data-only view registry. Adding a publisher never requires a gateway route.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Metrics,
    Table,
    Notes,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Format {
    Text,
    Number,
    Currency,
    Percent,
    Datetime,
    Duration,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Field {
    key: String,
    label: String,
    format: Format,
    scale: f64,
    precision: u8,
    unit: String,
    currency: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Row {
    id: String,
    values: BTreeMap<String, Value>,
    observed_ms: Option<u64>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Section {
    id: String,
    kind: Kind,
    title: String,
    description: String,
    empty: String,
    fields: Vec<Field>,
    rows: Vec<Row>,
    max_age_ms: Option<u64>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Book {
    schema_version: u8,
    id: String,
    execution_authority: bool,
    title: String,
    subtitle: String,
    mark: String,
    capabilities: Vec<String>,
    observed_ms: u64,
    lease_ms: u64,
    sections: Vec<Section>,
}
fn identifier(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && s.as_bytes()[0].is_ascii_lowercase()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}
fn text(s: &str) -> bool {
    s.chars().count() <= 4096
}
fn cell(value: &Value) -> bool {
    match value {
        Value::Null | Value::Bool(_) => true,
        Value::String(s) => text(s),
        Value::Number(n) => n
            .as_f64()
            .is_some_and(|v| v.is_finite() && v.abs() <= 9_007_199_254_740_991.0),
        _ => false,
    }
}
fn project(value: Value, filename: &str, now: u64) -> Option<Value> {
    let book: Book = serde_json::from_value(value).ok()?;
    if book.schema_version != 1
        || book.execution_authority
        || !identifier(&book.id)
        || filename != format!("{}.json", book.id)
        || !text(&book.title)
        || !text(&book.subtitle)
        || book.mark.chars().count() > 8
        || book.capabilities.len() > 8
        || !book.capabilities.iter().all(|s| text(s))
        || !(1000..=300_000).contains(&book.lease_ms)
        || book.observed_ms > 8_640_000_000_000_000
        || book.sections.len() > 16
    {
        return None;
    }
    let mut section_ids = BTreeSet::new();
    for section in &book.sections {
        if !identifier(&section.id)
            || !section_ids.insert(&section.id)
            || ![&section.title, &section.description, &section.empty]
                .iter()
                .all(|s| text(s))
            || !(1..=12).contains(&section.fields.len())
            || section.rows.len() > 100
            || section
                .max_age_ms
                .is_some_and(|v| !(1..=604_800_000).contains(&v))
        {
            return None;
        }
        let mut fields = BTreeSet::new();
        for field in &section.fields {
            if !identifier(&field.key)
                || !fields.insert(&field.key)
                || !text(&field.label)
                || !text(&field.unit)
                || field.precision > 6
                || !field.scale.is_finite()
                || field.scale <= 0.0
                || field.scale > 1_000_000.0
                || field.currency.len() != 3
                || !field.currency.bytes().all(|b| b.is_ascii_uppercase())
            {
                return None;
            }
        }
        let mut row_ids = BTreeSet::new();
        for row in &section.rows {
            if row.id.len() > 64
                || !row_ids.insert(&row.id)
                || row.values.keys().collect::<BTreeSet<_>>() != fields
                || !row.values.values().all(cell)
                || row
                    .observed_ms
                    .is_some_and(|at| at > book.observed_ms || section.max_age_ms.is_none())
            {
                return None;
            }
        }
    }
    let online = book.observed_ms <= now && now - book.observed_ms <= book.lease_ms;
    let mut result = serde_json::to_value(book).ok()?;
    result["online"] = json!(online);
    Some(result)
}

fn private(info: &std::fs::Metadata, mode: u32) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        info.permissions().mode() & 0o777 == mode
    }
    #[cfg(not(unix))]
    {
        let _ = (info, mode);
        true
    }
}

pub async fn status(root: Option<&Path>, now: u64) -> Value {
    let mut result = json!({"schema_version":1,"execution_authority":false,
        "online":false,"available":true,"refresh_ms":5000,"rejected_count":0,"books":[]});
    let Some(root) = root else {
        return result;
    };
    let directory = root.join("research-views");
    let info = match tokio::fs::symlink_metadata(&directory).await {
        Ok(info) => info,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return result,
        Err(_) => {
            result["available"] = json!(false);
            return result;
        }
    };
    if !info.is_dir() || !private(&info, 0o700) {
        result["available"] = json!(false);
        return result;
    }
    let Ok(mut entries) = tokio::fs::read_dir(directory).await else {
        result["available"] = json!(false);
        return result;
    };
    let mut paths = Vec::new();
    loop {
        match entries.next_entry().await {
            Ok(Some(entry)) => {
                let name = entry.file_name();
                if name
                    .to_str()
                    .is_some_and(|s| s.ends_with(".json") && !s.starts_with('.'))
                {
                    paths.push(entry.path());
                    if paths.len() > 32 {
                        result["available"] = json!(false);
                        return result;
                    }
                }
            }
            Ok(None) => break,
            Err(_) => {
                result["available"] = json!(false);
                return result;
            }
        }
    }
    paths.sort();
    let mut books = Vec::new();
    let mut rejected = 0;
    for path in paths {
        let loaded = async {
            let info = tokio::fs::symlink_metadata(&path).await.ok()?;
            if !info.is_file() || !private(&info, 0o600) || info.len() > 1_048_576 {
                return None;
            }
            let wire = tokio::fs::read(&path).await.ok()?;
            if wire.len() > 1_048_576 {
                return None;
            }
            project(
                serde_json::from_slice(&wire).ok()?,
                path.file_name()?.to_str()?,
                now,
            )
        }
        .await;
        match loaded {
            Some(book) => books.push(book),
            None => rejected += 1,
        }
    }
    result["online"] = json!(books.iter().any(|b| b["online"] == true));
    result["books"] = json!(books);
    result["rejected_count"] = json!(rejected);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Value {
        serde_json::from_str(include_str!(
            "../../../../tests/fixtures/research-view.json"
        ))
        .unwrap()
    }
    #[test]
    fn accepts_python_contract_and_derives_liveness() {
        let at = fixture()["observed_ms"].as_u64().unwrap();
        assert_eq!(
            project(fixture(), "rates-study.json", at).unwrap()["online"],
            true
        );
        assert_eq!(
            project(fixture(), "rates-study.json", at + 15001).unwrap()["online"],
            false
        );
        assert_eq!(
            project(fixture(), "rates-study.json", at - 1).unwrap()["online"],
            false
        );
        assert!(project(fixture(), "other.json", at).is_none());
    }
    #[test]
    fn rejects_unknown_renderers_authority_and_nested_private_data() {
        for (pointer, value) in [
            ("/execution_authority", json!(true)),
            ("/sections/0/kind", json!("remote-script")),
            ("/sections/0/fields/0/format", json!("html")),
            (
                "/sections/0/rows/0/values/rate",
                json!({"access_token":"secret"}),
            ),
            ("/sections/0/fields/0/scale", json!(0)),
            ("/id", json!("../secret")),
        ] {
            let mut data = fixture();
            *data.pointer_mut(pointer).unwrap() = value;
            assert!(
                project(data, "rates-study.json", u64::MAX).is_none(),
                "{pointer}"
            );
        }
        let mut data = fixture();
        data["access_token"] = json!("secret");
        assert!(project(data, "rates-study.json", u64::MAX).is_none());
    }
    #[tokio::test]
    async fn discovers_removes_and_isolates_invalid_registrations() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let root = std::env::temp_dir().join(format!("helios-views-{}", uuid::Uuid::new_v4()));
        let dir = root.join("research-views");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = dir.join("rates-study.json");
        std::fs::write(&path, fixture().to_string()).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let at = fixture()["observed_ms"].as_u64().unwrap();
        assert_eq!(
            status(Some(&root), at).await["books"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        symlink(&path, dir.join("alias.json")).unwrap();
        let data = status(Some(&root), at).await;
        assert_eq!(data["books"].as_array().unwrap().len(), 1);
        assert_eq!(data["rejected_count"], 1);
        std::fs::remove_file(&path).unwrap();
        assert!(status(Some(&root), at).await["books"]
            .as_array()
            .unwrap()
            .is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }
}
