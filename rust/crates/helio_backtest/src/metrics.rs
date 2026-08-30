//! Summary metrics on **simple returns** (e.g. per-bar or per-day fractions).

use helio_stats::OnlineMoments;

/// Annualized Sharpe from a **daily** simple-return series: `(mean / sample_std) * sqrt(252)`.
///
/// Uses **sample** standard deviation (`n-1` denominator). Returns `None` if fewer than 2
/// observations or if the sample std is effectively zero (constant series).
///
/// `252` is a conventional U.S. equity **trading** day count; swap scaling if your `returns`
/// are on another cadence.
pub fn sharpe_annualized_daily(returns: &[f64]) -> Option<f64> {
    let mut moments = OnlineMoments::new();
    for &value in returns {
        if moments.try_push(value).is_err() {
            return None;
        }
    }
    let mean = moments.mean()?;
    let std = moments.sample_stddev()?;
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

    #[test]
    fn sharpe_rejects_non_finite_and_unrepresentable_series() {
        assert!(sharpe_annualized_daily(&[0.01, f64::NAN]).is_none());
        assert!(sharpe_annualized_daily(&[f64::MAX, -f64::MAX]).is_none());
    }
}
