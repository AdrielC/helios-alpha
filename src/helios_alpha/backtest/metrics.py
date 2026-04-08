from __future__ import annotations

import numpy as np
from scipy import stats


def bootstrap_mean_diff(
    treatment: np.ndarray,
    control: np.ndarray,
    *,
    n_boot: int = 5000,
    seed: int = 42,
) -> tuple[float, float, float, float]:
    """
    Return observed_diff, two-sided bootstrap p-value, and a 95% CI for the difference.
    Diff is mean(treatment) minus mean(control).
    """
    rng = np.random.default_rng(seed)
    t = np.asarray(treatment, dtype=float)
    c = np.asarray(control, dtype=float)
    t = t[np.isfinite(t)]
    c = c[np.isfinite(c)]
    obs = float(np.mean(t) - np.mean(c))
    boots = []
    for _ in range(n_boot):
        tb = rng.choice(t, size=len(t), replace=True)
        cb = rng.choice(c, size=len(c), replace=True)
        boots.append(float(np.mean(tb) - np.mean(cb)))
    boots_arr = np.array(boots)
    p = 2 * min(np.mean(boots_arr <= 0), np.mean(boots_arr >= 0))
    p = min(1.0, p)
    lo, hi = np.percentile(boots_arr, [2.5, 97.5])
    return obs, p, float(lo), float(hi)


def welch_t_pvalue(treatment: np.ndarray, control: np.ndarray) -> float:
    t = np.asarray(treatment, dtype=float)
    c = np.asarray(control, dtype=float)
    t = t[np.isfinite(t)]
    c = c[np.isfinite(c)]
    if len(t) < 2 or len(c) < 2:
        return float("nan")
    return float(stats.ttest_ind(t, c, equal_var=False).pvalue)
