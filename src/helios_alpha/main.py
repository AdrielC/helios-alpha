from __future__ import annotations

from pathlib import Path

import pendulum
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
from helios_alpha.ingest import polygon as polygon_mod
from helios_alpha.ingest import prices as prices_mod
from helios_alpha.ingest import protons as protons_mod
from helios_alpha.markets.trading_calendar import load_trading_calendar_config
from helios_alpha.timekeeping import Clock, FrozenClock, SystemClock
from helios_alpha.utils.time import parse_date_iso


def _clock_from_cfg(cfg: DictConfig) -> Clock:
    kind = str(OmegaConf.select(cfg, "pipeline.clock.kind") or "system")
    if kind == "frozen":
        iso = OmegaConf.select(cfg, "pipeline.clock.frozen_iso")
        if not iso:
            msg = "pipeline.clock.frozen_iso required when kind=frozen"
            raise ValueError(msg)
        return FrozenClock(pendulum.parse(str(iso), tz="UTC"))
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
    start = parse_date_iso(str(cfg.pipeline.start_date))
    end = parse_date_iso(str(cfg.pipeline.end_date))
    raw_as_of = OmegaConf.select(cfg, "pipeline.as_of_date")
    as_of = (
        parse_date_iso(str(raw_as_of)) if raw_as_of not in (None, "null", "") else end
    )
    assets_path = (repo / str(cfg.pipeline.paths.assets)).resolve()
    thresholds_path = (repo / str(cfg.pipeline.paths.thresholds)).resolve()
    markets_path = (repo / str(cfg.pipeline.paths.markets)).resolve()
    tickers = _load_tickers(assets_path)
    tcal = load_trading_calendar_config(markets_path)

    if cfg.pipeline.steps.ingest_solar:
        print("Fetching DONKI flares / CMEs …")
        fl = flares_mod.fetch_flares_range(start, min(end, as_of))
        cm = cmes_mod.fetch_cmes_range(start, min(end, as_of))
        flares_mod.save_flares_parquet(fl)
        cmes_mod.save_cmes_parquet(cm)
    else:
        fl = pl.read_parquet(repo / "data/raw/solar/flares.parquet")
        cm = pl.read_parquet(repo / "data/raw/solar/cmes.parquet")

    if cfg.pipeline.steps.ingest_dst:
        src = str(cfg.pipeline.dst.source)
        if src == "kyoto_iswa":
            print("Ingesting Kyoto Dst (ISWA mirror) …")
            dst_kyoto.ingest_kyoto_dst_range(start, min(end, as_of))
        elif src == "omni_cdf":
            print("Ingesting OMNI hourly CDF Dst (SPDF) …")
            h = omni_dst.fetch_omni_dst_range(start, min(end, as_of))
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
            trading_calendar=tcal,
        )
        ev = compute_ssi(ev, config_path=thresholds_path)
        merge_events.save_event_table(ev)

    if cfg.pipeline.steps.download_prices:
        prov = str(OmegaConf.select(cfg, "pipeline.market.provider") or "yfinance")
        print(f"Downloading daily prices ({prov}) …")
        if prov == "polygon":
            s = load_settings()
            px = polygon_mod.download_daily_prices_polygon(
                tickers,
                start,
                min(end, as_of),
                api_key=s.polygon_api_key or None,
                base_url=str(cfg.pipeline.market.polygon.base_url),
            )
        elif prov == "yfinance":
            px = prices_mod.download_daily_prices(tickers, start, min(end, as_of))
        else:
            msg = f"Unknown pipeline.market.provider: {prov}"
            raise ValueError(msg)
        prices_mod.save_prices_parquet(px)

    if cfg.pipeline.steps.event_study:
        print("Running event study …")
        ev_path = repo / "data/processed/events/flare_cme_events.parquet"
        px_path = repo / "data/raw/market/daily_prices.parquet"
        ev_df = pl.read_parquet(ev_path) if ev_path.exists() else pl.DataFrame()
        px_df = pl.read_parquet(px_path) if px_path.exists() else pl.DataFrame()
        outcomes, summary = es.run_event_study(
            ev_df,
            px_df,
            tickers,
            as_of=as_of,
            trading_calendar=tcal,
            filter_events_to_sessions=bool(cfg.pipeline.trading.filter_events_to_sessions),
            use_session_horizons=True,
        )
        es.save_event_study(outcomes, summary)

    _ = clock
    print("Done.")
