"""Exercise the guard against actual Git index/history, including forced ignored files."""

import importlib.util
import subprocess
from pathlib import Path

import pytest

SPEC = importlib.util.spec_from_file_location(
    "privacy_guard", Path(__file__).parents[1] / "scripts/check-private-data.py"
)
assert SPEC and SPEC.loader
guard = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(guard)


@pytest.fixture
def repo(tmp_path, monkeypatch):
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True)
    monkeypatch.chdir(tmp_path)
    return tmp_path


def stage(repo, name, body):
    path = repo / name
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(body)
    subprocess.run(["git", "add", "-f", "--", name], check=True)


def test_reads_index_not_sanitized_worktree(repo):
    secret = b"secret-test-" + b"q" * 40
    stage(repo, "config.txt", secret)
    (repo / "config.txt").write_text("redacted")
    _, findings = guard.scan("staged", [secret])
    assert findings == [("config.txt", ["exact local Helios credential"])]
    assert secret.decode() not in repr(findings)


def test_forced_private_file_is_blocked(repo):
    stage(repo, "private-strategies/proprietary.py", b"signal = 1")
    _, findings = guard.scan("staged", [])
    assert findings[0][1] == ["private artifact path"]


def test_deleted_historical_secret_is_still_found(repo):
    secret = b"test-only-" + b"k" * 40
    stage(repo, "leak.txt", secret)
    command = ["git", "-c", "user.name=Test", "-c", "user.email=test@example.invalid"]
    subprocess.run([*command, "commit", "-qm", "fixture"], check=True)
    subprocess.run(["git", "rm", "-q", "leak.txt"], check=True)
    subprocess.run([*command, "commit", "-qm", "remove"], check=True)
    _, findings = guard.scan("history", [secret])
    assert findings == [("leak.txt", ["exact local Helios credential"])]


def test_provider_patterns_and_safe_example():
    example = b"sk-" + b"x" * 40
    assert guard.inspect_blob(example, []) == ["OpenAI credential"]
    assert guard.inspect_blob(b"api_key = os.environ['OPENAI_API_KEY']", []) == []
    assert not guard.private_path(".env.example")


def test_pending_robinhood_signing_seed_is_in_the_local_secret_guard(tmp_path, monkeypatch):
    monkeypatch.setattr(guard.Path, "home", lambda: tmp_path)
    folder = tmp_path / ".config/helios-alpha"
    folder.mkdir(parents=True)
    path = folder / "robinhood-signing-credentials.json"
    secret = "test-signing-seed-" + "s" * 32
    public = "test-public-key-" + "p" * 32
    path.write_text(
        '{"private_key_base64": "' + secret + '", "public_key_base64": "' + public + '"}'
    )
    path.chmod(0o600)
    assert secret.encode() in guard.known_credentials()
    assert public.encode() not in guard.known_credentials()
    assert guard.private_path(path.name)


def test_direct_broker_oauth_tokens_are_in_the_local_secret_guard(tmp_path, monkeypatch):
    import json

    monkeypatch.setattr(guard.Path, "home", lambda: tmp_path)
    folder = tmp_path / ".local/share/helios-alpha/private"
    folder.mkdir(parents=True)
    secret = "test-oauth-token-" + "s" * 32
    path = folder / "broker-sync-oauth-tokens.json"
    path.write_text(json.dumps({"access_token": secret}))
    path.chmod(0o600)
    assert secret.encode() in guard.known_credentials()
    assert guard.inspect_blob(secret.encode(), guard.known_credentials()) == [
        "exact local Helios credential"
    ]
