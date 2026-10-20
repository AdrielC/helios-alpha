import json
from types import SimpleNamespace

import pytest

from helios_alpha.control.golem_store import GolemControlStore
from helios_alpha.control.models import ClientGrant, RiskLimits
from helios_alpha.control.settings import cached_settings, export_settings, mcp_connection
from helios_alpha.control.store import ControlError, ControlStore


@pytest.fixture
def store(tmp_path):
    return ControlStore.initialize(
        tmp_path / "private",
        RiskLimits(
            capital_micros=1000,
            order_micros=100,
            daily_loss_micros=50,
            drawdown_micros=100,
            symbol_exposure_micros=500,
            max_daily_orders=3,
            max_spread_bps=10,
            max_quote_age_ms=1000,
            symbols=["BTC-USD"],
        ),
        [
            ClientGrant(client_id="observer", capabilities=["risk:read", "status:read"]),
            ClientGrant(client_id="status-only", capabilities=["status:read"]),
        ],
        actor="test",
    )


def test_private_export_is_explicitly_non_authoritative_and_tamper_detected(store):
    exported = export_settings(store, "observer")
    cached = cached_settings(store.root)
    assert cached["settings"] == exported["settings"] == store.settings("observer")
    assert not cached["execution_authority"]
    assert cached["read_only_export"]
    assert cached["cache_age_seconds"] >= 0
    path = store.root / "settings.json"
    assert path.stat().st_mode & 0o777 == 0o600
    value = json.loads(path.read_bytes())
    value["settings"]["policy"]["limits"]["capital_micros"] += 1
    path.write_text(json.dumps(value))
    with pytest.raises(ControlError, match="integrity"):
        cached_settings(store.root)


def test_settings_require_both_read_permissions_and_recheck_revocation(store):
    with pytest.raises(ControlError, match="capability"):
        store.settings("status-only")
    with store.connection(write=True) as conn:
        conn.execute("DELETE FROM clients WHERE id='observer'")
    with pytest.raises(ControlError, match="capability"):
        store.settings("observer")
    with pytest.raises(ControlError, match="unknown"):
        mcp_connection(store, "observer")


def test_export_refuses_symlink_and_connection_binds_existing_identity(store, tmp_path):
    destination = tmp_path / "unrelated.json"
    destination.write_text("untouched")
    (store.root / "settings.json").symlink_to(destination)
    with pytest.raises(ControlError, match="symlink"):
        export_settings(store, "observer")
    assert destination.read_text() == "untouched"
    config = mcp_connection(store, "observer")["mcpServers"]["helios-alpha"]
    assert config["args"][-2:] == ["--client", "observer"]
    assert "env" not in config


def test_recovered_golem_identity_is_explicit_and_validated(tmp_path):
    root = tmp_path / "private"
    root.mkdir(mode=0o700)
    config = root / "golem-connection.json"
    value = {
        "schema_version": 1,
        "executable": "/usr/bin/golem",
        "account_id": "test",
        "phantom_id": "a82fbfdc-39f0-422c-817a-3f9482e8af7e",
    }
    config.write_text(json.dumps(value))
    config.chmod(0o600)
    bound = GolemControlStore(root)
    assert bound.agent_address.endswith('CryptoControlAgent("test")[' + value["phantom_id"] + "]")
    value["phantom_id"] = 'bad"]'
    config.write_text(json.dumps(value))
    with pytest.raises(ControlError, match="recovery identity"):
        GolemControlStore(root)


@pytest.mark.parametrize("profile", [None, "isolated-proof"])
def test_golem_calls_target_the_selected_profile_without_manifest_discovery(
    tmp_path, monkeypatch, profile
):
    root = tmp_path / "private"
    root.mkdir(mode=0o700)
    config = root / "golem-connection.json"
    value = {"schema_version": 1, "executable": "/usr/bin/golem", "account_id": "test"}
    if profile is not None:
        value["profile"] = profile
    config.write_text(json.dumps(value))
    config.chmod(0o600)
    calls = []

    def invoke(command, **kwargs):
        calls.append((command, kwargs))
        return SimpleNamespace(
            returncode=0,
            stdout=json.dumps(
                {
                    "result_json": {
                        "typ": {"type": "Result", "ok": {"type": "Str"}, "err": {"type": "Str"}},
                        "value": {"ok": '{"mode":"paper"}'},
                    }
                }
            ),
        )

    monkeypatch.setattr("helios_alpha.control.golem_store.subprocess.run", invoke)
    assert GolemControlStore(root).status("observer") == {"mode": "paper"}
    command, kwargs = calls[0]
    assert command[command.index("--profile") + 1] == (profile or "local")
    assert command[command.index("--config-dir") + 1] == str(root / "golem-cli")
    assert "--disable-app-manifest-discovery" in command
    assert kwargs["cwd"] == root


@pytest.mark.parametrize("profile", ["", "--cloud", "../global", "has space", True, None, 42])
def test_invalid_golem_profile_is_rejected_before_invocation(tmp_path, profile):
    root = tmp_path / "private"
    root.mkdir(mode=0o700)
    config = root / "golem-connection.json"
    config.write_text(
        json.dumps(
            {
                "schema_version": 1,
                "executable": "/usr/bin/golem",
                "account_id": "test",
                "profile": profile,
            }
        )
    )
    config.chmod(0o600)
    with pytest.raises(ControlError, match="invalid Golem profile"):
        GolemControlStore(root)
