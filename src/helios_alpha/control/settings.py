"""Versioned settings reads, private inspection exports, and local MCP connection config."""

from __future__ import annotations

import fcntl
import hashlib
import os
import sys
import tempfile
import time

from .golem_store import GolemControlStore, read_private_json
from .store import ControlError, canonical, private_root


def export_settings(store: GolemControlStore, client: str) -> dict:
    """Export a coherent Golem read. This file is never an execution authority."""
    root = private_root(store.root)
    lock_fd = os.open(root / "settings-export.lock", os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
    with os.fdopen(lock_fd, "a+") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        settings = store.settings(client)
        envelope = {
            "schema_version": 1,
            "read_only_export": True,
            "execution_authority": False,
            "exported_ns": time.time_ns(),
            "settings": settings,
        }
        envelope["sha256"] = hashlib.sha256(canonical(envelope).encode()).hexdigest()
        destination = root / "settings.json"
        if destination.is_symlink():
            raise ControlError("settings export must not be a symlink")
        fd, name = tempfile.mkstemp(prefix=".settings-", dir=root)
        try:
            with os.fdopen(fd, "w") as file:
                file.write(canonical(envelope) + "\n")
                file.flush()
                os.fsync(file.fileno())
            os.replace(name, destination)
            directory = os.open(root, os.O_RDONLY)
            try:
                os.fsync(directory)
            finally:
                os.close(directory)
        finally:
            if os.path.exists(name):
                os.unlink(name)
    return {"path": str(destination), **envelope}


def cached_settings(root) -> dict:
    """Explicit inspection-only offline read. Callers must never trade from this export."""
    envelope = read_private_json(private_root(root) / "settings.json")
    digest = envelope.pop("sha256", None)
    if (
        envelope.get("schema_version") != 1
        or envelope.get("read_only_export") is not True
        or envelope.get("execution_authority") is not False
        or hashlib.sha256(canonical(envelope).encode()).hexdigest() != digest
        or type(envelope.get("exported_ns")) is not int
        or envelope["exported_ns"] > time.time_ns()
    ):
        raise ControlError("settings export failed integrity/version checks")
    return {
        **envelope,
        "sha256": digest,
        "cache_age_seconds": (time.time_ns() - envelope["exported_ns"]) // 1_000_000_000,
        "freshness": "unverified; use get_settings or the settings command for a live read",
    }


def mcp_connection(store: GolemControlStore, client: str) -> dict:
    """An existing, durable role is bound at process launch, never selected by a tool call."""
    store.capabilities(client)  # Unknown or revoked identities cannot receive a connection config.
    return {
        "mcpServers": {
            "helios-alpha": {
                "command": os.path.abspath(sys.executable),
                "args": [
                    "-m",
                    "helios_alpha.control.mcp_server",
                    "--root",
                    str(store.root),
                    "--client",
                    client,
                ],
            }
        }
    }
