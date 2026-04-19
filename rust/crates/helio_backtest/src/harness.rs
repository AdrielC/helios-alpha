use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::Instant;

use helio_scan::{Scan, SnapshottingScan};
use crate::clock::*;
use crate::fingerprint::*;
use crate::kalman::{
    fit_local_level_em, fit_local_level_mle, run_kalman_local_level, KalmanLocalLevelScan,
    LocalLevelEmOptions, LocalLevelMleOptions,
};
use crate::kalman_options::{KalmanFitMode, KalmanHarnessOptions};
use crate::metrics::sharpe_annualized_daily;
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
    #[serde(default)]
    pub kalman: KalmanHarnessOptions,
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
    /// Wall time to execute the backtest body (fingerprint + bar loop), seconds.
    pub run_wall_secs: f64,
    /// Sum of per-period toy **simple returns** in the demo path (same units as each daily `w`).
    pub pnl_simple: f64,
    /// Annualized Sharpe from **daily** simple returns using `sqrt(252)` scaling; `None` if
    /// fewer than two days or zero sample volatility.
    pub sharpe_daily_annualized: Option<f64>,
    /// When [`KalmanHarnessOptions::enabled`] was true: fitted `q`/`r`, last filtered level, sum of squared innovations.
    #[serde(default)]
    pub kalman: Option<KalmanHarnessSummary>,
}

/// Summary statistics from the optional Kalman pass (see [`BacktestRunSpec::kalman`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KalmanHarnessSummary {
    pub fit_mode: KalmanFitMode,
    pub q: f64,
    pub r: f64,
    /// Number of points used to fit `(q, r)`.
    pub mle_fit_n: usize,
    pub last_x_hat: f64,
    pub innovation_energy: f64,
    /// `0.5 * Σ (ln S_t + ν²/S_t)` on the **full** series at fitted `(q, r)` (same objective as MLE).
    pub neg_loglik: f64,
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
        let kalman_json = serde_json::to_value(&spec.kalman).unwrap_or(json!({}));
        let input = PipelineFingerprintInput {
            pipeline_id: &spec.pipeline.id,
            pipeline_version: &spec.pipeline.version,
            range: spec.range,
            strategy_digest_hex: &spec.strategy.digest_hex,
            clock_mode,
            clock_anchor_epoch_sec: clock_anchor,
            kalman: kalman_json.clone(),
            extra: spec.fingerprint_extra.clone(),
        };
        let t0 = Instant::now();
        let fingerprint_hex = fingerprint_hex(&input);

        // Deterministic toy stream: one bar per UTC calendar day in range.
        let n_days = spec.range.span_secs() / 86_400 + 1;
        let bars_processed = n_days;
        let mut daily: Vec<f64> = Vec::with_capacity(n_days as usize);
        let mut acc = 0.0f64;
        for i in 0..n_days {
            let t = spec.range.start_epoch_sec + (i as i64) * 86_400;
            let w = ((t % 13) as f64) * 1e-4;
            daily.push(w);
            acc += w;
        }
        let sharpe_daily_annualized = sharpe_annualized_daily(&daily);

        let kalman = if spec.kalman.enabled {
            let cap = spec.kalman.train_prefix_cap.unwrap_or(50_000).max(3);
            let mle_fit_n = (n_days as usize).min(cap);
            let fit_slice = &daily[..mle_fit_n];
            let cfg = match spec.kalman.fit_mode {
                KalmanFitMode::Em => fit_local_level_em(fit_slice, LocalLevelEmOptions::default()),
                KalmanFitMode::Mle => fit_local_level_mle(fit_slice, LocalLevelMleOptions::default()),
            };
            let (outs, _st) = run_kalman_local_level(cfg, &daily);
            let last = outs.last().expect("n_days >= 1");
            let innovation_energy: f64 = outs.iter().map(|o| o.innovation * o.innovation).sum();
            let neg_loglik = crate::kalman::innovation_neg_loglik(&daily, cfg.q, cfg.r)
                .unwrap_or(f64::NAN);

            if spec.kalman.verify_snapshot_resume {
                let nd = n_days as usize;
                if nd >= 2 {
                    let scan = KalmanLocalLevelScan::new(cfg);
                    let split = (nd / 2).clamp(1, nd.saturating_sub(1));
                    let mut st_a = scan.init();
                    let mut e_a = helio_scan::VecEmitter::new();
                    helio_scan::run_slice(&scan, &mut st_a, &daily[..split], &mut e_a);
                    let snap = scan.snapshot(&st_a);
                    let mut st_b = scan.restore(snap);
                    helio_scan::run_slice(&scan, &mut st_b, &daily[split..], &mut e_a);
                    let split_out = e_a.into_inner();
                    assert_eq!(split_out.len(), outs.len());
                    for (a, b) in outs.iter().zip(split_out.iter()) {
                        assert!((a.x_hat - b.x_hat).abs() < 1e-9, "kalman snapshot drift");
                    }
                }
            }

            Some(KalmanHarnessSummary {
                fit_mode: spec.kalman.fit_mode,
                q: cfg.q,
                r: cfg.r,
                mle_fit_n,
                last_x_hat: last.x_hat,
                innovation_energy,
                neg_loglik,
            })
        } else {
            None
        };

        let run_wall_secs = t0.elapsed().as_secs_f64();

        Ok(BacktestReport {
            pipeline_id: spec.pipeline.id.clone(),
            pipeline_version: spec.pipeline.version.clone(),
            range: spec.range,
            strategy_digest_hex: spec.strategy.digest_hex.clone(),
            fingerprint_hex,
            clock_mode: clock_mode.to_string(),
            clock_now_epoch_sec: clock_anchor,
            bars_processed,
            run_wall_secs,
            pnl_simple: acc,
            sharpe_daily_annualized,
            kalman,
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
        // ~20y of daily toy observations (heavy default for throughput demos).
        range: EpochRange::new(0, 20 * 365 * 86_400).expect("demo range"),
        strategy: StrategySpec {
            digest_hex: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
                .into(),
        },
        kalman: KalmanHarnessOptions {
            enabled: true,
            fit_mode: KalmanFitMode::Em,
            train_prefix_cap: Some(50_000),
            verify_snapshot_resume: false,
        },
        fingerprint_extra: json!({"venue": "XNYS"}),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

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
        assert!(r.run_wall_secs >= 0.0);
        assert!(r.sharpe_daily_annualized.is_some());
    }

    #[test]
    fn twenty_year_demo_with_kalman_finishes_quickly() {
        let h = BacktestHarness::fixed(0);
        let spec = demo_run_spec();
        let t0 = Instant::now();
        let r = h.run(&spec).unwrap();
        let elapsed = t0.elapsed();
        assert_eq!(r.bars_processed, 20 * 365 + 1);
        assert!(r.kalman.is_some(), "kalman should run by default on demo spec");
        assert!(
            elapsed.as_secs() < 5,
            "20y + kalman took {:?} (expected < 5s in CI)",
            elapsed
        );
    }

    #[test]
    fn kalman_snapshot_resume_matches_in_harness() {
        let h = BacktestHarness::fixed(0);
        let mut spec = demo_run_spec();
        spec.range = EpochRange::new(0, 5_000 * 86_400).expect("range");
        spec.kalman.verify_snapshot_resume = true;
        h.run(&spec).expect("verify path");
    }
}
