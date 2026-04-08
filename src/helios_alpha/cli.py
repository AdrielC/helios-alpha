from __future__ import annotations

import argparse
from datetime import date
from pathlib import Path

import polars as pl
import yaml

from helios_alpha.backtest import event_study as es
from helios_alpha.config import load_settings
from helios_alpha.features.solar_shock_index import compute_ssi
from helios_alpha.ingest import cmes as cmes_mod
from helios_alpha.ingest import flares as flares_mod
from helios_alpha.ingest import geomagnetic as geo_mod
from helios_alpha.ingest import merge_events
from helios_alpha.ingest import prices as prices_mod
from helios_alpha.ingest import protons as protons_mod


def _load_tickers(path: Path) -> list[str]:
    raw = yaml.safe_load(path.read_text(encoding="utf-8"))
    out: list[str] = []
    for _, v in (raw.get("tickers") or {}).items():
        if isinstance(v, list):
            out.extend(str(x) for x in v)
    return sorted(set(out))


def main(argv: list[str] | None = None) -> None:
    p = argparse.ArgumentParser(description="helios-alpha ingest + event study pipeline")
    p.add_argument("--start", type=str, required=True, help="ISO start date (inclusive)")
    p.add_argument("--end", type=str, required=True, help="ISO end date (inclusive)")
    p.add_argument(
        "--assets",
        type=Path,
        default=None,
        help="Path to assets.yaml (default: config/assets.yaml under repo root)",
    )
    args = p.parse_args(argv)

    s = load_settings()
    start = date.fromisoformat(args.start)
    end = date.fromisoformat(args.end)
    assets_path = args.assets or (s.repo_root / "config" / "assets.yaml")
    tickers = _load_tickers(assets_path)

    print("Fetching DONKI flares / CMEs …")
    fl = flares_mod.fetch_flares_range(start, end)
    cm = cmes_mod.fetch_cmes_range(start, end)
    flares_mod.save_flares_parquet(fl)
    cmes_mod.save_cmes_parquet(cm)

    print("Refreshing geomagnetic + proton snapshots …")
    geo_mod.ingest_kp_daily_refresh()
    protons_mod.ingest_protons_refresh()
    kp = geo_mod.load_kp_daily()
    pr_path = s.data_raw / "solar" / "protons_ge10.parquet"
    protons = pl.read_parquet(pr_path) if pr_path.exists() else pl.DataFrame()

    print("Building merged event table …")
    ev = merge_events.build_event_table(fl, cm, kp, protons if not protons.is_empty() else None)
    ev = compute_ssi(ev)
    merge_events.save_event_table(ev)

    print("Downloading daily prices …")
    px = prices_mod.download_daily_prices(tickers, start, end)
    prices_mod.save_prices_parquet(px)

    print("Running event study …")
    outcomes, summary = es.run_event_study(ev, px, tickers)
    es.save_event_study(outcomes, summary)
    print("Done. Processed outputs under data/processed/.")


if __name__ == "__main__":
    main()
