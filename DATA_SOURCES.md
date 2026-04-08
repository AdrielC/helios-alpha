# Data sources

Authoritative links and how this repo uses them. Prefer **versioned files** or **documented APIs**; cache to Parquet under `data/raw/` (gitignored).

## Solar flares and CMEs (fast, structured JSON)

| Source | What | Endpoint / pattern | Notes |
|--------|------|--------------------|--------|
| **NASA DONKI** (CCMC-backed) | Solar flares (`FLR`), CMEs (`CME`), linked events, ENLIL when present | `https://api.nasa.gov/DONKI/FLR?startDate=…&endDate=…&api_key=…` and `…/DONKI/CME?…` | **Accurate for catalogued events**; not real-time trading latency. Use your own `api.nasa.gov` key (`HELIOS_NASA_API_KEY`) for rate limits. Implemented in `helios_alpha/ingest/flares.py`, `cmes.py`. |

## Geomagnetic indices

| Source | What | Endpoint / pattern | Notes |
|--------|------|--------------------|--------|
| **NOAA SWPC** | Planetary K index, 1-minute cadence (rolling window on their server) | `https://services.swpc.noaa.gov/json/planetary_k_index_1m.json` | Good for **recent** Kp; not a full historical archive in one file. We aggregate to **daily max** and merge into `kp_daily.parquet`. |
| **Kyoto Dst (via NASA ISWA mirror)** | Hourly Dst, WDC-style monthly text | `https://iswa.gsfc.nasa.gov/iswa_data_tree/index/geomagnetic/Dst-realtime/WDC-Kyoto/dstYYMM.txt` | **Primary Dst path in this repo** when SPDF is blocked. Mirror of Kyoto-style files; for publication-grade provenance also cite **WDC Kyoto** directly. Parser: `helios_alpha/ingest/dst_kyoto.py`. |
| **OMNI (NASA SPDF)** | Hourly merged indices including Dst (dataset-dependent) | `https://spdf.gsfc.nasa.gov/pub/data/omni/omni_cdaweb/hourly/{year}/omni2_h0_mrg1hr_{yyyymmdd}_v01.cdf` | **Canonical interplanetary context**; requires `cdflib` and network access to SPDF. Some environments reset TLS to SPDF; then use **Kyoto ISWA** path. Code: `helios_alpha/ingest/omni_dst.py`. |

## Solar energetic particles (proxy for radiation / comms risk)

| Source | What | Endpoint | Notes |
|--------|------|----------|--------|
| **NOAA SWPC GOES** | Integral proton flux, multiple energies, ~7 day JSON | `https://services.swpc.noaa.gov/json/goes/primary/integral-protons-7-day.json` | We keep `>=10 MeV` as `protons_ge10.parquet`. |

## Market prices

| Source | What | Access | Notes |
|--------|------|--------|--------|
| **Yahoo Finance** (via `yfinance`) | Adjusted daily OHLCV | Unofficial; no contract | Default in pipeline. **Not** for redistribution. `helios_alpha/ingest/prices.py`. |
| **Polygon.io** | REST aggregates (daily, etc.) | Paid API key | Recommended licensed path. `HELIOS_POLYGON_API_KEY`, `pipeline.market.provider=polygon`. `helios_alpha/ingest/polygon.py`. |

See [docs/MARKET_DATA_PROVIDERS.md](docs/MARKET_DATA_PROVIDERS.md) for vendor comparison. Canonical **ticker** ids and provider symbol maps: [docs/INSTRUMENTS.md](docs/INSTRUMENTS.md).

## Facebook / Meta forecasting stacks (optional)

| Package | Role in this repo | Install |
|---------|-------------------|---------|
| **Prophet** | Univariate baselines, seasonality decomposition for returns or vol | `pip install -e ".[forecasting]"` — bridge: `helios_alpha/forecasting/prophet_bridge.py` |
| **Kats** | Change-point / outlier detection on daily series | **Not** in `uv.lock`: pins old `statsmodels`; use a **separate venv** or `uv pip install kats` in an isolated env. Bridge: `helios_alpha/forecasting/kats_bridge.py`. |

These are **not** used in the default ingest pipeline; wire them in notebooks or a dedicated Hydra job when you move past event studies.

## Configuration and reproducibility

- **Hydra**: `src/helios_alpha/conf/` — run `helios-pipeline pipeline.start_date=… pipeline.end_date=…`
- **Clock**: `pipeline.clock.kind=frozen` + `pipeline.clock.frozen_iso=…` for backtests; `system` only when you intentionally want live snapshot pulls. Implementation: `helios_alpha/timekeeping.py`.
- **UV**: use `uv lock` / `uv sync` with `uv.lock` in repo for reproducible resolves (see README).
