"""
Network integration tests. Run with: pytest -m integration
Requires NASA DEMO_KEY or HELIOS_NASA_API_KEY for DONKI if rate-limited.
"""

from datetime import date

import pytest

pytestmark = pytest.mark.integration


@pytest.mark.parametrize(
    "url",
    [
        "https://services.swpc.noaa.gov/json/planetary_k_index_1m.json",
        "https://iswa.gsfc.nasa.gov/iswa_data_tree/index/geomagnetic/Dst-realtime/WDC-Kyoto/dst2401.txt",
    ],
)
def test_http_sources_reachable(url: str):
    import httpx

    r = httpx.get(url, timeout=45.0)
    assert r.status_code == 200
    assert len(r.text) > 50


def test_donki_flr_small_range():
    from helios_alpha.ingest import flares as flares_mod

    df = flares_mod.fetch_flares_range(date(2024, 1, 1), date(2024, 1, 10))
    assert df.height > 0


def test_kyoto_month_fetch():
    from helios_alpha.ingest import dst_kyoto

    df = dst_kyoto.fetch_kyoto_dst_month(2024, 1)
    assert df.height >= 24 * 28
