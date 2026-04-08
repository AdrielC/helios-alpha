from __future__ import annotations

from datetime import date, datetime
from pathlib import Path

import polars as pl
import yaml
from omegaconf import DictConfig, OmegaConf

from helios_alpha.backtest import event_study as es
from helios_alpha.config import load_settings
from helios_alpha.features.solar_shock_index import compute_ssi
from helios_alpha.ingest import cmes as cmes_mod
from helios_alpha.ingest import dst_kyoto, merge_events, omni_dst
from helios_alpha.ingest import flares as flares_mod
from helios_alpha.ingest import geomagnetic as geo_mod
from helios_alpha.ingest import prices as prices_mod
from helios_alpha.ingest import protons as protons_mod
from helios_alpha.timekeeping import Clock, FrozenClock, SystemClock


def _clock_from_cfg(cfg: DictConfig) -> Clock:
    kind = str(OmegaConf.select(cfg, "pipeline.clock.kind") or "system")
    if kind == "frozen":
        iso = OmegaConf.select(cfg, "pipeline.clock.frozen_iso")
        if not iso:
            msg = "pipeline.clock.frozen_iso required when kind=frozen"
            raise ValueError(msg)
        return FrozenClock(datetime.fromisoformat(str(iso).replace("Z", "+00:00")))
    return SystemClock()


def _repo_root() -> Path:
    return load_settings().repo_root


def _load_tickers(path: Path) -> list[str]:
    raw = yaml.safe_load(path.read_text(encoding="utf-8"))
    out: list[str] = []
    for _, v in (raw.get("tickers") or {}).items():
        if isinstance(v, list):
            out.extend(str(x) for x in v)
    return sorted(set(out))


def run_pipeline(cfg: DictConfig) -> None:
    clock = _clock_from_cfg(cfg)
    repo = _repo_root()
    start = date.fromisoformat(str(cfg.pipeline.start_date))
    end = date.fromisoformat(str(cfg.pipeline.end_date))
    assets_path = (repo / str(cfg.pipeline.paths.assets)).resolve()
    thresholds_path = (repo / str(cfg.pipeline.paths.thresholds)).resolve()
    tickers = _load_tickers(assets_path)

    if cfg.pipeline.steps.ingest_solar:
        print("Fetching DONKI flares / CMEs …")
        fl = flares_mod.fetch_flares_range(start, end)
        cm = cmes_mod.fetch_cmes_range(start, end)
        flares_mod.save_flares_parquet(fl)
        cmes_mod.save_cmes_parquet(cm)
    else:
        fl = pl.read_parquet(repo / "data/raw/solar/flares.parquet")
        cm = pl.read_parquet(repo / "data/raw/solar/cmes.parquet")

    if cfg.pipeline.steps.ingest_dst:
        src = str(cfg.pipeline.dst.source)
        if src == "kyoto_iswa":
            print("Ingesting Kyoto Dst (ISWA mirror) …")
            dst_kyoto.ingest_kyoto_dst_range(start, end)
        elif src == "omni_cdf":
            print("Ingesting OMNI hourly CDF Dst (SPDF) …")
            h = omni_dst.fetch_omni_dst_range(start, end)
            if not h.is_empty():
                omni_dst.merge_omni_dst_daily_from_hourly(h)
            else:
                print(
                    "OMNI pull returned empty (network or cdflib); "
                    "try pipeline.dst.source=kyoto_iswa."
                )
        elif src == "none":
            pass
        else:
            msg = f"Unknown pipeline.dst.source: {src}"
            raise ValueError(msg)

    if cfg.pipeline.steps.ingest_snapshots:
        print("Refreshing rolling NOAA snapshots …")
        if cfg.pipeline.snapshots.refresh_kp:
            geo_mod.ingest_kp_daily_refresh(clock)
        if cfg.pipeline.snapshots.refresh_protons:
            protons_mod.ingest_protons_refresh(clock)

    kp = geo_mod.load_kp_daily()
    dst = dst_kyoto.load_dst_daily()
    pr_path = repo / "data/raw/solar/protons_ge10.parquet"
    protons = pl.read_parquet(pr_path) if pr_path.exists() else pl.DataFrame()

    if cfg.pipeline.steps.merge_events:
        print("Building merged event table …")
        ev = merge_events.build_event_table(
            fl,
            cm,
            kp,
            protons if not protons.is_empty() else None,
            dst_daily=dst if not dst.is_empty() else None,
        )
        ev = compute_ssi(ev, config_path=thresholds_path)
        merge_events.save_event_table(ev)

    if cfg.pipeline.steps.download_prices:
        print("Downloading daily prices …")
        px = prices_mod.download_daily_prices(tickers, start, end)
        prices_mod.save_prices_parquet(px)

    if cfg.pipeline.steps.event_study:
        print("Running event study …")
        ev_path = repo / "data/processed/events/flare_cme_events.parquet"
        px_path = repo / "data/raw/market/daily_prices.parquet"
        ev_df = pl.read_parquet(ev_path) if ev_path.exists() else pl.DataFrame()
        px_df = pl.read_parquet(px_path) if px_path.exists() else pl.DataFrame()
        outcomes, summary = es.run_event_study(ev_df, px_df, tickers)
        es.save_event_study(outcomes, summary)

    _ = clock
    print("Done.")
