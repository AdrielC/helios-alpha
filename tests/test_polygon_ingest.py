import os

import pytest

from helios_alpha.ingest import polygon as poly


@pytest.mark.integration
@pytest.mark.skipif(not os.environ.get("HELIOS_POLYGON_API_KEY"), reason="HELIOS_POLYGON_API_KEY not set")
def test_polygon_daily_agg_smoke():
    from datetime import date

    df = poly.fetch_daily_aggregates(
        "SPY", date(2024, 1, 2), date(2024, 1, 5), api_key=os.environ["HELIOS_POLYGON_API_KEY"]
    )
    assert df.height >= 1
    assert "close" in df.columns
