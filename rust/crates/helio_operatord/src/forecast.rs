use crate::types::{ForecastBundle, ForecastInputRequirement, ForecastState};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use thiserror::Error;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ForecastBundleManifest {
    schema_version: u8,
    bundle_version: u32,
    id: String,
    label: String,
    thesis: String,
    horizon: String,
    state: ForecastState,
    strategy_ids: Vec<String>,
    series_ids: Vec<String>,
    shared_series_ids: Vec<String>,
    input_contract: Vec<ForecastInputRequirement>,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ForecastBundleError {
    #[error("forecast bundle is not valid JSON: {0}")]
    InvalidJson(String),
    #[error("unsupported forecast bundle schema {0}")]
    UnsupportedSchema(u8),
    #[error("bundleVersion must be positive")]
    InvalidVersion,
    #[error("{0} must not be empty")]
    EmptyField(&'static str),
    #[error("{0} contains duplicates")]
    DuplicateValues(&'static str),
    #[error("sharedSeriesIds must be a subset of seriesIds")]
    InvalidSharedSeries,
    #[error("inputContract must exactly match seriesIds order")]
    InputOrderMismatch,
    #[error("inputContract must contain at least one required input")]
    NoRequiredInput,
    #[error("inputContract maxAgeSeconds must be positive")]
    InvalidMaxAge,
}

impl ForecastBundle {
    pub fn from_manifest_bytes(raw: &[u8]) -> Result<Self, ForecastBundleError> {
        let manifest: ForecastBundleManifest = serde_json::from_slice(raw)
            .map_err(|error| ForecastBundleError::InvalidJson(error.to_string()))?;
        validate(&manifest)?;
        Ok(Self {
            schema_version: manifest.schema_version,
            bundle_version: manifest.bundle_version,
            definition_sha256: hex::encode(Sha256::digest(raw)),
            id: manifest.id,
            label: manifest.label,
            thesis: manifest.thesis,
            horizon: manifest.horizon,
            state: manifest.state,
            strategy_ids: manifest.strategy_ids,
            series_ids: manifest.series_ids,
            shared_series_ids: manifest.shared_series_ids,
            input_contract: manifest.input_contract,
        })
    }
}

fn validate(manifest: &ForecastBundleManifest) -> Result<(), ForecastBundleError> {
    if manifest.schema_version != 1 {
        return Err(ForecastBundleError::UnsupportedSchema(
            manifest.schema_version,
        ));
    }
    if manifest.bundle_version == 0 {
        return Err(ForecastBundleError::InvalidVersion);
    }
    for (value, field) in [
        (&manifest.id, "id"),
        (&manifest.label, "label"),
        (&manifest.thesis, "thesis"),
        (&manifest.horizon, "horizon"),
    ] {
        if value.trim().is_empty() {
            return Err(ForecastBundleError::EmptyField(field));
        }
    }
    validate_text_set(&manifest.strategy_ids, "strategyIds")?;
    validate_text_set(&manifest.series_ids, "seriesIds")?;
    validate_text_set(&manifest.shared_series_ids, "sharedSeriesIds")?;

    let series: HashSet<_> = manifest.series_ids.iter().collect();
    if manifest
        .shared_series_ids
        .iter()
        .any(|id| !series.contains(id))
    {
        return Err(ForecastBundleError::InvalidSharedSeries);
    }
    let input_ids: Vec<_> = manifest
        .input_contract
        .iter()
        .map(|input| &input.series_id)
        .collect();
    if input_ids != manifest.series_ids.iter().collect::<Vec<_>>() {
        return Err(ForecastBundleError::InputOrderMismatch);
    }
    if !manifest.input_contract.iter().any(|input| input.required) {
        return Err(ForecastBundleError::NoRequiredInput);
    }
    for input in &manifest.input_contract {
        if input.series_id.trim().is_empty() {
            return Err(ForecastBundleError::EmptyField("inputContract.seriesId"));
        }
        if input.role.trim().is_empty() {
            return Err(ForecastBundleError::EmptyField("inputContract.role"));
        }
        if input.max_age_seconds == 0 {
            return Err(ForecastBundleError::InvalidMaxAge);
        }
        validate_text_set(&input.source_ids, "inputContract.sourceIds")?;
    }
    Ok(())
}

fn validate_text_set(values: &[String], field: &'static str) -> Result<(), ForecastBundleError> {
    if values.iter().any(|value| value.trim().is_empty()) {
        return Err(ForecastBundleError::EmptyField(field));
    }
    let unique: HashSet<_> = values.iter().collect();
    if unique.len() != values.len() {
        return Err(ForecastBundleError::DuplicateValues(field));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &[u8] = br#"{
        "schemaVersion":1,
        "bundleVersion":2,
        "id":"forecast",
        "label":"Forecast",
        "thesis":"A causal hypothesis",
        "horizon":"minutes",
        "state":"monitoring",
        "strategyIds":[],
        "seriesIds":["source"],
        "sharedSeriesIds":[],
        "inputContract":[{
            "seriesId":"source",
            "role":"trigger",
            "required":true,
            "maxAgeSeconds":30,
            "sourceIds":["source-v1"]
        }]
    }"#;

    #[test]
    fn manifest_is_validated_and_fingerprinted() {
        let bundle = ForecastBundle::from_manifest_bytes(VALID).unwrap();
        assert_eq!(bundle.schema_version, 1);
        assert_eq!(bundle.bundle_version, 2);
        assert_eq!(bundle.definition_sha256, hex::encode(Sha256::digest(VALID)));
    }

    #[test]
    fn input_order_drift_is_rejected() {
        let invalid = String::from_utf8(VALID.to_vec())
            .unwrap()
            .replace("\"seriesIds\":[\"source\"]", "\"seriesIds\":[\"other\"]");
        assert_eq!(
            ForecastBundle::from_manifest_bytes(invalid.as_bytes()).unwrap_err(),
            ForecastBundleError::InputOrderMismatch
        );
    }
}
