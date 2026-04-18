use serde::Serialize;
use sha2::{Digest, Sha256};

/// Canonical JSON payload hashed for [`crate::BacktestReport::fingerprint_hex`].
#[derive(Debug, Clone, Serialize)]
pub struct PipelineFingerprintInput<'a> {
    pub pipeline_id: &'a str,
    pub pipeline_version: &'a str,
    pub range: crate::EpochRange,
    pub strategy_digest_hex: &'a str,
    pub clock_mode: &'a str,
    pub clock_anchor_epoch_sec: i64,
    pub extra: serde_json::Value,
}

/// Stable SHA-256 over canonical JSON (`serde_json::to_vec` with sorted map keys where applicable).
pub fn fingerprint_hex(input: &PipelineFingerprintInput<'_>) -> String {
    let body = serde_json::to_vec(input).expect("fingerprint json");
    let h = Sha256::digest(&body);
    hex::encode(h)
}
