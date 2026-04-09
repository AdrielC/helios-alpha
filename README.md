```
    _________________________________________________________________
   /                                                                 \
  |   .     *     .         *              h e l i o s - a l p h a   |
  |      *    \  |  /    *                                          |
  |   .    . -- ( @ ) -- .    .        flare  ·  cme  ·  dst  ·  book |
  |      *    /  |  \    *                                          |
  |   .     *     .         *                                       |
  |              \ | /                                              |
  |               \|/   space weather in  -->  risk out  -->  PnL   |
  |                |                                                |
  |    ~~~^~~~^~~~^~~~^~~~^~~~^~~~^~~~^~~~^~~~^~~~^~~~^~~~^~~~^~~~   |
  |      |   |   |   |   |   |   |   |   |   |   |   |   |   |      |
   \_____|___|___|___|___|___|___|___|___|___|___|___|___|___|_____/
```

# helios-alpha

A research repo for testing whether space weather events, especially solar flares, coronal mass ejections, and geomagnetic storms, create measurable effects in financial markets through infrastructure stress, operational disruption, volatility regime shifts, or delayed news transmission.

This is not astrology.  
This is an attempt to turn the Sun into a risk factor.

## What lives here

- **Ingest**: NASA DONKI (flares, CMEs), NOAA SWPC (1-minute Kp, GOES integral protons), **Kyoto Dst (ISWA mirror)** and optional **OMNI hourly CDF**, Yahoo Finance daily prices (`yfinance`).
- **Features**: Solar Shock Index (SSI) from human priors in `config/thresholds.yaml` — tweak weights, do not worship them.
- **Backtest**: Event-study style comparison of top-decile SSI flare days vs spaced-out control days, with a bootstrap on the mean difference.
- **Time**: **`pendulum`** everywhere we parse, construct, or shift instants; stdlib **`date`** only at Polars/pandas/yfinance edges. **`Clock`** (`FrozenClock` vs `SystemClock`); only `SystemClock` calls `pendulum.now("UTC")`. See [docs/PENDULUM_AND_PANDAS.md](docs/PENDULUM_AND_PANDAS.md).
- **Sessions**: **`exchange_calendars`** (XNYS) + pandas **`CustomBusinessDay` / `CustomBusinessHour`** — see [docs/TRADING_CALENDAR.md](docs/TRADING_CALENDAR.md).
- **Causal cut**: `pipeline.as_of_date` threads through ingest windows and the event study (default: `end_date`).
- **Config**: **Hydra** compose (`src/helios_alpha/conf/`) — all pipeline args are overrides.

**Data catalog**: see [DATA_SOURCES.md](DATA_SOURCES.md).

**Licensed market data:** [docs/MARKET_DATA_PROVIDERS.md](docs/MARKET_DATA_PROVIDERS.md) — default pick **Polygon.io**; `pipeline.market.provider=polygon` + `HELIOS_POLYGON_API_KEY`.

**Symbols:** canonical ids + per-provider maps — [docs/INSTRUMENTS.md](docs/INSTRUMENTS.md) (`config/instruments.yaml`, `config/assets.yaml`).

**Live path (signals → Rust)**: local ZMQ pub/sub + JSON schema — [docs/EXECUTION_AND_SIGNALS.md](docs/EXECUTION_AND_SIGNALS.md) and `rust/helios_signald/`. Install `pip install -e ".[execution]"` for `pyzmq`. Orders stay behind a separate risk/broker process.

**Rust scan substrate**: composable state machines over ordered streams — [docs/HELIO_SCAN.md](docs/HELIO_SCAN.md) and `rust/helio_scan/` (Cargo workspace root: `rust/Cargo.toml`).

Parquet outputs are gitignored; regenerate locally.

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

CI (GitHub Actions): **ruff** + **pytest** (unit always; integration job is best-effort), **Rust** `cargo test -p helio_scan` and `cargo build --release -p helios_signald` from `rust/` with `libzmq3-dev` + `g++`.

Artifacts:

- `data/raw/solar/flares.parquet`, `cmes.parquet`, `solar/protons_ge10.parquet`, `geomagnetic/kp_daily.parquet`, `geomagnetic/dst_daily.parquet`, `market/daily_prices.parquet`
- `data/processed/events/flare_cme_events.parquet` (merged + SSI)
- `data/processed/backtest/event_study_*.parquet`

## Honest limitations

- **OMNI CDF** may be unreachable from some networks; use `pipeline.dst.source=kyoto_iswa` (default).
- **Kp “forecast” in SSI** is proxied by **prior UTC calendar day max Kp** (no lookahead relative to the flare timestamp).
- **CME Earth arrival** often missing in ENLIL; `earth_directed_strict` = model-listed Earth or WSA flags; `earth_directed_inclusive` adds halo/heuristic; **SSI uses strict** for the Earth-directed term.

## Notebooks

See `notebooks/` after you have run the pipeline once.

## Thesis chain

Solar event → Earth impact forecast → infrastructure stress / headline risk → asset response.  
If nothing shows up in simple event studies, the rest of the stack is entertainment.
