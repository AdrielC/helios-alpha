import json

from helios_alpha.signals.schema import HeliosSignalV1, SignalKind


def test_signal_roundtrip_json():
    s = HeliosSignalV1(
        source="test.unit",
        kind=SignalKind.warning,
        topic_suffix="ssi",
        emitted_at_utc="2024-01-01T12:00:00+00:00",
        payload={"ssi": 0.55, "band": "warning"},
    )
    d = json.loads(s.to_json_bytes().decode("utf-8"))
    assert d["schema_version"] == "1"
    assert d["kind"] == "warning"
    assert d["payload"]["ssi"] == 0.55
    assert "signal_id" in d
    assert "emitted_at_utc" in d
