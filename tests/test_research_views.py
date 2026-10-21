import json
from pathlib import Path

import pytest

from helios_alpha.control.store import ControlError
from helios_alpha.natgas.cli import publish_snapshot
from helios_alpha.natgas.lab import Journal, settings
from helios_alpha.research_views import ViewDefinition, project, publish, view_settings

NOW = 1_800_000_000_000
FIXTURE = Path(__file__).parent / "fixtures/research-view.json"


def definition():
    return ViewDefinition.model_validate(
        {
            "title": "Rate revisions",
            "subtitle": "Synthetic contract fixture",
            "mark": "RR",
            "capabilities": ["Release archive", "Revision study"],
            "sections": [
                {
                    "id": "summary",
                    "kind": "metrics",
                    "title": "Policy rate",
                    "fields": [
                        {
                            "key": "rate",
                            "label": "Target",
                            "path": "rate",
                            "format": "percent",
                            "scale": 100,
                        }
                    ],
                },
                {
                    "id": "observations",
                    "kind": "table",
                    "title": "Release observations",
                    "rows_path": "observations",
                    "observed_path": "at",
                    "max_age_ms": 10000,
                    "fields": [
                        {"key": "source", "label": "Source", "path": "source"},
                        {
                            "key": "rate",
                            "label": "Rate",
                            "path": "rate",
                            "format": "percent",
                            "scale": 100,
                        },
                    ],
                },
                {
                    "id": "method",
                    "kind": "notes",
                    "title": "Research notes",
                    "rows_path": "notes",
                    "fields": [{"key": "note", "label": "Method", "path": "."}],
                },
            ],
        }
    )


def snapshot():
    return {
        "execution_authority": False,
        "rate": 425,
        "observations": [{"source": "Test release", "rate": 425, "at": NOW - 1000}],
        "notes": ["Synthetic data for renderer verification."],
        "private_payload": "must not escape",
    }


def test_shared_python_rust_browser_contract():
    actual = project("rates-study", definition(), snapshot(), NOW)
    assert actual == json.loads(FIXTURE.read_text())
    assert "must not escape" not in json.dumps(actual)
    assert "rows_path" not in json.dumps(actual)


def test_registration_lifecycle_and_persistent_display_settings(tmp_path):
    root = tmp_path / "private"
    root.mkdir(mode=0o700)
    default = tmp_path / "default.json"
    default.write_text(definition().model_dump_json())
    configured = view_settings(root, "rates-study", default)
    publish(root, "rates-study", configured, snapshot(), NOW)
    publish(root, "other-study", configured, snapshot(), NOW)
    directory = root / "research-views"
    assert sorted(p.stem for p in directory.glob("*.json")) == ["other-study", "rates-study"]
    assert (directory / "rates-study.json").stat().st_mode & 0o777 == 0o600
    path = root / "research-view-settings/rates-study.json"
    saved = json.loads(path.read_text())
    saved.update(enabled=False, title="My rate study")
    path.write_text(json.dumps(saved))
    restarted = view_settings(root, "rates-study", default)
    assert restarted.title == "My rate study" and restarted.enabled is False
    publish(root, "rates-study", restarted, snapshot(), NOW + 1000)
    assert not (directory / "rates-study.json").exists()
    assert (directory / "other-study.json").exists()
    restarted.enabled = True
    publish(root, "rates-study", restarted, snapshot(), NOW + 2000)
    assert json.loads((directory / "rates-study.json").read_text())["title"] == "My rate study"


def test_unknown_missing_and_unsafe_values_are_not_fabricated(tmp_path):
    data = snapshot()
    del data["rate"]
    output = project("rates-study", definition(), data, NOW)
    assert output["sections"][0]["rows"][0]["values"]["rate"] is None
    data["rate"] = {"account_secret": "never"}
    with pytest.raises(ValueError, match="scalar"):
        project("rates-study", definition(), data, NOW)
    with pytest.raises(ValueError, match="publisher"):
        publish(tmp_path, "../outside", definition(), snapshot(), NOW)
    data = snapshot()
    data["execution_authority"] = True
    with pytest.raises(ValueError, match="non-executing"):
        project("rates-study", definition(), data, NOW)
    data = snapshot()
    data["observations"][0]["at"] = NOW + 1
    with pytest.raises(ValueError, match="observation time"):
        project("rates-study", definition(), data, NOW)
    for mutate in [
        lambda d: d["sections"][0].update(kind="remote-component"),
        lambda d: d["sections"][0]["fields"][0].update(path="__import__('os')"),
        lambda d: d["sections"][0]["fields"][0].update(format="html"),
        lambda d: d["sections"].append(d["sections"][0]),
    ]:
        value = definition().model_dump()
        mutate(value)
        with pytest.raises(ValueError):
            ViewDefinition.model_validate(value)


def test_symlink_settings_are_not_read(tmp_path):
    root = tmp_path / "private"
    root.mkdir(mode=0o700)
    (root / "research-view-settings").symlink_to(tmp_path, target_is_directory=True)
    with pytest.raises(ControlError, match="symlink"):
        view_settings(root, "rates-study", FIXTURE)


def test_actual_collector_registers_and_removes_only_its_own_view(tmp_path):
    root = tmp_path / "private"
    root.mkdir(mode=0o700)
    journal = Journal(root)
    try:
        config = settings(root)
        publish_snapshot(root, journal, config)
        path = root / "research-views/natural-gas.json"
        book = json.loads(path.read_text())
        assert book["title"] == "Natural gas"
        assert book["sections"][1]["rows"] == []
        assert all("path" not in f for s in book["sections"] for f in s["fields"])
        setting = root / "research-view-settings/natural-gas.json"
        saved = json.loads(setting.read_text())
        saved["enabled"] = False
        setting.write_text(json.dumps(saved))
        publish_snapshot(root, journal, config)
        assert not path.exists()
        assert (root / "natgas-status.json").exists()
        # A bad display edit cannot stop evidence capture or leave a plausible live panel.
        setting.write_text('{"access_token":"not-for-the-browser","sections":"bad"}')
        publish_snapshot(root, journal, config)
        rejected = json.loads(path.read_text())
        assert rejected == {
            "schema_version": 1,
            "registration_error": "invalid_definition_or_projection",
        }
        assert json.loads((root / "natgas-status.json").read_text())["execution_authority"] is False
    finally:
        journal.close()
