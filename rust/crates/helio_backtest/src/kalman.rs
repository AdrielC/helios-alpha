//! Local-level (random-walk) **1D Kalman filter** as a [`helio_scan::Scan`] + [`helio_scan::SnapshottingScan`]:
//! O(1) per step, **pause/restartable** via snapshot, **serde** for config and snapshots, composable
//! with `then` / `run_slice` like any other scan.
//!
//! Parameter fitting:
//! - [`fit_local_level_em`] — **full EM** for scalar `(q, r)`: E-step = Kalman **filter** + **RTS
//!   smoother** (smoothed states + lag-one covariance); M-step = closed-form updates from
//!   smoothed moments.
//! - [`fit_local_level_mle`] — faster **coordinate-wise MLE** on innovation likelihood (no smoother).
//!
//! ## Composition with `helio_scan`
//!
//! **[`KalmanLocalLevelScan`]** implements [`Scan`], [`FlushableScan`], [`SnapshottingScan`]: it
//! composes with **`then`**, **`and`**, **`run_slice`**, checkpoint wrappers, etc., like any other
//! scan. **EM is offline** on a `&[f64]` slice (`fit_local_level_em`): it is not itself a `Scan`, but
//! it **outputs** a [`KalmanLocalLevelConfig`] you plug into [`KalmanLocalLevelScan::new`] for
//! streaming / composable runtime filtering.

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

/// Forward pass matching [`KalmanLocalLevelScan`]: stores filtered moments and per-step innovation stats.
#[derive(Debug, Clone)]
struct LocalLevelForward {
    /// Filtered mean `x_{t|t}` and variance `P_{t|t}` after `y[t]`.
    pub xf: Vec<f64>,
    pub pf: Vec<f64>,
    /// Predicted variance `P_{t+1|t} = P_{t|t} + q` after processing `y[t]` (needed for RTS `J_t`).
    pub p_pred_tp1: Vec<f64>,
    /// Innovation variance `S_t` when updating with `y[t]`.
    pub s: Vec<f64>,
    /// Innovation `ν_t` at update with `y[t]`.
    pub nu: Vec<f64>,
}

fn forward_filter_local_level(cfg: KalmanLocalLevelConfig, y: &[f64]) -> Option<LocalLevelForward> {
    let n = y.len();
    if n == 0 || !(cfg.q > 0.0 && cfg.r > 0.0) {
        return None;
    }
    let mut xf = Vec::with_capacity(n);
    let mut pf = Vec::with_capacity(n);
    let mut p_pred_tp1 = Vec::with_capacity(n);
    let mut s = Vec::with_capacity(n);
    let mut nu = Vec::with_capacity(n);

    let mut x = cfg.x_init;
    let mut p = cfg.p_init;
    for t in 0..n {
        let p_prior = p + cfg.q;
        let st = p_prior + cfg.r;
        if !(st > 0.0 && st.is_finite()) {
            return None;
        }
        let innov = y[t] - x;
        let k = p_prior / st;
        let x_new = x + k * innov;
        let p_new = (1.0 - k).max(0.0) * p_prior;
        p_pred_tp1.push(p_new + cfg.q);
        s.push(st);
        nu.push(innov);
        xf.push(x_new);
        pf.push(p_new);
        x = x_new;
        p = p_new;
    }
    Some(LocalLevelForward {
        xf,
        pf,
        p_pred_tp1,
        s,
        nu,
    })
}

/// Half the **negative** log-likelihood of innovations (matches [`KalmanLocalLevelScan`] forward pass).
pub fn innovation_neg_loglik(y: &[f64], q: f64, r: f64) -> Option<f64> {
    let cfg = KalmanLocalLevelConfig {
        q,
        r,
        x_init: y.first().copied().unwrap_or(0.0),
        p_init: r.max(1e-12),
    };
    let f = forward_filter_local_level(cfg, y)?;
    let mut ll = 0.0f64;
    for t in 0..y.len() {
        ll += f.s[t].ln() + f.nu[t] * f.nu[t] / f.s[t];
    }
    Some(0.5 * ll)
}

/// RTS smoother: returns smoothed means `xs`, variances `Ps`, and lag-one cross-covariance
/// `cov_lm1_t[t] = Cov(x_{t-1}, x_t | Y)` for `t >= 1` (index 0 unused).
fn rts_smooth_local_level(fwd: &LocalLevelForward) -> Option<(Vec<f64>, Vec<f64>, Vec<f64>)> {
    let n = fwd.xf.len();
    if n == 0 {
        return None;
    }
    let mut xs = fwd.xf.clone();
    let mut ps = fwd.pf.clone();
    let mut cov_lm1_t = vec![0.0; n];

    if n == 1 {
        return Some((xs, ps, cov_lm1_t));
    }

    // Backward: xs[t] = xf[t] + J_t (xs[t+1] - xf[t]) with J_t = P_{t|t} / P_{t+1|t}, x_{t+1|t}=xf[t].
    for t in (0..n - 1).rev() {
        let denom = fwd.p_pred_tp1[t];
        if !(denom > 1e-18 && denom.is_finite()) {
            return None;
        }
        let j = fwd.pf[t] / denom;
        let dx = xs[t + 1] - fwd.xf[t];
        xs[t] = fwd.xf[t] + j * dx;
        let dp = ps[t + 1] - denom;
        ps[t] = fwd.pf[t] + j * j * dp;
    }

    for t in 1..n {
        let denom = fwd.p_pred_tp1[t - 1];
        if !(denom > 1e-18 && denom.is_finite()) {
            return None;
        }
        let j = fwd.pf[t - 1] / denom;
        cov_lm1_t[t] = j * ps[t];
    }

    Some((xs, ps, cov_lm1_t))
}

fn em_mstep_local_level(
    y: &[f64],
    xs: &[f64],
    ps: &[f64],
    cov_lm1_t: &[f64],
) -> Option<(f64, f64)> {
    let n = y.len();
    if n < 2 || xs.len() != n || ps.len() != n || cov_lm1_t.len() != n {
        return None;
    }
    let mut sum_diff2 = 0.0f64;
    for t in 1..n {
        let ex2 = ps[t] + xs[t] * xs[t];
        let exm12 = ps[t - 1] + xs[t - 1] * xs[t - 1];
        let c = cov_lm1_t[t];
        sum_diff2 += ex2 + exm12 - 2.0 * c - 2.0 * xs[t - 1] * xs[t];
    }
    let q = (sum_diff2 / (n - 1) as f64).max(1e-30);

    let mut sum_obs2 = 0.0f64;
    for t in 0..n {
        let res = y[t] - xs[t];
        sum_obs2 += res * res + ps[t];
    }
    let r = (sum_obs2 / n as f64).max(1e-30);
    Some((q, r))
}

/// Options for [`fit_local_level_em`].
#[derive(Debug, Clone, Copy)]
pub struct LocalLevelEmOptions {
    pub max_iters: usize,
    pub tol_q_rel: f64,
    pub tol_r_rel: f64,
    /// Clamp for numerical stability.
    pub q_max: f64,
    pub r_max: f64,
}

impl Default for LocalLevelEmOptions {
    fn default() -> Self {
        Self {
            max_iters: 80,
            tol_q_rel: 1e-6,
            tol_r_rel: 1e-6,
            q_max: 1e6,
            r_max: 1e6,
        }
    }
}

/// **EM** for scalar local-level `(q, r)`: repeated **E-step** (Kalman filter + RTS smoother) and
/// **M-step** (closed-form `q`, `r` from smoothed second moments).
///
/// Initializes from [`fit_local_level_mle`] then iterates EM. `x_init = y[0]`, `p_init = r` each outer restart.
pub fn fit_local_level_em(y: &[f64], em_opts: LocalLevelEmOptions) -> KalmanLocalLevelConfig {
    let n = y.len();
    if n < 3 {
        return KalmanLocalLevelConfig::default();
    }
    let mut cfg = fit_local_level_mle(y, LocalLevelMleOptions::default());
    for _ in 0..em_opts.max_iters {
        let q0 = cfg.q;
        let r0 = cfg.r;
        let fwd = match forward_filter_local_level(cfg, y) {
            Some(f) => f,
            None => break,
        };
        let (xs, ps, cov) = match rts_smooth_local_level(&fwd) {
            Some(x) => x,
            None => break,
        };
        let Some((q_new, r_new)) = em_mstep_local_level(y, &xs, &ps, &cov) else {
            break;
        };
        cfg.q = q_new.min(em_opts.q_max);
        cfg.r = r_new.min(em_opts.r_max);
        cfg.x_init = y[0];
        cfg.p_init = cfg.r.max(1e-12);
        let dq = (cfg.q - q0).abs() / q0.max(1e-30);
        let dr = (cfg.r - r0).abs() / r0.max(1e-30);
        if dq < em_opts.tol_q_rel && dr < em_opts.tol_r_rel {
            break;
        }
    }
    cfg
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

    #[test]
    fn em_fit_runs_and_filter_matches_scan() {
        let y: Vec<f64> = (0..500).map(|i| (i as f64).sin() * 0.03).collect();
        let cfg = fit_local_level_em(&y, LocalLevelEmOptions::default());
        assert!(cfg.q > 0.0 && cfg.r > 0.0);
        let (outs, _) = run_kalman_local_level(cfg, &y);
        let scan = KalmanLocalLevelScan::new(cfg);
        let mut st = scan.init();
        let mut e = VecEmitter::new();
        helio_scan::run_slice(&scan, &mut st, &y, &mut e);
        let v2 = e.into_inner();
        assert_eq!(outs.len(), v2.len());
        for (a, b) in outs.iter().zip(v2.iter()) {
            assert!((a.x_hat - b.x_hat).abs() < 1e-12);
        }
    }
}
