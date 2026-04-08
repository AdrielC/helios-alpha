from __future__ import annotations

from pathlib import Path

import matplotlib.pyplot as plt
import polars as pl


def plot_ssi_histogram(df: pl.DataFrame, ssi_col: str = "ssi", out: Path | None = None) -> None:
    if df.is_empty() or ssi_col not in df.columns:
        return
    x = df[ssi_col].drop_nulls().to_numpy()
    fig, ax = plt.subplots(figsize=(7, 4))
    ax.hist(x, bins=40, color="#2d2d2d", edgecolor="white", alpha=0.85)
    ax.set_title("Solar Shock Index (SSI) — distribution")
    ax.set_xlabel("SSI")
    ax.set_ylabel("Count")
    fig.tight_layout()
    if out:
        out.parent.mkdir(parents=True, exist_ok=True)
        fig.savefig(out, dpi=150)
    plt.close(fig)
