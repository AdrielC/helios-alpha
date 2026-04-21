# Event-shock vertical — generalization notes

This memo records what generalized cleanly when adding a **second ingest shape** (compact CSV with optional region scope), **incremental replay**, **lead-time reporting**, and a **second strategy preset**—without changing the core `EventShock` model or the `Scan` algebra.

## What generalized cleanly

- **`EventShock` as the only shock payload** — Compact and full CSV rows both map to the same struct (`tags`, `scope`, `available_at`, impact window, severity, confidence). The vertical scan (`EventShockVerticalScan`) is unchanged; it does not interpret `tags`.
- **Ingest at the edge** — `load_compact_event_shocks_csv`, `load_compact_region_event_shocks_csv`, and `load_event_shocks_csv` stay outside the pipeline. No domain-specific enums inside scans.
- **Incremental vs batch vs checkpoint** — `step` is the single execution primitive; `collect_vertical_trades_incremental`, `collect_vertical_trades_batch` (`run_slice`), and `collect_vertical_trades_with_checkpoint_resume` all produce identical `TradeResult` sequences on the same ordered input (verified in tests and by default in `replay_event_shock`).
- **Lead time** — `signal_lead_secs` / `summarize_lead_times` are pure functions on `&[EventShock]`; the CLI wires the same `[min,max]` band into `EventShockFilterConfig` so “reported tradable count” matches “what the filter allows.”
- **Second strategy** — `EventShockStrategyPreset` only selects `ExitPolicy`, `Exposure`, and control horizon; still one `EventShockToSignalScan` inside the vertical.

## What still smells demo-specific

- **`SessionDate` = integer day index** — Real venues need a single **session calendar** shared by bars and shock alignment. Today `SimpleWeekdayCalendar` + bar `session` column are a stand-in; downstream meaning comes from how you choose those indices, not from the shock struct.
- **Demo bars** — Synthetic XLU / SPY / ITA series exist to exercise pairs; production would ingest real daily OHLC keyed to the same session index convention.
- **Control horizon** — For `MidImpactWindowSession` exits, controls still use a fixed `horizon_sessions` (5) in config; not automatically “same calendar length as treatment.” Good enough to prove dual strategies, not a final causal design.

## Stabilize before live transport (beyond `std` / tests)

1. **Ordered merge contract** — Document invariants for `build_vertical_replay` (bars-first per session, shock `stream_seq`, late bar handling). Streaming adapters must preserve order or explicitly watermark.
2. **Session index authority** — One module or config path that maps wall time → `SessionDate` for both events and bars; avoid each adapter guessing.
3. **Checkpoint boundaries** — Define what `FlushReason::Checkpoint` means for the vertical (already plumbed through sub-scans); callers need offset + snapshot keying policy before MPSC or file tailers.
4. **Backpressure / pending execution** — `SignalExecutionScan` buffers signals until bars arrive; for live feeds, cap or spill policy must be explicit.

## Roadmap alignment

1. Runnable demo — done.  
2. Second ingest shape + incremental replay — done (compact-region CSV, replay helpers, CLI verify).  
3. **Next:** time-keyed / session-keyed window execution, watermarks, bucket completion vs availability — see workspace roadmap in `HELIO_RUST_WORKSPACE.md`.

## Invariant checklist (merge, sessions, checkpoints, execution)

- **Merge key:** [`vertical_merge_key`](rust/crates/helio_event/src/event_shock_vertical.rs) sorts by `(session, kind, stream_seq)` with stable tie-break on ingest order; bars sort before shocks in the same session. See rustdoc on [`build_vertical_replay`](rust/crates/helio_event/src/event_shock_ingest.rs) vs [`build_vertical_replay_with_calendar`](rust/crates/helio_event/src/event_shock_ingest.rs).
- **Session authority:** shocks in the CLI path use [`merge_session_for_shock`](rust/crates/helio_event/src/event_shock_ingest.rs) with the same [`TradingCalendar`](rust/crates/helio_time/src/calendar.rs) as the vertical; [`validate_bar_sessions_vs_shock_calendar`](rust/crates/helio_event/src/event_shock_ingest.rs) rejects bars keyed to raw UTC weekend days when a shock rolls forward.
- **Checkpoints:** [`EventShockVerticalScan::flush`](rust/crates/helio_event/src/event_shock_vertical.rs) forwards `FlushReason` to all sub-scans; replay helpers document cadence in [`event_shock_replay`](rust/crates/helio_event/src/event_shock_replay.rs).
- **Execution backpressure:** [`ExecutionBufferPolicy`](rust/crates/helio_event/src/event_shock_execution.rs) on [`SignalExecutionScan`](rust/crates/helio_event/src/event_shock_execution.rs); default unbounded; optional cap with deterministic `DropOldest`. Wire via [`EventShockVerticalScan::with_exec_buffer`](rust/crates/helio_event/src/event_shock_vertical.rs).
