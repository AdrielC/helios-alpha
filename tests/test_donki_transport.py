from datetime import date

import pytest

from helios_alpha.ingest import cmes, donki, flares


@pytest.mark.parametrize("fetch", [cmes._donki_cme, flares._donki_flr])
@pytest.mark.parametrize("api_key", ["", "DEMO_KEY", "test-private-key"])
def test_donki_routes_credentials_only_to_keyed_gateway(monkeypatch, fetch, api_key):
    requests = []
    records = [{"submissionTime": "2024-01-02T12:00Z", "versionId": 3}]

    def get_json(url, *, params):
        requests.append((url, params))
        return records

    monkeypatch.setattr(donki, "get_json", get_json)
    assert fetch(date(2024, 1, 1), date(2024, 1, 10), api_key) == records
    assert len(requests) == 1
    url, params = requests[0]
    assert params["startDate"] == "2024-01-01"
    assert params["endDate"] == "2024-01-10"
    if api_key == "test-private-key":
        assert url.startswith("https://api.nasa.gov/DONKI/")
        assert params["api_key"] == api_key
    else:
        assert url.startswith("https://ccmc.gsfc.nasa.gov/DONKI-API/get/")
        assert "api_key" not in params
