# Event-shock vertical — benchmark baselines and thresholds

Recorded on **2026-04-09** from `cargo bench -p helio_bench --bench event_shock_vertical -- --noplot` (Criterion **0.4.0**, release build, Linux agent). **Your machine will differ**; use these as order-of-magnitude budgets, not golden files.

## Workload (what the bench measures)

- **256** synthetic `EventShock` rows + **~180** daily bar rows (two symbols), merged into a vertical replay stream.
- **E2E** = full `EventShockVerticalScan` (gate, filter, align, treatment + control signals, daily execution).
- **Checkpoint** = same stream through `Persisted` + `Runner` with one mid-stream `FlushReason::Checkpoint`.

## Recorded medians (representative run)

| Benchmark | Median | Notes |
|-----------|--------|--------|
| `event_shock_align_pipeline` | **~24.8 µs** | 256 shocks through gate + filter + align only |
| `event_shock_aligned_to_signal` | **~19.5 µs** | pre-aligned shocks → `EventShockToSignalScan` |
| `event_shock_e2e_replay` | **~4.39 ms** | full vertical, per-record `step` loop |
| `event_shock_e2e_step_batch_slice` | **~4.38 ms** | default `step_batch` (= ordered `step`) |
| `event_shock_e2e_run_slice` | **~4.42 ms** | `run_slice` over the replay buffer |
| `event_shock_checkpoint_restart` | **~4.40 ms** | e2e + one persisted checkpoint flush |

## Thresholds (manual regression triage)

Until CI runs Criterion with saved baselines, treat **>2×** the median above on the same workload as “investigate,” and **>3×** as “likely regression or very different hardware.”

| Benchmark | Investigate above | Strong signal above |
|-----------|-------------------|---------------------|
| `event_shock_align_pipeline` | **45 µs** | **65 µs** |
| `event_shock_aligned_to_signal` | **45 µs** | **65 µs** |
| `event_shock_e2e_replay` | **9 ms** | **13 ms** |
| `event_shock_e2e_step_batch_slice` | **9 ms** | **13 ms** |
| `event_shock_e2e_run_slice` | **9 ms** | **13 ms** |
| `event_shock_checkpoint_restart` | **9 ms** | **13 ms** |

**Composition overhead takeaway:** align (~25 µs) + signal (~19 µs) is **~45 µs** for 256 shocks, while full e2e is **~4.4 ms** — execution + control sampling + bar joins dominate; scan composition is not the bottleneck at this scale. **Step vs `step_batch` / `run_slice`** is effectively noise here (same per-element work).

Rolling-window step vs optimized batch for `helio_window` lives in `cargo bench -p helio_bench --bench execution_modes`.

## Reproduce

```bash
cd rust
cargo bench -p helio_bench --bench event_shock_vertical -- --noplot
```
