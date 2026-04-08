from __future__ import annotations

import polars as pl


def prophet_forecast_daily(
    df: pl.DataFrame,
    *,
    date_col: str = "date",
    y_col: str = "y",
    periods: int = 30,
    yearly_seasonality: bool = True,
) -> pl.DataFrame:
    """
    Univariate daily forecast. Requires ``prophet`` installed.

    ``df`` must have columns ``date_col`` (Date or datetime) and ``y_col`` (float).
    """
    try:
        from prophet import Prophet
    except ImportError as e:
        msg = "Install prophet: pip install helios-alpha[forecasting]"
        raise ImportError(msg) from e

    pdf = df.select([date_col, y_col]).drop_nulls()
    if pdf.is_empty():
        return pl.DataFrame(schema={"ds": pl.Date, "yhat": pl.Float64})
    p = pdf.to_pandas()
    p = p.rename(columns={date_col: "ds", y_col: "y"})
    m = Prophet(yearly_seasonality=yearly_seasonality, daily_seasonality=False)
    m.fit(p)
    future = m.make_future_dataframe(periods=periods)
    fc = m.predict(future)
    out = pl.from_pandas(fc[["ds", "yhat", "yhat_lower", "yhat_upper"]])
    return out.rename({"ds": date_col})
