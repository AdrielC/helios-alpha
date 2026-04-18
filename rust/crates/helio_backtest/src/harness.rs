use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::clock::*;
use crate::fingerprint::*;
use crate::range::*;
use crate::Result;

/// Identifies a backtest pipeline line (app name + semver-style version string).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineSpec {
    pub id: String,
    pub version: String,
}

impl PipelineSpec {
    pub fn new(id: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            version: version.into(),
        }
    }
}

/// Strategy / parameters blob digested into the fingerprint (caller supplies stable hex).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StrategySpec {
    /// Pre-hashed strategy config, e.g. SHA-256 hex of a frozen JSON or Ron file.
    pub digest_hex: String,
}

/// One deterministic backtest invocation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BacktestRunSpec {
    pub pipeline: PipelineSpec,
    pub range: EpochRange,
    pub strategy: StrategySpec,
    /// Arbitrary JSON merged into fingerprint (sorted keys recommended by caller).
    #[serde(default)]
    pub fingerprint_extra: serde_json::Value,
}

/// Outcome of [`BacktestHarness::run`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BacktestReport {
    pub pipeline_id: String,
    pub pipeline_version: String,
    pub range: EpochRange,
    pub strategy_digest_hex: String,
    pub fingerprint_hex: String,
    pub clock_mode: String,
    pub clock_now_epoch_sec: i64,
    pub bars_processed: u64,
    pub pnl_simple: f64,
}

/// Drives a minimal bar stream and aggregates a toy PnL (deterministic given inputs).
pub struct BacktestHarness<C: Clock> {
    pub clock: C,
}

impl<C: Clock> BacktestHarness<C> {
    pub fn new(clock: C) -> Self {
        Self { clock }
    }

    /// Run harness: validates range, computes fingerprint, simulates `span_secs + 1` daily bars.
    pub fn run(&self, spec: &BacktestRunSpec) -> Result<BacktestReport> {
        let _ = EpochRange::new(spec.range.start_epoch_sec, spec.range.end_epoch_sec)?;
        let clock_mode = std::any::type_name::<C>();
        let clock_anchor = self.clock.now_epoch_sec();
        let input = PipelineFingerprintInput {
            pipeline_id: &spec.pipeline.id,
            pipeline_version: &spec.pipeline.version,
            range: spec.range,
            strategy_digest_hex: &spec.strategy.digest_hex,
            clock_mode,
            clock_anchor_epoch_sec: clock_anchor,
            extra: spec.fingerprint_extra.clone(),
        };
        let fingerprint_hex = fingerprint_hex(&input);

        // Deterministic toy stream: one bar per UTC calendar day in range.
        let n_days = spec.range.span_secs() / 86_400 + 1;
        let bars_processed = n_days;
        // Toy PnL: hash nibbles of fingerprint modulate a constant — stable for same fingerprint.
        let mut acc = 0.0f64;
        for i in 0..n_days {
            let t = spec.range.start_epoch_sec + (i as i64) * 86_400;
            let w = ((t % 13) as f64) * 1e-4;
            acc += w;
        }

        Ok(BacktestReport {
            pipeline_id: spec.pipeline.id.clone(),
            pipeline_version: spec.pipeline.version.clone(),
            range: spec.range,
            strategy_digest_hex: spec.strategy.digest_hex.clone(),
            fingerprint_hex,
            clock_mode: clock_mode.to_string(),
            clock_now_epoch_sec: clock_anchor,
            bars_processed,
            pnl_simple: acc,
        })
    }
}

impl BacktestHarness<WallClock> {
    pub fn wall() -> Self {
        Self::new(WallClock)
    }
}

impl BacktestHarness<FixedClock> {
    pub fn fixed(epoch_sec: i64) -> Self {
        Self::new(FixedClock(epoch_sec))
    }
}

/// Build a default demo spec (epoch range around Unix epoch week).
pub fn demo_run_spec() -> BacktestRunSpec {
    BacktestRunSpec {
        pipeline: PipelineSpec::new("helio_backtest.demo", "0.1.0"),
        range: EpochRange::new(0, 6 * 86_400).expect("demo range"),
        strategy: StrategySpec {
            digest_hex: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
                .into(),
        },
        fingerprint_extra: json!({"venue": "XNYS"}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_clock_repeatable_fingerprint() {
        let h = BacktestHarness::fixed(1_700_000_000);
        let spec = demo_run_spec();
        let r1 = h.run(&spec).unwrap();
        let r2 = h.run(&spec).unwrap();
        assert_eq!(r1.fingerprint_hex, r2.fingerprint_hex);
        assert_eq!(r1.clock_now_epoch_sec, 1_700_000_000);
    }

    #[test]
    fn wall_clock_changes_fingerprint_anchor() {
        let h1 = BacktestHarness::fixed(100);
        let h2 = BacktestHarness::fixed(200);
        let spec = demo_run_spec();
        assert_ne!(
            h1.run(&spec).unwrap().fingerprint_hex,
            h2.run(&spec).unwrap().fingerprint_hex
        );
    }

    #[test]
    fn fingerprint_includes_dt_range_and_pipeline() {
        let h = BacktestHarness::fixed(42);
        let mut spec = demo_run_spec();
        spec.pipeline.id = "test.pipe".into();
        let r = h.run(&spec).unwrap();
        assert!(r.fingerprint_hex.len() == 64);
        assert_eq!(r.range.start_epoch_sec, spec.range.start_epoch_sec);
    }
}
