from __future__ import annotations

import json
from datetime import UTC, date, datetime
from pathlib import Path

import exchange_calendars as xc

from helios_alpha.markets.calendar_manifest import build_venue_schedule_manifest, main


def test_xnys_manifest_is_pinned_and_preserves_2026_thanksgiving_early_close() -> None:
    assert xc.__version__ == "4.13.2"
    generated_at = datetime(2026, 8, 30, tzinfo=UTC)
    manifest = build_venue_schedule_manifest(
        "XNYS", date(2026, 11, 25), date(2026, 11, 30), generated_at=generated_at
    )
    sessions = {row["label"]: row for row in manifest["sessions"]}
    thanksgiving = (date(2026, 11, 26) - date(1970, 1, 1)).days
    early_close = (date(2026, 11, 27) - date(1970, 1, 1)).days
    normal = (date(2026, 11, 25) - date(1970, 1, 1)).days

    assert thanksgiving not in sessions
    assert sessions[normal]["close_utc"] - sessions[normal]["open_utc"] == 23_400
    assert sessions[early_close]["close_utc"] - sessions[early_close]["open_utc"] == 12_600
    assert len(manifest["metadata"]["source_sha256"]) == 64


def test_manifest_is_deterministic_for_explicit_generation_time() -> None:
    generated_at = datetime(2026, 8, 30, tzinfo=UTC)
    args = ("XNYS", date(2026, 11, 25), date(2026, 11, 30))
    assert build_venue_schedule_manifest(*args, generated_at=generated_at) == (
        build_venue_schedule_manifest(*args, generated_at=generated_at)
    )


def test_python_export_matches_the_rust_interoperability_fixture() -> None:
    generated = build_venue_schedule_manifest(
        "XNYS",
        date(2026, 11, 25),
        date(2026, 11, 30),
        generated_at=datetime(2026, 8, 30, tzinfo=UTC),
    )
    fixture_path = (
        Path(__file__).parents[1]
        / "rust/crates/helio_time/tests/fixtures/xnys_2026_thanksgiving.json"
    )
    assert generated == json.loads(fixture_path.read_text())


def test_cli_writes_the_validated_manifest(tmp_path: Path) -> None:
    output = tmp_path / "schedules" / "xnys.json"
    assert (
        main(
            [
                "--exchange",
                "XNYS",
                "--start",
                "2026-11-25",
                "--end",
                "2026-11-30",
                "--generated-at",
                "2026-08-30T00:00:00Z",
                "--output",
                str(output),
            ]
        )
        == 0
    )
    assert json.loads(output.read_text()) == json.loads(
        (
            Path(__file__).parents[1]
            / "rust/crates/helio_time/tests/fixtures/xnys_2026_thanksgiving.json"
        ).read_text()
    )
