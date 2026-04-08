"""
Internal instrument ids → provider-native symbols.

YAML is the source of truth; pipelines and Parquet ``ticker`` columns use **id** only.
"""

from __future__ import annotations

from pathlib import Path

import yaml
from pydantic import BaseModel, Field


class Instrument(BaseModel):
    id: str = Field(..., description="Canonical id used in this repo (Parquet ticker column)")
    yfinance: str | None = None
    polygon: str | None = None

    def symbol_for(self, provider: str) -> str:
        p = provider.lower().strip()
        if p == "yfinance":
            return self.yfinance or self.id
        if p == "polygon":
            return self.polygon or self.id
        msg = f"Unknown provider: {provider}"
        raise ValueError(msg)


def load_instrument_registry(path: Path) -> dict[str, Instrument]:
    raw = yaml.safe_load(path.read_text(encoding="utf-8"))
    items = raw.get("instruments") or raw.get("universe")
    if not isinstance(items, list):
        msg = "instruments.yaml must contain an 'instruments' list"
        raise ValueError(msg)
    out: dict[str, Instrument] = {}
    for row in items:
        if not isinstance(row, dict):
            continue
        iid = str(row.get("id") or "").strip()
        if not iid:
            continue
        inst = Instrument(
            id=iid,
            yfinance=(str(row["yfinance"]).strip() if row.get("yfinance") else None),
            polygon=(str(row["polygon"]).strip() if row.get("polygon") else None),
        )
        out[iid] = inst
    return out


def universe_ids(registry: dict[str, Instrument]) -> list[str]:
    return sorted(registry.keys())


def provider_symbol(registry: dict[str, Instrument], instrument_id: str, provider: str) -> str:
    inst = registry.get(instrument_id)
    if inst is None:
        msg = f"Unknown instrument id: {instrument_id!r} (not in registry)"
        raise KeyError(msg)
    return inst.symbol_for(provider)
