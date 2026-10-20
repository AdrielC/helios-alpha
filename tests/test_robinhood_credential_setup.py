import base64
import importlib.util
import json
import os
import subprocess
import sys
from pathlib import Path
from types import SimpleNamespace

import pytest

pytest.importorskip("cryptography")

SCRIPT = Path(__file__).parents[1] / "scripts/setup-robinhood-credentials.py"
SPEC = importlib.util.spec_from_file_location("robinhood_setup", SCRIPT)
setup = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(setup)


@pytest.fixture
def private(tmp_path):
    config, root = tmp_path / "config", tmp_path / "private"
    config.mkdir(mode=0o700)
    root.mkdir(mode=0o700)
    binding = root / "broker-journal-connection.json"
    binding.write_text(json.dumps({"schema_version": 1, "account_key": "a" * 64}))
    binding.chmod(0o600)
    return config, root


def test_key_generation_is_persistent_private_and_public_output_only(private):
    config, _ = private
    first = setup.initialize(config)
    path = config / setup.KEY_FILE
    before = path.read_bytes()
    assert setup.initialize(config) == first
    assert setup.public_summary(setup.load_signer(config)) == first
    assert path.read_bytes() == before
    assert path.stat().st_mode & 0o777 == 0o600
    assert config.stat().st_mode & 0o777 == 0o700
    seed = setup.read_private(path)["private_key_base64"]
    assert len(base64.b64decode(seed)) == 32
    assert seed not in json.dumps(first)
    assert not first["broker_registration_verified"]
    assert not first["execution_authority"]
    assert not (config / setup.FINAL_FILE).exists()


def test_signing_seed_is_compatible_with_ed25519_verification(private):
    config, _ = private
    result = setup.initialize(config)
    signer = setup.load_signer(config)
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey

    verifier = Ed25519PublicKey.from_public_bytes(base64.b64decode(result["public_key_base64"]))
    request = b"test-key1700000000/api/v1/crypto/trading/accounts/GET"
    verifier.verify(signer.sign(request), request)


def test_install_matches_native_backend_schema_and_cannot_replace_credentials(private):
    config, root = private
    setup.initialize(config)
    result = setup.install(config, root, "API123", "test-only-api-key")
    assert result["state"] == "saved_unvalidated"
    assert result["account_binding_verified"] is False
    assert "test-only-api-key" not in json.dumps(result)
    path = config / setup.FINAL_FILE
    before = path.read_bytes()
    from helios_alpha.crypto.order_recovery import NativeRecoveryBackend

    # Exercise the existing backend's real credential validator without constructing a transport.
    backend = object.__new__(NativeRecoveryBackend)
    backend.credentials = path
    backend.account_key = "a" * 64
    credentials = backend.credentials_checked()
    assert credentials["account_number"] == "API123"
    assert credentials["api_key"] == "test-only-api-key"
    assert setup.install(config, root, "API123", "test-only-api-key") == result
    with pytest.raises(setup.SetupError, match="already exist"):
        setup.install(config, root, "API123", "another-test-key")
    assert path.read_bytes() == before
    assert path.stat().st_mode & 0o777 == 0o600


def test_existing_signing_key_can_be_imported_without_generating_a_replacement(
    private, monkeypatch
):
    config, root = private
    encoded = base64.b64encode(bytes(range(32))).decode()
    read = []

    def secret(reference):
        read.append(reference)
        return encoded

    monkeypatch.setattr(setup, "read_reference", secret)
    result = setup.install(config, root, "API123", "test-key", "op://vault/item/private")
    assert read == ["op://vault/item/private"]
    assert setup.read_private(config / setup.FINAL_FILE)["private_key_base64"] == encoded
    assert encoded not in json.dumps(result)
    assert not (config / setup.KEY_FILE).exists()


@pytest.mark.parametrize(
    "kind", ["symlink", "public_file", "public_directory", "mismatched_public"]
)
def test_existing_keys_are_never_silently_replaced_or_repaired(private, tmp_path, kind):
    config, _ = private
    setup.initialize(config)
    path = config / setup.KEY_FILE
    if kind == "symlink":
        real = tmp_path / "original.json"
        path.rename(real)
        path.symlink_to(real)
    elif kind == "public_file":
        path.chmod(0o644)
    elif kind == "public_directory":
        config.chmod(0o755)
    else:
        value = setup.read_private(path)
        value["public_key_base64"] = base64.b64encode(bytes(32)).decode()
        path.write_text(json.dumps(value))
    before = path.read_bytes()
    with pytest.raises(setup.SetupError):
        setup.initialize(config)
    assert path.read_bytes() == before


def test_keys_cannot_be_created_inside_the_repository():
    with pytest.raises(setup.SetupError, match="outside"):
        setup.private_directory(setup.REPO / ".never-create-credentials")
    assert not (setup.REPO / ".never-create-credentials").exists()


def test_1password_uses_an_exact_reference_without_shell_or_diagnostics(monkeypatch):
    observed = []

    def run(args, **kwargs):
        observed.append((args, kwargs))
        return SimpleNamespace(returncode=0, stdout=b"test-secret\n")

    monkeypatch.setattr(setup.subprocess, "run", run)
    assert setup.read_reference("op://private/Robinhood/API key") == "test-secret"
    args, kwargs = observed[0]
    assert args == ["op", "read", "op://private/Robinhood/API key"]
    assert kwargs["stdin"] == kwargs["stderr"] == subprocess.DEVNULL
    assert kwargs["timeout"] == 8
    assert "shell" not in kwargs
    for value in ["test-secret", "op://only/item", "op://v/i/f\n"]:
        with pytest.raises(setup.SetupError):
            setup.read_reference(value)
    assert len(observed) == 1


def test_timeout_never_exposes_partial_secret_output(monkeypatch):
    def run(*args, **kwargs):
        raise subprocess.TimeoutExpired("op", 8, output=b"partial-secret")

    monkeypatch.setattr(setup.subprocess, "run", run)
    with pytest.raises(setup.SetupError) as error:
        setup.read_reference("op://vault/item/key")
    assert "partial-secret" not in str(error.value)


def test_cli_key_output_and_nonterminal_install_do_not_leak_or_save_secrets(private):
    config, root = private
    command = [sys.executable, str(SCRIPT), "--config-dir", str(config), "--root", str(root)]
    generated = subprocess.run([*command, "init-key"], capture_output=True, check=True, text=True)
    shown = subprocess.run([*command, "public-key"], capture_output=True, check=True, text=True)
    assert json.loads(generated.stdout) == json.loads(shown.stdout)
    seed = setup.read_private(config / setup.KEY_FILE)["private_key_base64"]
    assert seed not in generated.stdout + generated.stderr + shown.stdout + shown.stderr
    failed = subprocess.run(
        [*command, "install", "--account-number", "API123"],
        input="do-not-echo-this-secret",
        capture_output=True,
        text=True,
        check=False,
        env={**os.environ, "PYTHONWARNINGS": "default"},
    )
    assert failed.returncode != 0
    assert "do-not-echo-this-secret" not in failed.stdout + failed.stderr
    assert not (config / setup.FINAL_FILE).exists()
