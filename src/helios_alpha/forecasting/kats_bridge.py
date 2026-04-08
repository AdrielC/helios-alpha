from __future__ import annotations

import polars as pl


def kats_detector_daily_returns(
    df: pl.DataFrame,
    *,
    time_col: str = "date",
    value_col: str = "ret",
) -> pl.DataFrame:
    """
    Simple change-point / outlier style detection on a daily series using Kats.

    Requires ``kats`` (and its stack). Returns detector output as Polars if available.
    """
    try:
        from kats.consts import TimeSeriesData
        from kats.detectors.outlier import OutlierDetector
    except ImportError as e:
        msg = "Install kats: pip install helios-alpha[forecasting]"
        raise ImportError(msg) from e

    sub = df.select([time_col, value_col]).drop_nulls().sort(time_col)
    if sub.is_empty():
        return pl.DataFrame()
    pdf = sub.to_pandas()
    pdf = pdf.rename(columns={time_col: "time", value_col: "y"})
    ts = TimeSeriesData(pdf)
    det = OutlierDetector(ts, "additive")
    det.detector()
    # Serialized scores vary by kats version — return as DataFrame
    scores = getattr(det, "scores", None)
    if scores is None:
        return pl.DataFrame()
    return pl.from_pandas(scores)
