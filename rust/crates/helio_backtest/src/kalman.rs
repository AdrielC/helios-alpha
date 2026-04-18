//! Local-level (random-walk) **1D Kalman filter** as a [`helio_scan::Scan`] + [`helio_scan::SnapshottingScan`]:
//! O(1) per step, **pause/restartable** via snapshot, **serde** for config and snapshots, composable
//! with `then` / `run_slice` like any other scan.
//!
//! Parameter fitting: [`fit_local_level_mle`] maximizes the **Gaussian innovation log-likelihood**
//! (exact for this linear Gaussian model) by alternating 1D minimization on `ln(q)` and `ln(r)`
//! with golden-section search — no variance-split heuristics.

use helio_scan::{
    Emit, FlushReason, FlushableScan, Scan, SnapshottingScan, VersionedSnapshot, VecEmitter,
};
use serde::{Deserialize, Serialize};

/// Static parameters for the local-level model `x_t = x_{t-1} + w_t`, `y_t = x_t + v_t`,
/// `Var(w)=q`, `Var(v)=r`, with initial `x_0 ~ (x_init, p_init)`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct KalmanLocalLevelConfig {
    pub q: f64,
    pub r: f64,
    pub x_init: f64,
    pub p_init: f64,
}

impl Default for KalmanLocalLevelConfig {
    fn default() -> Self {
        Self {
            q: 1e-8,
            r: 1e-4,
            x_init: 0.0,
            p_init: 1.0,
        }
    }
}

/// Serializable filter state after any number of [`KalmanLocalLevelScan::step`] calls.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct KalmanLocalLevelState {
    /// Posterior mean of the level after the last processed observation.
    pub x: f64,
    /// Posterior variance of the level.
    pub p: f64,
}

/// Same as [`KalmanLocalLevelState`] for snapshots (versioned for persistence).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct KalmanLocalLevelSnapshot {
    pub x: f64,
    pub p: f64,
}

impl VersionedSnapshot for KalmanLocalLevelSnapshot {
    const VERSION: u32 = 1;
}

/// One emission per input observation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct KalmanOutput {
    pub y: f64,
    /// Posterior level estimate after incorporating `y`.
    pub x_hat: f64,
    pub innovation: f64,
    /// Kalman gain used on this step.
    pub k: f64,
}

/// 1D local-level Kalman filter scan (identity “model”; all parameters in [`KalmanLocalLevelConfig`]).
#[derive(Debug, Clone, Copy)]
pub struct KalmanLocalLevelScan {
    pub cfg: KalmanLocalLevelConfig,
}

impl KalmanLocalLevelScan {
    pub fn new(cfg: KalmanLocalLevelConfig) -> Self {
        Self { cfg }
    }
}

impl Scan for KalmanLocalLevelScan {
    type In = f64;
    type Out = KalmanOutput;
    type State = KalmanLocalLevelState;

    fn init(&self) -> Self::State {
        KalmanLocalLevelState {
            x: self.cfg.x_init,
            p: self.cfg.p_init,
        }
    }

    fn step<E>(&self, st: &mut Self::State, y: Self::In, emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
        let q = self.cfg.q;
        let r = self.cfg.r;
        let x_pred = st.x;
        let p_prior = st.p + q;
        let s = p_prior + r;
        let k = if s > 0.0 && s.is_finite() {
            p_prior / s
        } else {
            0.0
        };
        let innov = y - x_pred;
        let x_post = x_pred + k * innov;
        let p_post = (1.0 - k).max(0.0) * p_prior;
        st.x = x_post;
        st.p = p_post;
        emit.emit(KalmanOutput {
            y,
            x_hat: x_post,
            innovation: innov,
            k,
        });
    }
}

impl FlushableScan for KalmanLocalLevelScan {
    type Offset = u64;

    fn flush<E>(&self, _st: &mut Self::State, _signal: FlushReason<Self::Offset>, _emit: &mut E)
    where
        E: Emit<Self::Out>,
    {
    }
}

impl SnapshottingScan for KalmanLocalLevelScan {
    type Snapshot = KalmanLocalLevelSnapshot;

    fn snapshot(&self, state: &Self::State) -> Self::Snapshot {
        KalmanLocalLevelSnapshot {
            x: state.x,
            p: state.p,
        }
    }

    fn restore(&self, snap: Self::Snapshot) -> Self::State {
        KalmanLocalLevelState {
            x: snap.x,
            p: snap.p,
        }
    }
}

/// Half the **negative** log-likelihood of innovations under the Kalman filter (up to `const + n*ln(2π)/2`):
/// `0.5 * Σ_t ( ln(S_t) + ν_t² / S_t )`. Minimizing this is equivalent to MLE for `(q, r)` given the filter.
pub fn innovation_neg_loglik(y: &[f64], q: f64, r: f64) -> Option<f64> {
    if y.is_empty() || !(q > 0.0 && r > 0.0 && q.is_finite() && r.is_finite()) {
        return None;
    }
    let mut x = y[0];
    let mut p = r.max(1e-12);
    let mut ll = 0.0f64;
    for &yi in y.iter().skip(1) {
        let p_prior = p + q;
        let s = p_prior + r;
        if !(s > 0.0 && s.is_finite()) {
            return None;
        }
        let nu = yi - x;
        ll += s.ln() + nu * nu / s;
        let k = p_prior / s;
        x = x + k * nu;
        p = (1.0 - k).max(0.0) * p_prior;
    }
    Some(0.5 * ll)
}

fn golden_section_minimize<F>(mut f: F, a: f64, b: f64, max_iter: usize) -> f64
where
    F: FnMut(f64) -> f64,
{
    let resphi = 2.0 - (1.0 + 5f64.sqrt()) * 0.5;
    let mut a = a;
    let mut b = b;
    let mut x1 = a + resphi * (b - a);
    let mut x2 = b - resphi * (b - a);
    let mut f1 = f(x1);
    let mut f2 = f(x2);
    for _ in 0..max_iter {
        if f1 > f2 {
            a = x1;
            x1 = x2;
            f1 = f2;
            x2 = b - resphi * (b - a);
            f2 = f(x2);
        } else {
            b = x2;
            x2 = x1;
            f2 = f1;
            x1 = a + resphi * (b - a);
            f1 = f(x1);
        }
        if (b - a).abs() < 1e-7 * (1.0 + a.abs() + b.abs()) {
            break;
        }
    }
    (a + b) * 0.5
}

/// Options for [`fit_local_level_mle`].
#[derive(Debug, Clone, Copy)]
pub struct LocalLevelMleOptions {
    /// Alternating passes over `(ln q, ln r)`.
    pub outer_iters: usize,
    /// Golden-section iterations per 1D pass.
    pub golden_iters: usize,
    pub log_q_lo: f64,
    pub log_q_hi: f64,
    pub log_r_lo: f64,
    pub log_r_hi: f64,
}

impl Default for LocalLevelMleOptions {
    fn default() -> Self {
        Self {
            outer_iters: 16,
            golden_iters: 40,
            log_q_lo: -28.0,
            log_q_hi: 4.0,
            log_r_lo: -28.0,
            log_r_hi: 4.0,
        }
    }
}

/// **MLE** for local-level `(q, r)` by minimizing innovation Gaussian negative log-likelihood,
/// alternating coordinate-wise golden-section search on `ln q` and `ln r`.
///
/// `x_init = y[0]`, `p_init = r` (diffuse on level relative to first observation noise).
pub fn fit_local_level_mle(y: &[f64], opts: LocalLevelMleOptions) -> KalmanLocalLevelConfig {
    let n = y.len();
    if n < 3 {
        return KalmanLocalLevelConfig::default();
    }
    let mut log_q = -12.0f64;
    let mut log_r = -10.0f64;
    for _ in 0..opts.outer_iters {
        log_q = golden_section_minimize(
            |lq| {
                innovation_neg_loglik(y, lq.exp(), log_r.exp()).unwrap_or(f64::INFINITY)
            },
            opts.log_q_lo,
            opts.log_q_hi,
            opts.golden_iters,
        );
        log_r = golden_section_minimize(
            |lr| {
                innovation_neg_loglik(y, log_q.exp(), lr.exp()).unwrap_or(f64::INFINITY)
            },
            opts.log_r_lo,
            opts.log_r_hi,
            opts.golden_iters,
        );
    }
    let q = log_q.exp().clamp(1e-30, 1e10);
    let r = log_r.exp().clamp(1e-30, 1e10);
    KalmanLocalLevelConfig {
        q,
        r,
        x_init: y[0],
        p_init: r.max(1e-12),
    }
}

/// Run filter on `y`, return outputs and final state (single pass).
pub fn run_kalman_local_level(
    cfg: KalmanLocalLevelConfig,
    y: &[f64],
) -> (Vec<KalmanOutput>, KalmanLocalLevelState) {
    let scan = KalmanLocalLevelScan::new(cfg);
    let mut st = scan.init();
    let mut e = VecEmitter::new();
    helio_scan::run_slice(&scan, &mut st, y, &mut e);
    (e.into_inner(), st)
}

#[cfg(test)]
mod tests {
    use helio_scan::Scan;

    use super::*;

    #[test]
    fn snapshot_resume_matches_continuous() {
        let y: Vec<f64> = (0..5000).map(|i| (i as f64).sin() * 0.01 + (i as f64) * 1e-5).collect();
        let cfg = fit_local_level_mle(&y, LocalLevelMleOptions::default());
        let scan = KalmanLocalLevelScan::new(cfg);

        let mut st1 = scan.init();
        let mut out1 = VecEmitter::new();
        helio_scan::run_slice(&scan, &mut st1, &y, &mut out1);
        let full = out1.into_inner();

        let split = 2400;
        let mut st2 = scan.init();
        let mut out2 = VecEmitter::new();
        helio_scan::run_slice(&scan, &mut st2, &y[..split], &mut out2);
        let snap = scan.snapshot(&st2);
        let mut st3 = scan.restore(snap);
        helio_scan::run_slice(&scan, &mut st3, &y[split..], &mut out2);
        let split_out = out2.into_inner();

        assert_eq!(full.len(), split_out.len());
        for (a, b) in full.iter().zip(split_out.iter()) {
            assert!((a.innovation - b.innovation).abs() < 1e-9);
            assert!((a.x_hat - b.x_hat).abs() < 1e-9);
        }
    }

    #[test]
    fn mle_lowers_neg_ll_vs_default() {
        let y: Vec<f64> = (0..2000)
            .map(|i| 0.5 * (i as f64) * 1e-4 + (i as f64).sin() * 0.02)
            .collect();
        let d = KalmanLocalLevelConfig::default();
        let n0 = innovation_neg_loglik(&y, d.q, d.r).expect("ll");
        let cfg = fit_local_level_mle(&y, LocalLevelMleOptions::default());
        let n1 = innovation_neg_loglik(&y, cfg.q, cfg.r).expect("ll");
        assert!(
            n1 < n0,
            "MLE should improve neg log-lik: default={n0:.4} mle={n1:.4} q={} r={}",
            cfg.q,
            cfg.r
        );
    }
}
