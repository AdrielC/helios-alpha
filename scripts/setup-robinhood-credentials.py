#!/usr/bin/env python3
"""Create local signing keys and install credentials without exposing secret values."""

from __future__ import annotations

import argparse
import base64
import getpass
import hashlib
import json
import os
import re
import stat
import subprocess
import tempfile
import warnings
from pathlib import Path

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

REPO = Path(__file__).resolve().parents[1]
CONFIG = Path.home() / ".config/helios-alpha"
KEY_FILE = "robinhood-signing-credentials.json"
FINAL_FILE = "robinhood-credentials.json"


class SetupError(Exception):
    """Safe, fixed messages only; never include provider output or credential values."""


def private_directory(path):
    if not path.is_absolute() or path.resolve().is_relative_to(REPO):
        raise SetupError("Use an absolute private directory outside the repository.")
    if not path.exists() and not path.is_symlink():
        path.mkdir(mode=0o700, parents=True)
    info = path.lstat()
    if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid() or info.st_mode & 0o077:
        raise SetupError("The credential directory must be private and owned by this user.")


def read_private(path):
    fd = None
    try:
        fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
        info = os.fstat(fd)
        if (
            not stat.S_ISREG(info.st_mode)
            or info.st_uid != os.getuid()
            or info.st_mode & 0o077
            or info.st_size > 65536
        ):
            raise SetupError("The local file must be private, user-owned, and bounded.")
        with os.fdopen(fd, "rb") as source:
            fd = None
            value = json.load(source)
        if not isinstance(value, dict):
            raise SetupError("The local file is not a valid settings object.")
        return value
    except (OSError, ValueError):
        raise SetupError("Could not read the private local file.") from None
    finally:
        if fd is not None:
            os.close(fd)


def create_private(path, data):
    """Publish a fully synced file without overwriting a competing or existing key."""
    private_directory(path.parent)
    fd, temporary = tempfile.mkstemp(prefix=".robinhood-setup-", dir=path.parent)
    try:
        with os.fdopen(fd, "w") as output:
            os.fchmod(output.fileno(), 0o600)
            json.dump(data, output)
            output.write("\n")
            output.flush()
            os.fsync(output.fileno())
        try:
            os.link(temporary, path)
        except FileExistsError:
            return False
        directory = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
        return True
    finally:
        os.unlink(temporary)


def decode_signer(encoded):
    try:
        raw = base64.b64decode(encoded, validate=True)
        if len(raw) != 32:
            raise ValueError()
        return Ed25519PrivateKey.from_private_bytes(raw)
    except (ValueError, TypeError):
        raise SetupError("Expected a base64-encoded 32-byte Ed25519 signing seed.") from None


def public_summary(signer):
    public = signer.public_key()
    challenge = b"Helios Alpha local signing-key validation"
    public.verify(signer.sign(challenge), challenge)
    raw = public.public_bytes_raw()
    return {
        "public_key_base64": base64.b64encode(raw).decode(),
        "public_key_sha256": hashlib.sha256(raw).hexdigest(),
        "broker_registration_verified": False,
        "execution_authority": False,
    }


def load_signer(config):
    private_directory(config)
    data = read_private(config / KEY_FILE)
    if set(data) != {"schema_version", "private_key_base64", "public_key_base64"} or (
        type(data["schema_version"]) is not int or data["schema_version"] != 1
    ):
        raise SetupError("The local signing-key record is invalid.")
    signer = decode_signer(data["private_key_base64"])
    if public_summary(signer)["public_key_base64"] != data["public_key_base64"]:
        raise SetupError("The local public key does not match its signing seed.")
    return signer


def initialize(config):
    private_directory(config)
    path = config / KEY_FILE
    if not path.exists() and not path.is_symlink():
        signer = Ed25519PrivateKey.generate()
        create_private(
            path,
            {
                "schema_version": 1,
                "private_key_base64": base64.b64encode(signer.private_bytes_raw()).decode(),
                "public_key_base64": public_summary(signer)["public_key_base64"],
            },
        )
    return public_summary(load_signer(config))


def read_reference(reference):
    if (
        not isinstance(reference, str)
        or not reference.startswith("op://")
        or len(reference) > 2048
        or len(reference.removeprefix("op://").split("/")) < 3
        or any(ord(c) < 32 or ord(c) == 127 for c in reference)
    ):
        raise SetupError("Provide a 1Password secret reference, not a secret value.")
    try:
        result = subprocess.run(
            ["op", "read", reference],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            timeout=8,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired):
        raise SetupError("1Password could not complete the authorized read.") from None
    if result.returncode or not 0 < len(result.stdout) <= 8192:
        raise SetupError("1Password did not return the requested credential.")
    try:
        return result.stdout.decode().rstrip("\r\n")
    except UnicodeError:
        raise SetupError("The credential must be text.") from None


def install(config, root, account_number, api_key, private_key_reference=None):
    private_directory(config)
    if not isinstance(account_number, str) or not re.fullmatch(
        r"[A-Za-z0-9]{1,128}", account_number
    ):
        raise SetupError("Supply the exact Trading API account number.")
    if (
        not isinstance(api_key, str)
        or not 1 <= len(api_key) <= 8192
        or api_key.startswith("op://")
        or any(c.isspace() or ord(c) < 32 or ord(c) == 127 for c in api_key)
    ):
        raise SetupError("The API key must be a nonempty value without whitespace.")
    binding = read_private(root / "broker-journal-connection.json")
    account_key = binding.get("account_key")
    if (
        type(binding.get("schema_version")) is not int
        or binding["schema_version"] != 1
        or not isinstance(account_key, str)
        or not re.fullmatch("[0-9a-f]{64}", account_key)
    ):
        raise SetupError("The local broker journal binding is invalid.")
    signer = (
        decode_signer(read_reference(private_key_reference))
        if private_key_reference
        else load_signer(config)
    )
    values = {
        "schema_version": 1,
        "account_number": account_number,
        "journal_account_key": account_key,
        "api_key": api_key,
        "private_key_base64": base64.b64encode(signer.private_bytes_raw()).decode(),
    }
    path = config / FINAL_FILE
    if not create_private(path, values) and read_private(path) != values:
        raise SetupError("Credentials already exist; this command does not replace them.")
    return {
        "state": "saved_unvalidated",
        "account_binding_verified": False,
        **public_summary(signer),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config-dir", type=Path, default=CONFIG)
    parser.add_argument(
        "--root", type=Path, default=Path.home() / ".local/share/helios-alpha/private"
    )
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser(
        "init-key", help="Create a persistent signing key; print only its public key."
    )
    commands.add_parser("public-key", help="Show the existing public key without replacing it.")
    install_parser = commands.add_parser(
        "install", help="Save credentials; does not call Robinhood."
    )
    install_parser.add_argument("--account-number", required=True)
    install_parser.add_argument(
        "--api-key-reference", help="1Password op:// reference, never a key."
    )
    install_parser.add_argument("--private-key-reference", help="For an existing API signing key.")
    args = parser.parse_args()
    os.umask(0o077)
    try:
        if args.command == "init-key":
            result = initialize(args.config_dir)
        elif args.command == "public-key":
            result = public_summary(load_signer(args.config_dir))
        else:
            if args.api_key_reference:
                key = read_reference(args.api_key_reference)
            else:
                with warnings.catch_warnings():
                    warnings.simplefilter("error", getpass.GetPassWarning)
                    key = getpass.getpass("Robinhood Trading API key (hidden): ")
            result = install(
                args.config_dir, args.root, args.account_number, key, args.private_key_reference
            )
        print(json.dumps(result, indent=2))
    except (SetupError, OSError, getpass.GetPassWarning, EOFError):
        raise SystemExit(
            "Setup could not complete. Check private-file permissions, input and vault access. "
            "No credential values were displayed; no broker requests were made."
        ) from None


if __name__ == "__main__":
    main()
