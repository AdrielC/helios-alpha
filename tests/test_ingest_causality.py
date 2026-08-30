from __future__ import annotations

import json
from datetime import UTC, date, datetime

import polars as pl

from helios_alpha.ingest.cmes import ingest_cmes_json
from helios_alpha.ingest.flares import ingest_flares_json
from helios_alpha.ingest.merge_events import build_event_table


def utc(hour: int, minute: int = 0) -> datetime:
    return datetime(2026, 8, 30, hour, minute, tzinfo=UTC)


def test_donki_ingest_preserves_publication_clock_and_version(tmp_path) -> None:
    flare_path = tmp_path / "flares.json"
    flare_path.write_text(
        json.dumps(
            [
                {
                    "flrID": "2026-08-30T12:00:00-FLR-001",
                    "peakTime": "2026-08-30T12:00Z",
                    "beginTime": "2026-08-30T11:55Z",
                    "endTime": "2026-08-30T12:08Z",
                    "classType": "X1.2",
                    "submissionTime": "2026-08-30T12:05Z",
                    "versionId": 3,
                    "linkedEvents": [
                        {"activityID": "2026-08-30T12:03:00-CME-001"}
                    ],
                }
            ]
        ),
        encoding="utf-8",
    )
    cme_path = tmp_path / "cmes.json"
    cme_path.write_text(
        json.dumps(
            [
                {
                    "activityID": "2026-08-30T12:03:00-CME-001",
                    "startTime": "2026-08-30T12:03Z",
                    "cmeAnalyses": [
                        {
                            "isMostAccurate": True,
                            "speed": 1400,
                            "halfAngle": 48,
                            "longitude": 8,
                            "latitude": -4,
                            "type": "C",
                            "submissionTime": "2026-08-30T12:15Z",
                            "versionId": 7,
                        }
                    ],
                }
            ]
        ),
        encoding="utf-8",
    )

    flare = ingest_flares_json(flare_path).row(0, named=True)
    cme = ingest_cmes_json(cme_path).row(0, named=True)

    assert flare["submission_time_utc"] == utc(12, 5)
    assert flare["version_id"] == "3"
    assert cme["submission_time_utc"] == utc(12, 15)
    assert cme["version_id"] == "7"


def test_event_availability_waits_for_every_required_donki_fact() -> None:
    flares = pl.DataFrame(
        {
            "flare_id": ["flare-complete", "flare-missing-cme-clock"],
            "peak_time_utc": [utc(12), utc(13)],
            "begin_time_utc": [utc(11, 55), utc(12, 55)],
            "end_time_utc": [utc(12, 8), utc(13, 8)],
            "class_type": ["X1.2", "M8.0"],
            "active_region_num": [14321, 14322],
            "linked_cme_ids": ["cme-complete", "cme-missing-clock"],
            "submission_time_utc": [utc(12, 5), utc(13, 5)],
            "version_id": ["3", "1"],
        }
    )
    cmes = pl.DataFrame(
        {
            "cme_id": ["cme-complete", "cme-missing-clock"],
            "start_time_utc": [utc(12, 3), utc(13, 3)],
            "speed_kms": [1400.0, 900.0],
            "half_angle_deg": [48.0, 38.0],
            "longitude_deg": [8.0, 12.0],
            "latitude_deg": [-4.0, 1.0],
            "cme_type": ["C", "C"],
            "earth_arrival_start_utc": [None, None],
            "earth_arrival_end_utc": [None, None],
            "enlil_earth_gb": [False, False],
            "enlil_earth_minor_impact": [False, False],
            "earth_impact_listed": [False, False],
            "earth_impact_glancing": [False, False],
            "earth_directed_heuristic": [True, True],
            "linked_flare_ids": ["flare-complete", "flare-missing-cme-clock"],
            "submission_time_utc": [utc(12, 15), None],
            "version_id": ["7", None],
        },
        schema_overrides={
            "earth_arrival_start_utc": pl.Datetime(time_zone="UTC"),
            "earth_arrival_end_utc": pl.Datetime(time_zone="UTC"),
            "submission_time_utc": pl.Datetime(time_zone="UTC"),
        },
    )
    kp_daily = pl.DataFrame(
        {
            "date_utc": [date(2026, 8, 29), date(2026, 8, 30)],
            "kp_estimated_max": [3.0, 4.0],
            "kp_index_max": [3, 4],
        }
    )

    events = build_event_table(flares, cmes, kp_daily).sort("peak_time_utc")
    complete = events.row(0, named=True)
    incomplete = events.row(1, named=True)

    assert complete["causal_replay_eligible"] is True
    assert complete["available_at_utc"] == utc(12, 15)
    assert incomplete["causal_replay_eligible"] is False
    assert incomplete["available_at_utc"] is None
