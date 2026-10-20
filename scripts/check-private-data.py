#!/usr/bin/env python3
"""Local Git guard. Reports paths/reasons only; never prints matched secret values.

Scan the index before commits and reachable history before pushes. This is defense in
depth, not a guarantee against every secret format or deliberately bypassed Git hooks.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
from pathlib import Path, PurePosixPath

PATTERNS = {
    "OpenAI credential": rb"\bsk-(?:proj-|svcacct-)?[A-Za-z0-9_-]{32,}",
    "GitHub credential": rb"\b(?:gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{30,})",
    "private key": rb"-----BEGIN (?:[A-Z0-9 ]+ )?PRIVATE KEY-----",
    "Robinhood credential": rb"\brh_[A-Za-z0-9_-]{32,}",
    "AWS access key": rb"\b(?:AKIA|ASIA)[A-Z0-9]{16}\b",
}
PRIVATE_PREFIXES = (
    "private/",
    ".private/",
    "private-strategies/",
    "strategies/local/",
    "config/local/",
    "data/crypto/",
    "data/decisions/",
    "data/shadow/",
)


def git(*args: str, cwd: Path | None = None) -> bytes:
    return subprocess.check_output(["git", *args], cwd=cwd, stderr=subprocess.PIPE)


def private_path(name: str) -> bool:
    leaf = PurePosixPath(name).name.lower()
    return (
        name.startswith(PRIVATE_PREFIXES)
        or leaf in {".env", ".envrc"}
        or (leaf.startswith(".env.") and leaf != ".env.example")
        or ("credentials" in leaf and leaf.endswith(".json"))
        or leaf.endswith((".pem", ".key", ".sqlite", ".sqlite3"))
        or ".sqlite3-" in leaf
    )


def known_credentials() -> list[bytes]:
    """Read only Helios-owned credential files; never enumerate the user's vault."""
    result: list[bytes] = []
    config = Path.home() / ".config/helios-alpha"
    private = Path.home() / ".local/share/helios-alpha/private"
    for path in (
        config / "decision-credentials.json",
        config / "robinhood-credentials.json",
        config / "robinhood-signing-credentials.json",
        private / "broker-sync-oauth-tokens.json",
        private / "broker-sync-oauth-client.json",
    ):
        if not path.exists():
            continue
        if path.is_symlink() or path.stat().st_mode & 0o077 or path.stat().st_uid != os.getuid():
            raise ValueError("Helios credential file permissions are not private")
        if path.stat().st_size > 64 * 1024:
            raise ValueError("Helios credential file exceeds size bound")
        value = json.loads(path.read_bytes())

        def walk(node: object) -> None:
            if isinstance(node, dict):
                for key, item in node.items():
                    if (
                        isinstance(item, str)
                        and len(item) >= 20
                        and key.lower() != "public_key_base64"
                        and any(word in key.lower() for word in ("key", "token", "secret"))
                    ):
                        result.append(item.encode())
                    elif isinstance(item, (dict, list)):
                        walk(item)
            elif isinstance(node, list):
                for item in node:
                    walk(item)

        walk(value)
    return result


def inspect_blob(body: bytes, secrets: list[bytes]) -> list[str]:
    reasons = [name for name, pattern in PATTERNS.items() if re.search(pattern, body)]
    if any(value in body for value in secrets):
        reasons.append("exact local Helios credential")
    return reasons


def scan(mode: str, secrets: list[bytes]) -> tuple[int, list[tuple[str, list[str]]]]:
    objects: dict[bytes, set[str]] = {}
    if mode == "staged":
        for record in git("ls-files", "--stage", "-z").split(b"\0"):
            if record:
                meta, raw_name = record.split(b"\t", 1)
                _, oid, _ = meta.split()
                objects.setdefault(oid, set()).add(os.fsdecode(raw_name))
    elif mode == "history":
        # Every reachable commit, not only HEAD or the last commit being pushed.
        commits = git("rev-list", "--all").splitlines()
        for commit in commits:
            for record in git("ls-tree", "-rz", commit.decode()).split(b"\0"):
                if not record:
                    continue
                meta, raw_name = record.split(b"\t", 1)
                _, kind, oid = meta.split()
                if kind == b"blob":
                    objects.setdefault(oid, set()).add(os.fsdecode(raw_name))
    else:
        raise ValueError("unknown scan mode")
    findings: list[tuple[str, list[str]]] = []
    for oid, names in objects.items():
        body = git("cat-file", "blob", oid.decode())
        reasons = inspect_blob(body, secrets)
        for name in sorted(names):
            path_reasons = ["private artifact path"] if private_path(name) else []
            if reasons or path_reasons:
                findings.append((name, path_reasons + reasons))
    return len(objects), findings


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--mode", choices=("staged", "history"), default="staged")
    args = parser.parse_args()
    try:
        count, findings = scan(args.mode, known_credentials())
    except (OSError, ValueError, subprocess.CalledProcessError):
        print("Privacy guard could not complete; commit/push blocked. No secret values displayed.")
        return 2
    print(f"Privacy guard: scanned {count} unique {args.mode} blobs; {len(findings)} findings.")
    for name, reasons in findings:
        # JSON quoting also prevents malicious filenames from injecting terminal controls.
        print(f"  {json.dumps(name)}: {', '.join(reasons)}")
    return 1 if findings else 0


if __name__ == "__main__":
    raise SystemExit(main())
