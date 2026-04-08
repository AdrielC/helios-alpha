# helios-alpha

A research repo for testing whether space weather events, especially solar flares, coronal mass ejections, and geomagnetic storms, create measurable effects in financial markets through infrastructure stress, operational disruption, volatility regime shifts, or delayed news transmission.

This is not astrology.  
This is an attempt to turn the Sun into a risk factor.

## What lives here

- **Ingest**: NASA DONKI (flares, CMEs), NOAA SWPC (1-minute Kp, GOES integral protons), Yahoo Finance daily prices (`yfinance`).
- **Features**: Solar Shock Index (SSI) from human priors in `config/thresholds.yaml` — tweak weights, do not worship them.
- **Backtest**: Event-study style comparison of top-decile SSI flare days vs spaced-out control days, with a bootstrap on the mean difference.

Parquet outputs are gitignored; regenerate locally.

## Quickstart

```bash
cd /path/to/helios-alpha
python -m venv .venv && source .venv/bin/activate
pip install -e ".[dev]"

# Optional: export HELIOS_NASA_API_KEY=... (defaults to DEMO_KEY)
helios-pipeline --start 2018-01-01 --end 2024-12-31
```

Artifacts:

- `data/raw/solar/flares.parquet`, `cmes.parquet`, `solar/protons_ge10.parquet`, `geomagnetic/kp_daily.parquet`, `market/daily_prices.parquet`
- `data/processed/events/flare_cme_events.parquet` (merged + SSI)
- `data/processed/backtest/event_study_*.parquet`

## Honest limitations (v0)

- **Dst** is not wired yet; add Kyoto or OMNI and join on storm windows when you need ring-current severity.
- **Kp “forecast” in SSI** is proxied by **prior UTC calendar day max Kp** (no lookahead relative to the flare timestamp).
- **CME Earth arrival** often missing in ENLIL; we keep model-listed Earth hits, WSA flags, and a wide/halo **heuristic** column — read `earth_directed` as a composite flag.

## Notebooks

See `notebooks/` after you have run the pipeline once.

## Thesis chain

Solar event → Earth impact forecast → infrastructure stress / headline risk → asset response.  
If nothing shows up in simple event studies, the rest of the stack is entertainment.
