# helios-alpha

**helios-alpha** is a research codebase for **event-shock trading strategies**: a shock is observed or forecast, you respect when it becomes **actionable** (causal availability), align to **sessions**, and evaluate **treatment vs control** style outcomes with clear limits on what the data supports.

Two layers work together:

1. **Empirical work (Python)** — End-to-end research: ingest data, build features, align to **exchange sessions**, enforce a **causal cut** (`as_of_date`), and run event-study style backtests. The tree includes **space-weather** sources as one example dataset; the same patterns apply to other forecastable shocks.

2. **Execution substrate (Rust)** — A **layered, deterministic scan engine** for streams where you must respect **causality**, optional **checkpoints**, **replay**, and **windowed** state. The **`helio_event`** crate provides a domain-agnostic **event-shock vertical** (lead times, signals, simulated execution); shock taxonomy lives in your ingest or in optional string **`tags`**, not in the scan types.

---

## Architecture at a glance

### Python: research and data plane

The **`helios_alpha`** package runs the end-to-end **research pipeline**: pull public space-weather and market data, build composite indices (e.g. Solar Shock Index), align to **exchange sessions**, enforce a **causal cut** (`as_of_date`), and compare event windows to controls. Configuration is **Hydra**; time handling is **pendulum** with an explicit **Clock** (frozen vs system). See **What lives here** below for ingest sources and artifacts.

### Rust: scan kernel and time semantics

Under `rust/` lives a **Cargo workspace** of small crates with strict boundaries (see [docs/HELIO_RUST_WORKSPACE.md](docs/HELIO_RUST_WORKSPACE.md) and [docs/HELIO_SCAN.md](docs/HELIO_SCAN.md)):

| Crate | Responsibility |
|-------|------------------|
| **`helio_scan`** | **Domain-free algebra**: `Scan` / `FlushableScan` / `SnapshottingScan`, combinators, checkpoints, **opaque batching by default** (`ScanBatchExt`), **opt-in** `BatchOptimizedScan`, **runners** (`run_iter`, `run_batch`, `run_receiver`, optional async `run_stream`) — transports stay *outside* the core traits. |
| **`helio_time`** | **Semantics only**: `Frequency`, `Bounds`, `BucketSpec`, `WindowSpec`, `Timed<T>`, `AvailableAt`, availability gates — *what* a window means in domain language, **not** automatic eviction of every variant. |
| **`helio_window`** | **Operational machinery**: ring buffers, aggregators, rolling/session/horizon scans — **today many rolling paths are sample-count-driven**; rich `WindowSpec` can describe more than the ring buffer enforces until time-keyed expiry is implemented (see [docs/TIME_AND_WINDOWS.md](docs/TIME_AND_WINDOWS.md)). |
| **`helio_event`** | **Event-shock strategies** on the scan stack: causal **event-study** harnesses *and* a domain-agnostic **event-shock vertical** (`EventShock`, lead-time filters, signal generation, replay). Intended to **stress-test** the stack; may split later if it grows. |
| **`helio_bench`** | **Criterion** benchmarks (not a default workspace member); pinned Criterion for toolchain compatibility — see crate README for the **intentional pin** and future **regression budgets**. |
| **`helios_signald`** | Optional **ZMQ** bridge toward live signals (needs system **libzmq** and a C++ toolchain to build). |

**Design invariants** worth preserving: **one step at a time** in the kernel; **batching as adapters** unless a lawful optimized batch exists; **semantic time** in `helio_time` vs **rolling operations** in `helio_window`; **replay and snapshot tests** in `helio_event` to lock determinism.

### How the two sides connect

- **Research** produces Parquet features and event-study outputs in Python.
- **Rust** is where you build **causality-correct**, **replayable** streaming logic (backtest iterator vs live channel) without polluting the scan traits with domain or transport.
- **Live path**: JSON over ZMQ — [docs/EXECUTION_AND_SIGNALS.md](docs/EXECUTION_AND_SIGNALS.md) and `rust/crates/helios_signald/`.

---

## What lives here (Python pipeline)

- **Ingest**: NASA DONKI (flares, CMEs), NOAA SWPC (1-minute Kp, GOES integral protons), **Kyoto Dst (ISWA mirror)** and optional **OMNI hourly CDF**, Yahoo Finance daily prices (`yfinance`).
- **Features**: Solar Shock Index (SSI) from human priors in `config/thresholds.yaml` — tweak weights, do not worship them.
- **Backtest**: Event-study style comparison of top-decile SSI flare days vs spaced-out control days, with a bootstrap on the mean difference.
- **Time**: **`pendulum`** everywhere we parse, construct, or shift instants; stdlib **`date`** only at Polars/pandas/yfinance edges. **`Clock`** (`FrozenClock` vs `SystemClock`); only `SystemClock` calls `pendulum.now("UTC")`. See [docs/PENDULUM_AND_PANDAS.md](docs/PENDULUM_AND_PANDAS.md).
- **Sessions**: **`exchange_calendars`** (XNYS) + pandas **`CustomBusinessDay` / `CustomBusinessHour`** — see [docs/TRADING_CALENDAR.md](docs/TRADING_CALENDAR.md).
- **Causal cut**: `pipeline.as_of_date` threads through ingest windows and the event study (default: `end_date`).
- **Config**: **Hydra** compose (`src/helios_alpha/conf/`) — all pipeline args are overrides.

**Data catalog**: [DATA_SOURCES.md](DATA_SOURCES.md).

**Licensed market data:** [docs/MARKET_DATA_PROVIDERS.md](docs/MARKET_DATA_PROVIDERS.md) — default pick **Polygon.io**; `pipeline.market.provider=polygon` + `HELIOS_POLYGON_API_KEY`.

**Symbols:** [docs/INSTRUMENTS.md](docs/INSTRUMENTS.md) (`config/instruments.yaml`, `config/assets.yaml`).

**Live path (signals → Rust)**: `pip install -e ".[execution]"` for `pyzmq`. Orders stay behind a separate risk/broker process.

Parquet outputs are gitignored; regenerate locally.

---

## Quickstart

### pip

```bash
cd /path/to/helios-alpha
python -m venv .venv && source .venv/bin/activate
pip install -e ".[dev]"

# Optional: export HELIOS_NASA_API_KEY=... (defaults to DEMO_KEY)
helios-pipeline pipeline.start_date=2024-01-01 pipeline.end_date=2024-01-31

# Causal cut: only data through as_of_date (defaults to end_date if omitted)
helios-pipeline pipeline.start_date=2020-01-01 pipeline.end_date=2024-12-31 pipeline.as_of_date=2023-06-30
```

### Rust workspace

```bash
cd rust
cargo test
# Benchmarks (optional):
cargo bench -p helio_bench --no-run
```

### uv (reproducible)

```bash
uv sync
uv run helios-pipeline pipeline.start_date=2024-01-01 pipeline.end_date=2024-01-31 pipeline.as_of_date=2024-01-31
```

### Frozen clock (no implicit “now” in library code)

```bash
helios-pipeline pipeline.clock.kind=frozen pipeline.clock.frozen_iso=2024-06-01T12:00:00+00:00 \
  pipeline.start_date=2024-01-01 pipeline.end_date=2024-01-31
```

### Optional forecasting (Prophet)

```bash
pip install -e ".[forecasting]"
```

Bridges: `helios_alpha/forecasting/prophet_bridge.py`, `kats_bridge.py` (Kats: separate venv — dependency conflict with modern `statsmodels`).

### Tests

```bash
pytest
pytest -m integration   # live API smoke tests
```

CI (GitHub Actions): **ruff** + **pytest** (unit always; integration job is best-effort), **Rust** `cargo test` and `cargo build --release -p helios_signald` from `rust/` with `libzmq3-dev` + `g++`.

Artifacts:

- `data/raw/solar/flares.parquet`, `cmes.parquet`, `solar/protons_ge10.parquet`, `geomagnetic/kp_daily.parquet`, `geomagnetic/dst_daily.parquet`, `market/daily_prices.parquet`
- `data/processed/events/flare_cme_events.parquet` (merged + SSI)
- `data/processed/backtest/event_study_*.parquet`

---

## Honest limitations

- **OMNI CDF** may be unreachable from some networks; use `pipeline.dst.source=kyoto_iswa` (default).
- **Kp “forecast” in SSI** is proxied by **prior UTC calendar day max Kp** (no lookahead relative to the flare timestamp).
- **CME Earth arrival** often missing in ENLIL; `earth_directed_strict` = model-listed Earth or WSA flags; `earth_directed_inclusive` adds halo/heuristic; **SSI uses strict** for the Earth-directed term.
- **Rust `WindowSpec`** can describe frequencies that **ring-buffer scans do not yet enforce** as wall-clock eviction; see [docs/TIME_AND_WINDOWS.md](docs/TIME_AND_WINDOWS.md).

---

## Notebooks

See `notebooks/` after you have run the pipeline once.

---

## Thesis chain

Forecastable shock (observation + lead time) → impact window → market repricing (vol, sectors, delay).  
The Rust stack encodes **event → availability → signal** under strict causality so you can swap shock sources without changing the scan kernel.

If simple event studies are inconclusive, the empirical layer still limits what you can claim — the substrate remains useful for other stream-driven research.
