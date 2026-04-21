use serde::{Deserialize, Serialize};

/// How to estimate `(q, r)` before streaming with [`crate::KalmanLocalLevelScan`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KalmanFitMode {
    /// Full EM: forward filter + RTS + M-step (see [`crate::fit_local_level_em`]).
    #[default]
    Em,
    /// Coordinate-wise innovation MLE (faster, no RTS).
    Mle,
}

/// Optional **local-level Kalman** pass over the harness’s daily toy series (see [`crate::kalman`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KalmanHarnessOptions {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub fit_mode: KalmanFitMode,
    /// Max prefix length (from the start of the series) used to **fit** `(q, r)` by MLE; `None` = cap at 50_000.
    #[serde(default)]
    pub train_prefix_cap: Option<usize>,
    /// When true, after the full run verifies mid-stream snapshot+resume matches one shot (O(2n); dev only).
    #[serde(default)]
    pub verify_snapshot_resume: bool,
}

impl Default for KalmanHarnessOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            fit_mode: KalmanFitMode::Em,
            train_prefix_cap: None,
            verify_snapshot_resume: false,
        }
    }
}
