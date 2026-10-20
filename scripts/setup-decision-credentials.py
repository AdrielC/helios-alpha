#!/usr/bin/env python3
"""User-operated hidden-input setup. Never send API keys through chat or command arguments."""

import argparse
import getpass
import hmac
import json
import os
import secrets
import tempfile
import time
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path
from urllib.parse import parse_qs


def save_credentials(destination, values):
    """Atomically merge credentials into an owner-only file outside the repository."""
    destination.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    if destination.is_symlink():
        raise ValueError("Credential path must not be a symlink.")
    previous = json.loads(destination.read_text()) if destination.exists() else {}
    if not isinstance(previous, dict):
        raise ValueError("Existing credentials must be an object.")
    previous.update(values)
    fd, temporary = tempfile.mkstemp(prefix=".credentials-", dir=destination.parent)
    try:
        with os.fdopen(fd, "w") as output:
            os.fchmod(output.fileno(), 0o600)
            json.dump(previous, output)
            output.write("\n")
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, destination)
        directory = os.open(destination.parent, os.O_RDONLY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def valid_secret(value):
    return (
        isinstance(value, str)
        and 0 < len(value) <= 8192
        and not value.startswith("op://")
        and not any(char.isspace() or ord(char) < 32 or ord(char) == 127 for char in value)
    )


class LocalSetupServer(HTTPServer):
    """A short-lived loopback form with no command execution or remote access."""

    def __init__(self, destination, lifetime=300):
        self.destination = destination
        self.token = secrets.token_urlsafe(32)
        self.completed = False
        self.deadline = time.monotonic() + lifetime
        super().__init__(("127.0.0.1", 0), LocalSetupHandler)
        self.timeout = 1
        self.origin = f"http://127.0.0.1:{self.server_port}"
        self.path = f"/setup/{self.token}"

    def get_request(self):
        connection, address = super().get_request()
        connection.settimeout(2)
        return connection, address


class LocalSetupHandler(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass  # Never log credential requests, form bodies, or the setup capability.

    def respond(self, status, body=b"", location=None):
        self.send_response(status)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        # no-referrer makes browsers send Origin: null on ordinary form POSTs.
        # same-origin preserves the Origin check without disclosing the setup URL
        # to another origin. https://fetch.spec.whatwg.org/#origin-header
        self.send_header("Referrer-Policy", "same-origin")
        self.send_header("X-Content-Type-Options", "nosniff")
        self.send_header(
            "Content-Security-Policy",
            "default-src 'none'; form-action 'self'; frame-ancestors 'none'; base-uri 'none'",
        )
        self.send_header("Connection", "close")
        if location is not None:
            self.send_header("Location", location)
        self.end_headers()
        self.wfile.write(body)
        self.close_connection = True

    def permitted(self):
        # Exact Host comparison also rejects DNS rebinding through another hostname.
        return (
            self.client_address[0] == "127.0.0.1"
            and self.headers.get_all("Host") == [self.server.origin.removeprefix("http://")]
            and time.monotonic() < self.server.deadline
        )

    def do_GET(self):
        if not self.permitted() or self.path != self.server.path:
            self.respond(404)
            return
        if self.server.completed:
            self.respond(410)
            return
        token = self.server.token  # URL-safe generated text; no user data enters the page.
        page = f"""<!doctype html>
<meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Helios Alpha credential setup</title>
<h1>Helios Alpha: OpenAI key</h1>
<p>This temporary form runs only on this Mac and expires after five minutes.</p>
<p>Saving stores the key in a private local file for the Helios decision runner.
It does not call OpenAI or place a trade.</p>
<form method="post" action="/setup/{token}" autocomplete="off">
<input type="hidden" name="csrf" value="{token}">
<label>OpenAI API key <input type="password" name="key" autocomplete="off"
autofocus required maxlength="8192"></label>
<button type="submit">Save key locally</button>
</form>"""
        self.respond(200, page.encode())

    def do_POST(self):
        if (
            not self.permitted()
            or self.path != self.server.path
            or self.headers.get_all("Origin") != [self.server.origin]
            or self.headers.get("Content-Type", "").split(";")[0]
            != "application/x-www-form-urlencoded"
            or self.headers.get("Transfer-Encoding") is not None
        ):
            self.respond(403)
            return
        if self.server.completed:
            self.respond(410)
            return
        try:
            lengths = self.headers.get_all("Content-Length", [])
            if len(lengths) != 1:
                raise ValueError()
            length = int(lengths[0])
            if not 0 < length <= 32768:
                raise ValueError()
            body = self.rfile.read(length)
            if len(body) != length:
                raise ValueError()
            values = parse_qs(
                body.decode("utf-8"), strict_parsing=True, keep_blank_values=True, max_num_fields=2
            )
            if set(values) != {"csrf", "key"} or any(len(v) != 1 for v in values.values()):
                raise ValueError()
            if not hmac.compare_digest(values["csrf"][0].encode(), self.server.token.encode()):
                raise ValueError()
            key = values["key"][0]
            if not valid_secret(key):
                raise ValueError()
        except (ValueError, UnicodeError, TimeoutError):
            self.respond(400, b"Invalid submission. No credentials were saved.")
            return
        try:
            save_credentials(self.server.destination, {"OPENAI_API_KEY": key})
        except (ValueError, OSError):
            self.respond(500, b"Could not save credentials. No details were logged.")
            return
        self.server.completed = True
        self.respond(
            200,
            b"<!doctype html><title>Helios setup complete</title>"
            b"<h1>Key saved locally</h1><p>The setup form is now closed. "
            b"Provider access has not been validated yet.</p>",
        )


def browser_setup(destination):
    with LocalSetupServer(destination) as server:
        print(f"Open on this Mac: {server.origin}{server.path}", flush=True)
        print("Loopback only; expires in five minutes. No API key is logged.", flush=True)
        while not server.completed and time.monotonic() < server.deadline:
            server.handle_request()
        if not server.completed:
            raise SystemExit("Setup expired without saving a credential.")
    print("Saved locally with owner-only permissions. The setup listener is closed.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--provider", choices=["openai", "jev", "clef"], default="openai")
    parser.add_argument(
        "--browser", action="store_true", help="Use a five-minute, loopback-only OpenAI setup form."
    )
    args = parser.parse_args()
    destination = Path.home() / ".config/helios-alpha/decision-credentials.json"
    if args.browser:
        if args.provider != "openai":
            parser.error("--browser currently supports OpenAI only.")
        browser_setup(destination)
        return
    values = {}
    names = {"openai": "OPENAI_API_KEY", "jev": "TYPESAFE_API_KEY", "clef": "CLOUDFLARE_API_TOKEN"}
    urls = {
        "openai": "https://platform.openai.com/api-keys",
        "jev": "https://platform.typesafe.ai",
        "clef": "https://dash.cloudflare.com/profile/api-tokens",
    }
    print(f"Create your API key in your provider account: {urls[args.provider]}")
    print("Paste it below. Input is hidden and saved outside the repository.")
    # getpass fails closed when no real terminal exists; never fall back to echoed input.
    import warnings

    with warnings.catch_warnings():
        warnings.simplefilter("error", getpass.GetPassWarning)
        secret = getpass.getpass(f"{names[args.provider]}: ").strip()
    if not valid_secret(secret):
        raise SystemExit("Expected a nonempty API key without whitespace.")
    values[names[args.provider]] = secret
    if args.provider == "clef":
        account = input("Cloudflare account ID: ").strip()
        if len(account) != 32 or any(c not in "0123456789abcdefABCDEF" for c in account):
            raise SystemExit("Expected a 32-character hexadecimal account ID.")
        values["CLOUDFLARE_ACCOUNT_ID"] = account
    save_credentials(destination, values)
    print(f"Saved locally with owner-only permissions: {destination}")
    print("Connection has not been validated yet. Run helios-decision --smoke next.")


if __name__ == "__main__":
    main()
