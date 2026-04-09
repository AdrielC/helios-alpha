# Public API surface — recommendation

This is a **stability and packaging** pass over the Rust workspace: what to treat as *supported public API* vs *internal substrate*, whether to add an umbrella crate, and whether `helio_event` is the right boundary.

## 1. Internal-first vs public crates

| Crate | Default stance | Rationale |
|-------|----------------|-----------|
| **`helio_scan`** | **Internal / power-user API** | The value is the *algebra* (`Scan`, checkpoint, combinators). External users who only want “run a backtest” should not need to import it. Document as **kernel**; semver can stay strict, but **do not market** as the primary integration point. |
| **`helio_time`** | **Internal / supporting** | Semantics (`Timed`, `AvailableAt`, calendar traits) are shared infrastructure. Public to other Helio crates and advanced integrators; **not** the main story for app authors. |
| **`helio_window`** | **Internal / feature crate** | Rolling/session/horizon machinery. Same as `helio_time`: stable for composability, but **secondary** to the workload-facing API. |
| **`helio_event`** | **Primary *application-facing* crate (today)** | Holds the event-shock vertical, adapters, CLI, and classic event-study harness. This is the natural **“import this to do something real”** package until you split by volume. |
| **`helio_bench`** | **Non-published tooling** | Benchmarks only; keep `publish = false` and out of “public API” mentally. |
| **`helios_signald`** | **Deployable binary, not a library API** | Integrators care about the process and config, not re-exported types. |

**Summary:** Treat **`helio_event`** as the **user-facing library surface** for research/backtest workflows; treat **`helio_scan` + `helio_time` + `helio_window`** as **layered internals** that downstream *can* depend on if they build custom scans, but should not *have* to for the flagship path.

## 2. Umbrella crate (`helio` / `helios_core`) — do you need it?

**Not yet.**

- A single umbrella that re-exports `helio_scan`, `helio_time`, `helio_window`, `helio_event` **inflates dependency graphs** and blurs the “kernel vs workload” story.
- **Add an umbrella only when** at least one of:
  - You publish to **crates.io** and want **one version line** for consumers.
  - You add **FFI** or a **second binary** that must share a tiny, frozen type set.
  - `helio_event` becomes **too large** and you split **event-shock** vs **event-study** — then an umbrella can re-export both with a curated prelude.

**Interim pattern:** Document **`helio_event`** as the default dependency; mention optional direct deps on `helio_scan` / `helio_time` only for custom scan authors.

## 3. Is `helio_event` too specific, or correctly scoped?

**Correctly scoped for the current phase, with a clear fork ahead.**

- **Today:** One crate holding (a) classic **event-study** types and pipelines and (b) **event-shock** vertical is **honest**: both are “labeled time-series + scans” workloads on the same stack.
- **Risk:** As event-shock grows (more adapters, execution modes, reporting), the crate name **`helio_event`** reads like *all* events live here — which becomes heavy.
- **Future split (only if lines count and compile times hurt):**
  - `helio_event_shock` (or `helio_shock`) — forecast shocks, vertical, CLI.
  - `helio_event_study` — treatment/control, forward horizon, folds.
  - Shared tiny types (`SessionDate` stays in `helio_scan`; `Timed` in `helio_time`).

**Verdict:** **Do not rename `helio_event` now.** Revisit when the shock vertical and event-study each exceed ~N kLOC or when you need independent versioning.

## 4. Module / naming changes before scale

| Current | Issue | Suggestion |
|---------|--------|------------|
| `event_shock*` modules | Many files; fine internally | **Optional:** `src/shock/` directory with `mod.rs` re-exports — cosmetic, do when navigation hurts. |
| `EventShockStrategyPreset` | Name is clear | Keep; if strategies multiply, move to `strategy.rs` only (already there). |
| `replay_event_shock` binary | Good | If you add a second binary (`stream_shock`, etc.), keep **one** documented “golden” entrypoint in README. |
| `Symbol`, `SessionDate` | `Symbol` in `helio_event`, `SessionDate` in `helio_scan` | **Acceptable.** Avoid duplicating `SessionDate` in `helio_event`. If “market types” proliferate, add **`helio_market`** later — not now. |

**Do not** rename `helio_scan` / `helio_time` — names already match roles.

## 5. Documented “public” modules inside `helio_event` (suggested)

Treat as **stable-ish** for external callers:

- `EventShock`, `EventShockVerticalScan`, `TradeResult`, `Exposure`, `ExitPolicy`
- `build_vertical_replay`, `timed_shock`, `load_*` CSV helpers
- `EventShockStrategyPreset`, `summarize_lead_times`
- `collect_vertical_trades_*` replay helpers

Treat as **research / legacy** (may move or change more freely):

- `CausalEventStudyPipeline`, `TreatmentEvent`, `EventStudyFoldScan`, cluster/horizon wiring

Add a short **`helio_event` crate-level rustdoc** section (“Stable vs experimental”) when you next touch `lib.rs` — no need for a separate crate yet.

## 6. Bottom line

- **Internal:** `helio_scan`, `helio_time`, `helio_window` as **substrate** (public types, but not the product API).
- **Public face:** **`helio_event`** (+ `replay_event_shock`) for the flagship path.
- **Umbrella:** **Skip** until crates.io, FFI, or a forced split.
- **Rename `helio_event`:** **No** until shock vs study separation justifies two crates.
- **Next hardening:** Session/calendar authority and streaming invariants (see [EVENT_SHOCK_ARCHITECTURE.md](EVENT_SHOCK_ARCHITECTURE.md)) — that matters more than crate renaming.
