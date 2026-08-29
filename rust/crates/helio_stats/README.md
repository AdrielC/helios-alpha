# helio_stats

Domain-free, restartable online statistics for Helios pipelines.

## Primitives

- `OnlineMoments`: Welford update state `(count, mean, M2)`, sample/population variance, rolling removal, and Chan-style parallel merge.
- `merge_moments_balanced`: deterministic balanced reduction over partition states.
- `OnlineCovariance`: mergeable covariance, marginal variance, and correlation.
- `ExponentialHawkes`: O(1) marked exponential-kernel conditional intensity for clustered point events.
- `OnlineMomentsScan`, `OnlineCovarianceScan`, `HawkesScan`: `helio_scan` adapters with serializable, versioned snapshots.

The scan adapters implement `FallibleRestoreScan`, so non-finite or structurally invalid external snapshots are rejected before resume.

## Boundaries

These are state and execution primitives, not parameter-fitting or trading-profit claims. In particular, `ExponentialHawkes` evaluates an already chosen parameterization. Fit and validate parameters out of sample, test residuals, model regime changes, and include the configuration in the pipeline fingerprint.

```bash
cd rust
cargo test -p helio_stats
cargo bench -p helio_bench --bench online_stats -- --noplot
```
