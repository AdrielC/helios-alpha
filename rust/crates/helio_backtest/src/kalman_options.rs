use serde::{Deserialize, Serialize};

/// Optional **local-level Kalman** pass over the harness’s daily toy series (see [`crate::kalman`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KalmanHarnessOptions {
    #[serde(default)]
    pub enabled: bool,
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
            train_prefix_cap: None,
            verify_snapshot_resume: false,
        }
    }
}
