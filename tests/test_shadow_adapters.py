from __future__ import annotations

import hashlib
from datetime import UTC, datetime

from helios_alpha.shadow.adapters import (
    normalize_donki_cme,
    normalize_goes_xray,
    normalize_l1_magnetic_field,
)
from helios_alpha.shadow.models import RetrievedJson


def retrieved(payload: object, minute: int = 30) -> RetrievedJson:
    encoded = repr(payload).encode()
    return RetrievedJson(
        source_url="https://services.swpc.noaa.gov/feed.json",
        observed_at=datetime(2026, 8, 31, 12, minute, tzinfo=UTC),
        payload=payload,
        body_sha256=hashlib.sha256(encoded).hexdigest(),
    )


def test_goes_xray_uses_poll_receipt_as_conservative_availability() -> None:
    candidates = normalize_goes_xray(
        retrieved(
            [
                {
                    "time_tag": "2026-08-31T12:00:00Z",
                    "satellite": 18,
                    "energy": "0.05-0.4nm",
                    "flux": 1e-7,
                },
                {
                    "time_tag": "2026-08-31T12:00:00Z",
                    "satellite": 18,
                    "energy": "0.1-0.8nm",
                    "flux": 2e-6,
                    "observed_flux": 2.1e-6,
                    "electron_correction": 1e-7,
                    "electron_contaminaton": False,
                },
            ]
        )
    )

    assert len(candidates) == 1
    assert candidates[0].available_at is None
    assert candidates[0].payload["seriesId"] == "goes-xray-flux"
    assert candidates[0].payload["fluxWattsPerSquareMeter"] == 2e-6


def test_l1_magnetic_feed_preserves_provider_and_quality_flags() -> None:
    candidate = normalize_l1_magnetic_field(
        retrieved(
            [
                {
                    "time_tag": "2026-08-31T12:00:00",
                    "source": "IMAP",
                    "active": False,
                    "bt": 5.2,
                    "bz_gsm": -4.1,
                    "sample_size": 60,
                    "overall_quality": 2,
                }
            ]
        )
    )[0]

    assert candidate.payload["source"] == "IMAP"
    assert candidate.payload["bzGsmNt"] == -4.1
    assert candidate.quality_flags == ("provider_quality_nonzero", "source_not_primary")


def test_donki_cme_availability_waits_for_latest_model_completion() -> None:
    candidate = normalize_donki_cme(
        retrieved(
            [
                {
                    "activityID": "2026-08-31T10:00:00-CME-001",
                    "startTime": "2026-08-31T10:00Z",
                    "submissionTime": "2026-08-31T10:05Z",
                    "versionId": 2,
                    "linkedEvents": [],
                    "cmeAnalyses": [
                        {
                            "isMostAccurate": True,
                            "submissionTime": "2026-08-31T10:10Z",
                            "speed": 1400,
                            "halfAngle": 48,
                            "longitude": 8,
                            "latitude": -4,
                            "type": "C",
                            "enlilList": [
                                {
                                    "modelCompletionTime": "2026-08-31T10:20Z",
                                    "estimatedShockArrivalTime": "2026-09-02T03:00Z",
                                    "isEarthGB": True,
                                    "isEarthMinorImpact": False,
                                    "kp_18": 5,
                                    "kp_90": 7,
                                    "kp_135": 8,
                                    "kp_180": 8,
                                    "impactList": [
                                        {
                                            "location": "Earth",
                                            "arrivalTime": "2026-09-02T03:00Z",
                                            "isGlancingBlow": True,
                                        }
                                    ],
                                }
                            ],
                        }
                    ],
                }
            ],
            minute=30,
        )
    )[0]

    assert candidate.available_at == datetime(2026, 8, 31, 10, 20, tzinfo=UTC)
    assert candidate.payload["speedKms"] == 1400.0
    assert candidate.payload["earthImpacts"][0]["location"] == "Earth"
    assert candidate.payload["kpForecast"]["kp90"] == 7
