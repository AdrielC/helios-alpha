"""The loopback setup accepts exactly one same-origin, scoped credential transfer."""

import http.client
import importlib.util
import json
import threading
from pathlib import Path
from urllib.parse import urlencode

import pytest

spec = importlib.util.spec_from_file_location(
    "decision_credentials",
    Path(__file__).resolve().parents[1] / "scripts/setup-decision-credentials.py",
)
setup = importlib.util.module_from_spec(spec)
spec.loader.exec_module(setup)


@pytest.fixture
def form(tmp_path):
    destination = tmp_path / "private" / "credentials.json"
    server = setup.LocalSetupServer(destination)
    worker = threading.Thread(target=server.serve_forever, daemon=True)
    worker.start()
    try:
        yield server, destination
    finally:
        server.shutdown()
        worker.join()
        server.server_close()


def request(server, *, fields=None, headers=None, path=None, method="POST"):
    connection = http.client.HTTPConnection("127.0.0.1", server.server_port, timeout=3)
    body = urlencode(
        fields if fields is not None else {"csrf": server.token, "key": "fake-test-key"}
    )
    request_headers = {
        "Host": f"127.0.0.1:{server.server_port}",
        "Origin": server.origin,
        "Content-Type": "application/x-www-form-urlencoded",
    }
    request_headers.update(headers or {})
    connection.request(method, path or server.path, body, request_headers)
    result = connection.getresponse()
    response = (result.status, result.read(), dict(result.getheaders()))
    connection.close()
    return response


@pytest.mark.parametrize(
    "changes",
    [
        {"headers": {"Origin": "https://attacker.invalid"}},
        {"headers": {"Origin": "null"}},
        {"headers": {"Origin": ""}},
        {"headers": {"Host": "attacker.invalid"}},
        {"headers": {"Content-Type": "text/plain"}},
        {"path": "/setup/incorrect-capability"},
        {"fields": {"csrf": "incorrect-token", "key": "fake-test-key"}},
        {"fields": {"csrf": "non-ascii-\u2603", "key": "fake-test-key"}},
        {"fields": {"key": "fake-test-key"}},
        {"fields": {"csrf": "irrelevant", "key": "test", "extra": "field"}},
    ],
)
def test_untrusted_requests_never_write_credentials(form, changes):
    server, destination = form
    status, _, _ = request(server, **changes)
    assert status in (400, 403)
    assert not destination.exists()
    assert not server.completed


def test_one_successful_transfer_preserves_other_keys_and_never_echoes_secret(form):
    server, destination = form
    setup.save_credentials(destination, {"TYPESAFE_API_KEY": "fake-other-key"})
    secret = "fake-key-with-encoded-characters:/+?&="
    status, body, headers = request(server, fields={"csrf": server.token, "key": secret})
    assert status == 200
    assert secret.encode() not in body
    assert headers["Cache-Control"] == "no-store"
    assert "frame-ancestors 'none'" in headers["Content-Security-Policy"]
    assert json.loads(destination.read_text()) == {
        "TYPESAFE_API_KEY": "fake-other-key",
        "OPENAI_API_KEY": secret,
    }
    assert destination.stat().st_mode & 0o777 == 0o600
    assert server.completed
    assert request(server)[0] == 410
    assert json.loads(destination.read_text())["OPENAI_API_KEY"] == secret


@pytest.mark.parametrize(
    "secret",
    ["", "has space", "has\nnewline", "has\0nul", "x" * 8193, "op://Personal/Example/credential"],
)
def test_invalid_credential_is_not_written_or_echoed(form, secret):
    server, destination = form
    status, body, _ = request(server, fields={"csrf": server.token, "key": secret})
    assert status == 400
    assert not destination.exists()
    if secret:
        assert secret.encode() not in body


def test_expired_form_rejects_reads_and_writes(form):
    server, destination = form
    server.deadline = 0
    assert request(server, method="GET")[0] == 404
    assert request(server)[0] == 403
    assert not destination.exists()


def test_empty_form_never_contains_existing_credentials(form):
    server, destination = form
    setup.save_credentials(destination, {"OPENAI_API_KEY": "fake-existing-secret"})
    status, body, headers = request(server, method="GET")
    assert status == 200
    assert headers["Referrer-Policy"] == "same-origin"
    assert b'type="password"' in body
    assert b"fake-existing-secret" not in body
    assert not server.completed


def test_symlink_destination_is_never_followed(form, tmp_path):
    server, destination = form
    target = tmp_path / "unrelated.json"
    target.write_text('{"keep":"unchanged"}')
    destination.parent.mkdir()
    destination.symlink_to(target)
    assert request(server)[0] == 500
    assert target.read_text() == '{"keep":"unchanged"}'
    assert not server.completed
