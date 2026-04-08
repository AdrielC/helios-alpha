from __future__ import annotations

from dataclasses import dataclass
from datetime import date
from math import isfinite
from pathlib import Path

import numpy as np
import polars as pl

from helios_alpha.backtest.metrics import bootstrap_mean_diff, welch_t_pvalue
from helios_alpha.config import load_settings

HORIZONS = (0, 1, 2, 5)


def _finite_floats(values: list) -> list[float]:
    out: list[float] = []
    for x in values:
        if x is None:
            continue
        try:
            v = float(x)
        except (TypeError, ValueError):
            continue
        if isfinite(v):
            out.append(v)
    return out


@dataclass
class EventStudyConfig:
    ssi_extreme_q: float = 0.9
    min_control_days: int = 500


def _window_metrics(rets: list[float]) -> tuple[float, float]:
    """Cumulative return and annualized realized vol from daily simple returns."""
    clean = []
    for x in rets:
        if x is None:
            continue
        try:
            v = float(x)
        except (TypeError, ValueError):
            continue
        if isfinite(v):
            clean.append(v)
    if not clean:
        return float("nan"), float("nan")
    cum = 1.0
    for r in clean:
        cum *= 1.0 + r
    cum_r = cum - 1.0
    if len(clean) < 2:
        rv = abs(clean[0]) * np.sqrt(252.0)
    else:
        rv = float(np.std(clean, ddof=1) * np.sqrt(252.0))
    return float(cum_r), rv


def forward_outcomes_for_ticker(
    prices: pl.DataFrame, ticker: str, event_dates: list[date]
) -> pl.DataFrame:
    """Rows per event date with forward cumulative return and RV for each horizon window."""
    p = (
        prices.filter(pl.col("ticker") == ticker)
        .sort("date")
        .with_columns((pl.col("close") / pl.col("close").shift(1) - 1.0).alias("ret"))
    )
    if p.is_empty():
        return pl.DataFrame()
    dates = p["date"].to_list()
    rets = p["ret"].to_list()
    d2i = {d: i for i, d in enumerate(dates)}
    rows: list[dict] = []
    for ed in event_dates:
        i0 = d2i.get(ed)
        if i0 is None:
            continue
        row: dict = {"event_date_utc": ed, "ticker": ticker}
        for k in HORIZONS:
            sl = rets[i0 : i0 + k + 1]
            cum_r, rv = _window_metrics(sl)
            row[f"ret_cum_{k}"] = cum_r
            row[f"rv_ann_{k}"] = rv
        rows.append(row)
    return pl.DataFrame(rows)


def run_event_study(
    events: pl.DataFrame,
    prices: pl.DataFrame,
    tickers: list[str],
    *,
    ssi_col: str = "ssi",
    event_date_col: str = "event_date_utc",
    extreme_quantile: float = 0.9,
) -> tuple[pl.DataFrame, pl.DataFrame]:
    """
    Build per-event forward outcomes, then compare high-SSI events vs non-event control days.

    Control days: all trading dates for the ticker that are not within ±3 calendar days of any
    high-SSI event date (same ticker universe).
    """
    if events.is_empty() or prices.is_empty():
        return pl.DataFrame(), pl.DataFrame()

    ev = events.drop_nulls(subset=[ssi_col, event_date_col])
    thr = float(ev[ssi_col].quantile(extreme_quantile))
    high_dates = sorted(set(ev.filter(pl.col(ssi_col) >= thr)[event_date_col].to_list()))

    all_outcomes: list[pl.DataFrame] = []
    for t in tickers:
        all_outcomes.append(forward_outcomes_for_ticker(prices, t, high_dates))
    outcomes = pl.concat(all_outcomes, how="vertical_relaxed") if all_outcomes else pl.DataFrame()

    ssi_by_date = (
        ev.group_by(event_date_col)
        .agg(pl.col(ssi_col).max().alias("ssi_at_event"))
        .rename({event_date_col: "event_date_utc"})
    )
    outcomes = outcomes.join(ssi_by_date, on="event_date_utc", how="left")

    summary_rows: list[dict] = []
    for t in tickers:
        p = prices.filter(pl.col("ticker") == t).sort("date")
        if p.is_empty():
            continue
        dates = p["date"].to_list()
        rets = (p["close"] / p["close"].shift(1) - 1.0).to_list()
        high_for_ticker = set(high_dates)

        def is_control_day(d: date) -> bool:
            if d in high_for_ticker:
                return False
            for hd in high_for_ticker:
                if abs((d - hd).days) <= 3:
                    return False
            return True

        control_indices = [i for i, d in enumerate(dates) if is_control_day(d)]
        for k in HORIZONS:
            treat_vals: list[float] = []
            if not outcomes.is_empty():
                sub = outcomes.filter(pl.col("ticker") == t)
                col = f"ret_cum_{k}"
                if col in sub.columns:
                    treat_vals = _finite_floats(sub[col].to_list())

            ctrl_vals: list[float] = []
            for i in control_indices:
                if i + k >= len(rets):
                    continue
                sl = rets[i : i + k + 1]
                cum_r, _ = _window_metrics(_finite_floats(sl))
                if cum_r == cum_r:
                    ctrl_vals.append(cum_r)

            if len(treat_vals) < 5 or len(ctrl_vals) < 30:
                continue
            obs, p_boot, lo, hi = bootstrap_mean_diff(
                np.array(treat_vals), np.array(ctrl_vals)
            )
            p_welch = welch_t_pvalue(np.array(treat_vals), np.array(ctrl_vals))
            summary_rows.append(
                {
                    "ticker": t,
                    "horizon": k,
                    "metric": "ret_cum",
                    "n_treat": len(treat_vals),
                    "n_control": len(ctrl_vals),
                    "mean_treat": float(np.mean(treat_vals)),
                    "mean_control": float(np.mean(ctrl_vals)),
                    "diff": obs,
                    "p_bootstrap": p_boot,
                    "ci95_low": lo,
                    "ci95_high": hi,
                    "p_welch": p_welch,
                    "ssi_threshold": thr,
                }
            )

            treat_rv = []
            if not outcomes.is_empty():
                sub = outcomes.filter(pl.col("ticker") == t)
                c2 = f"rv_ann_{k}"
                if c2 in sub.columns:
                    treat_rv = _finite_floats(sub[c2].to_list())
            ctrl_rv: list[float] = []
            for i in control_indices:
                if i + k >= len(rets):
                    continue
                sl = rets[i : i + k + 1]
                _, rv = _window_metrics(_finite_floats(sl))
                if rv == rv:
                    ctrl_rv.append(rv)
            if len(treat_rv) < 5 or len(ctrl_rv) < 30:
                continue
            obs2, p_b2, lo2, hi2 = bootstrap_mean_diff(
                np.array(treat_rv), np.array(ctrl_rv)
            )
            summary_rows.append(
                {
                    "ticker": t,
                    "horizon": k,
                    "metric": "rv_ann",
                    "n_treat": len(treat_rv),
                    "n_control": len(ctrl_rv),
                    "mean_treat": float(np.mean(treat_rv)),
                    "mean_control": float(np.mean(ctrl_rv)),
                    "diff": obs2,
                    "p_bootstrap": p_b2,
                    "ci95_low": lo2,
                    "ci95_high": hi2,
                    "p_welch": welch_t_pvalue(np.array(treat_rv), np.array(ctrl_rv)),
                    "ssi_threshold": thr,
                }
            )

    summary = pl.DataFrame(summary_rows)
    return outcomes, summary


def save_event_study(
    outcomes: pl.DataFrame, summary: pl.DataFrame, prefix: str = "event_study"
) -> tuple[Path, Path]:
    s = load_settings()
    base = s.data_processed / "backtest"
    base.mkdir(parents=True, exist_ok=True)
    p1 = base / f"{prefix}_outcomes.parquet"
    p2 = base / f"{prefix}_summary.parquet"
    if not outcomes.is_empty():
        outcomes.write_parquet(p1)
    if not summary.is_empty():
        summary.write_parquet(p2)
    return p1, p2
