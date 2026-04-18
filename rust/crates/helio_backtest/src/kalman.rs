//! Local-level (random-walk) **1D Kalman filter** as a [`helio_scan::Scan`] + [`helio_scan::SnapshottingScan`]:
//! O(1) per step, **pause/restartable** via snapshot, **serde** for config and snapshots, composable
//! with `then` / `run_slice` like any other scan.

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
        // Pure filter: no flush emissions.
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

/// Fast **diagonal** heuristic on a prefix of observations (no iterative MLE).
///
/// Uses sample variance of first differences as a scale for process noise and allocates
/// measurement noise from residual variance vs. a crude signal split. Intended for **warm-start**
/// or backtest defaults on long streams; refine offline if you need MLE.
pub fn train_local_level_heuristic(y: &[f64]) -> KalmanLocalLevelConfig {
    let n = y.len();
    if n < 3 {
        return KalmanLocalLevelConfig::default();
    }
    let mean_y: f64 = y.iter().sum::<f64>() / n as f64;
    let mut v_y = 0.0f64;
    for yi in y {
        let d = *yi - mean_y;
        v_y += d * d;
    }
    v_y /= (n - 1) as f64;

    let mut v_d = 0.0f64;
    let nd = n - 1;
    let mut mean_d = 0.0f64;
    for i in 1..n {
        mean_d += y[i] - y[i - 1];
    }
    mean_d /= nd as f64;
    for i in 1..n {
        let d = y[i] - y[i - 1] - mean_d;
        v_d += d * d;
    }
    v_d /= (nd - 1).max(1) as f64;

    let q = (v_d * 0.5).max(1e-16);
    let r_raw = v_y - q;
    let r = r_raw.max(v_y * 0.05).max(1e-16);

    KalmanLocalLevelConfig {
        q,
        r,
        x_init: y[0],
        p_init: r.max(1e-8),
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

/// Mid-stream snapshot + restore must match uninterrupted filter.
#[cfg(test)]
mod tests {
    use helio_scan::Scan;

    use super::*;

    #[test]
    fn snapshot_resume_matches_continuous() {
        let y: Vec<f64> = (0..5000).map(|i| (i as f64).sin() * 0.01 + (i as f64) * 1e-5).collect();
        let cfg = train_local_level_heuristic(&y[..500.min(y.len())]);
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
}
