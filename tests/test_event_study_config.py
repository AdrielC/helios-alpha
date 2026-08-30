from __future__ import annotations

import textwrap
from datetime import UTC, date, datetime, time, timedelta
from pathlib import Path

import polars as pl
import pytest

from helios_alpha.backtest.event_study import (
    CausalEventStudyError,
    classify_causal_extremes,
    load_event_study_config,
    run_event_study,
)


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
    events = pl.DataFrame(
        {
            "event_date_utc": event_dates,
            "available_at_utc": [
                datetime.combine(day, time(12), tzinfo=UTC) for day in event_dates
            ],
            "ssi": ssi_vals,
        }
    )

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
    assert row["ssi_threshold_mode"] == "expanding_prior_only"


def test_later_events_cannot_change_an_earlier_extreme_classification() -> None:
    base = datetime(2026, 1, 1, tzinfo=UTC)
    original = pl.DataFrame(
        {
            "event_date_utc": [(base + timedelta(days=i)).date() for i in range(7)],
            "available_at_utc": [base + timedelta(days=i) for i in range(7)],
            "ssi": [0.1, 0.2, 0.3, 0.4, 0.5, 0.9, 0.1],
        }
    )
    changed_future = original.with_columns(
        pl.when(pl.col("available_at_utc") == base + timedelta(days=6))
        .then(10_000.0)
        .otherwise(pl.col("ssi"))
        .alias("ssi")
    )
    before = classify_causal_extremes(original, quantile=0.5, min_history=5)
    after = classify_causal_extremes(changed_future, quantile=0.5, min_history=5)
    assert before["is_extreme_ssi"][5] is True
    assert before["is_extreme_ssi"][5] == after["is_extreme_ssi"][5]
    assert before["ssi_threshold_at_event"][5] == after["ssi_threshold_at_event"][5]


def test_event_study_refuses_missing_availability() -> None:
    events = pl.DataFrame({"event_date_utc": [date(2026, 1, 1)], "ssi": [0.9]})
    prices = pl.DataFrame(
        {"ticker": ["AAA"], "date": [date(2026, 1, 1)], "close": [100.0]}
    )
    with pytest.raises(CausalEventStudyError, match="available_at_utc"):
        run_event_study(events, prices, ["AAA"])
