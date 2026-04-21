//! Summary metrics on **simple returns** (e.g. per-bar or per-day fractions).

/// Annualized Sharpe from a **daily** simple-return series: `(mean / sample_std) * sqrt(252)`.
///
/// Uses **sample** standard deviation (`n-1` denominator). Returns `None` if fewer than 2
/// observations or if the sample std is effectively zero (constant series).
///
/// `252` is a conventional U.S. equity **trading** day count; swap scaling if your `returns`
/// are on another cadence.
pub fn sharpe_annualized_daily(returns: &[f64]) -> Option<f64> {
    let n = returns.len();
    if n < 2 {
        return None;
    }
    let sum: f64 = returns.iter().sum();
    let mean = sum / n as f64;
    let mut acc = 0.0f64;
    for r in returns {
        let d = *r - mean;
        acc += d * d;
    }
    let var = acc / (n - 1) as f64;
    let std = var.sqrt();
    if !std.is_finite() || std < 1e-12 {
        return None;
    }
    let sharpe = (mean / std) * 252f64.sqrt();
    sharpe.is_finite().then_some(sharpe)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sharpe_positive_on_upward_drift() {
        let r: Vec<f64> = (0..60).map(|i| 0.001 + (i as f64) * 1e-5).collect();
        let s = sharpe_annualized_daily(&r).expect("finite std");
        assert!(s > 0.0, "expected positive Sharpe, got {s}");
    }

    #[test]
    fn sharpe_none_on_constant() {
        let r = vec![0.01f64; 10];
        assert!(sharpe_annualized_daily(&r).is_none());
    }
}
