from __future__ import annotations

import time
from typing import Any

import httpx


def get_json(url: str, *, params: dict[str, Any] | None = None, retries: int = 4) -> Any:
    last_err: Exception | None = None
    for attempt in range(retries + 1):
        try:
            with httpx.Client(timeout=60.0) as client:
                r = client.get(url, params=params)
                r.raise_for_status()
                return r.json()
        except (httpx.HTTPError, ValueError) as e:
            last_err = e
            if attempt < retries:
                time.sleep(4 * (2**attempt))
    assert last_err is not None
    raise last_err
