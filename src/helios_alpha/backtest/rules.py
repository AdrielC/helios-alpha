from __future__ import annotations

import polars as pl


def rule_long_vol_proxy(
    events: pl.DataFrame,
    *,
    ssi_min: float = 0.55,
    earth_directed: bool = True,
    speed_min: float = 400.0,
) -> pl.DataFrame:
    """Example Phase-2 filter: high SSI + Earth-directed + minimum CME speed."""
    q = events.filter(pl.col("ssi") >= ssi_min)
    if earth_directed:
        q = q.filter(pl.col("earth_directed"))
    q = q.filter(pl.col("speed_kms").is_not_null() & (pl.col("speed_kms") >= speed_min))
    return q
