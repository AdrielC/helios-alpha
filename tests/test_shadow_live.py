from __future__ import annotations

from datetime import UTC, datetime

import pytest

from helios_alpha.shadow.adapters import DEFAULT_SHADOW_FEEDS, HttpxJsonTransport
from helios_alpha.timekeeping import SystemClock


@pytest.mark.integration
def test_noaa_goes_xray_shadow_contract_is_live() -> None:
    feed = next(
        candidate
        for candidate in DEFAULT_SHADOW_FEEDS
        if candidate.source_id == "noaa-swpc-goes-xray-primary-v1"
    )
    clock = SystemClock()
    now = clock.now_utc()
    with HttpxJsonTransport(clock, timeout_seconds=20) as transport:
        document, candidates = feed.fetch(
            transport,
            datetime.fromtimestamp(now.timestamp(), tz=UTC).date(),
        )

    assert document.source_url.startswith("https://services.swpc.noaa.gov/")
    assert candidates
    assert all(item.payload["seriesId"] == "goes-xray-flux" for item in candidates)
    assert all(item.available_at is None for item in candidates)
