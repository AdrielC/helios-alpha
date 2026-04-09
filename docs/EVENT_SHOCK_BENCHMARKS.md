# Event-shock vertical — benchmark baselines and thresholds

Recorded on **2026-04-09** from `cargo bench -p helio_bench --bench event_shock_vertical -- --noplot` (Criterion **0.4.0**, release build, Linux agent). **Your machine will differ**; use these as order-of-magnitude budgets, not golden files.

## Workload (what the bench measures)

- **256** synthetic `EventShock` rows + **~180** daily bar rows (two symbols), merged into a vertical replay stream.
- **E2E** = full `EventShockVerticalScan` (gate, filter, align, treatment + control signals, daily execution).
- **Checkpoint** = same stream through `Persisted` + `Runner` with one mid-stream `FlushReason::Checkpoint`.

## Recorded medians (representative run)

| Benchmark | Median | Notes |
|-----------|--------|--------|
| `event_shock_align_pipeline` | **~20.3 µs** | 256 shocks through gate + filter + align only |
| `event_shock_aligned_to_signal` | **~19.0 µs** | pre-aligned shocks → `EventShockToSignalScan` |
| `event_shock_e2e_replay` | **~4.33 ms** | full vertical, one replay pass |
| `event_shock_checkpoint_restart` | **~4.36 ms** | e2e + one persisted checkpoint flush |

## Thresholds (manual regression triage)

Until CI runs Criterion with saved baselines, treat **>2×** the median above on the same workload as “investigate,” and **>3×** as “likely regression or very different hardware.”

| Benchmark | Investigate above | Strong signal above |
|-----------|-------------------|---------------------|
| `event_shock_align_pipeline` | **45 µs** | **65 µs** |
| `event_shock_aligned_to_signal` | **45 µs** | **65 µs** |
| `event_shock_e2e_replay` | **9 ms** | **13 ms** |
| `event_shock_checkpoint_restart` | **9 ms** | **13 ms** |

**Composition overhead takeaway:** align (~20 µs) + signal (~19 µs) is **~40 µs** for 256 shocks, while full e2e is **~4.3 ms** — execution + control sampling + bar joins dominate; scan composition is not the bottleneck at this scale.

## Reproduce

```bash
cd rust
cargo bench -p helio_bench --bench event_shock_vertical -- --noplot
```
