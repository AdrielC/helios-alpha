from __future__ import annotations

import textwrap
from datetime import date, timedelta
from pathlib import Path

import polars as pl

from helios_alpha.backtest.event_study import load_event_study_config, run_event_study


def test_load_event_study_config_from_path(tmp_path: Path) -> None:
    p = tmp_path / "thresholds.yaml"
    p.write_text(
        textwrap.dedent(
            """
            event_study:
              extreme_ssi_quantile: 0.85
              control_day_buffer_days: 7
            """
        ).strip(),
        encoding="utf-8",
    )
    c = load_event_study_config(p)
    assert c.extreme_ssi_quantile == 0.85
    assert c.control_day_buffer_days == 7


def test_run_event_study_uses_buffer_from_config(tmp_path: Path) -> None:
    """Wider buffer should admit fewer control windows (same prices, different exclusion)."""
    thr = tmp_path / "t.yaml"
    thr.write_text(
        textwrap.dedent(
            """
            event_study:
              extreme_ssi_quantile: 0.5
              control_day_buffer_days: 0
            """
        ).strip(),
        encoding="utf-8",
    )
    thr_wide = tmp_path / "t2.yaml"
    thr_wide.write_text(
        textwrap.dedent(
            """
            event_study:
              extreme_ssi_quantile: 0.5
              control_day_buffer_days: 5
            """
        ).strip(),
        encoding="utf-8",
    )

    # 11 flare days: median SSI = 0.5 → 6 treatment dates (>= 5 required for summary rows).
    n_flares = 11
    base = date(2020, 1, 1)
    event_dates = [base + timedelta(days=30 * i) for i in range(n_flares)]
    ssi_vals = [0.2] * 5 + [0.9] * 6
    events = pl.DataFrame({"event_date_utc": event_dates, "ssi": ssi_vals})

    # Long price panel so control pool can exceed 30 after exclusions.
    n_px = 420
    prices = pl.DataFrame(
        {
            "ticker": ["AAA"] * n_px,
            "date": [base + timedelta(days=i) for i in range(n_px)],
            "close": [100.0 + i * 0.01 for i in range(n_px)],
        }
    )

    as_of = base + timedelta(days=n_px - 1)
    _, s0 = run_event_study(events, prices, ["AAA"], thresholds_path=thr, as_of=as_of)
    _, s5 = run_event_study(events, prices, ["AAA"], thresholds_path=thr_wide, as_of=as_of)

    assert not s0.is_empty() and not s5.is_empty()
    n0 = int(s0.filter(pl.col("horizon") == 1, pl.col("metric") == "ret_cum")["n_control"][0])
    n5 = int(s5.filter(pl.col("horizon") == 1, pl.col("metric") == "ret_cum")["n_control"][0])
    assert n0 >= n5

    row = s0.row(0, named=True)
    assert row["control_day_buffer_days"] == 0
    assert row["extreme_ssi_quantile"] == 0.5
