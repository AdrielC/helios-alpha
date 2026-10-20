from __future__ import annotations

from datetime import date
from typing import Literal

from helios_alpha.utils.http import get_json


def fetch_donki(
    product: Literal["CME", "FLR"], start: date, end: date, api_key: str
) -> list[dict]:
    """Use CCMC's public API by default, preserving explicitly keyed NASA access."""
    params = {"startDate": start.isoformat(), "endDate": end.isoformat()}
    if api_key and api_key != "DEMO_KEY":
        url = f"https://api.nasa.gov/DONKI/{product}"
        params["api_key"] = api_key
    else:
        url = f"https://ccmc.gsfc.nasa.gov/DONKI-API/get/{product}"
    data = get_json(url, params=params)
    return data if isinstance(data, list) else []
