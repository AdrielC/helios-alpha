"""Offline reference store for isolated protocol tests, not the runtime authority.

Production CLI and MCP use GolemControlStore and never fall back to this database.

This is a local process boundary. A caller with unrestricted access to the same OS
account can also edit these files; MCP permissions do not sandbox that caller.
"""

from __future__ import annotations

import hashlib
import json
import os
import sqlite3
import stat
import time
from contextlib import contextmanager
from pathlib import Path

from .models import ClientGrant, PolicyUpdate, RiskLimits

DEFAULT_ROOT = Path.home() / ".local/share/helios-alpha/private"


class ControlError(Exception):
    """Safe, value-free diagnostic suitable for a tool response."""


def private_root(root: Path, *, create: bool = False) -> Path:
    root = root.expanduser().absolute()
    # Reject symlink components as well as a symlink at the final directory.
    if any(p.is_symlink() for p in (root, *root.parents)):
        raise ControlError("private state path must not contain symlinks")
    if any((p / ".git").exists() for p in (root, *root.parents)):
        raise ControlError("private state must be outside every Git working tree")
    if create:
        root.mkdir(parents=True, mode=0o700, exist_ok=True)
    info = root.stat()
    if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid() or info.st_mode & 0o077:
        raise ControlError("private state directory must be user-owned with mode 0700")
    return root


def canonical(value: object) -> str:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False)


class ControlStore:
    def __init__(self, root: Path = DEFAULT_ROOT):
        self.root = private_root(root)
        self.path = self.root / "control.sqlite3"
        self._check_file()

    def _check_file(self) -> None:
        info = self.path.lstat()
        if not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid() or info.st_mode & 0o077:
            raise ControlError("control database must be a private, user-owned regular file")

    @contextmanager
    def connection(self, *, write: bool = False):
        private_root(self.root)
        self._check_file()
        conn = sqlite3.connect(self.path.as_uri() + "?mode=rw", uri=True, timeout=5)
        conn.row_factory = sqlite3.Row
        try:
            conn.execute("PRAGMA foreign_keys=ON")
            conn.execute("PRAGMA synchronous=FULL")
            conn.execute("BEGIN IMMEDIATE" if write else "BEGIN")
            yield conn
            conn.commit()
        except BaseException:
            conn.rollback()
            raise
        finally:
            conn.close()
            for suffix in ("-wal", "-shm"):
                path = Path(str(self.path) + suffix)
                if path.exists() and not path.is_symlink():
                    try:
                        path.chmod(0o600)
                    except FileNotFoundError:
                        # SQLite removes these when its last connection closes;
                        # another reader may close between exists() and chmod().
                        pass

    @classmethod
    def initialize(
        cls, root: Path, limits: RiskLimits, clients: list[ClientGrant], *, actor: str
    ) -> ControlStore:
        root = private_root(root, create=True)
        path = root / "control.sqlite3"
        fd = os.open(path, os.O_CREAT | os.O_EXCL | os.O_WRONLY | os.O_NOFOLLOW, 0o600)
        os.close(fd)
        store = cls(root)
        # This is one-time setup: a failed setup is not silently retried/overwritten.
        with sqlite3.connect(path) as conn:
            conn.executescript("""
                PRAGMA journal_mode=WAL;
                PRAGMA synchronous=FULL;
                CREATE TABLE policy (
                    revision INTEGER PRIMARY KEY, limits TEXT NOT NULL,
                    digest TEXT NOT NULL, actor TEXT NOT NULL, reason TEXT NOT NULL,
                    created_ns INTEGER NOT NULL
                );
                CREATE TABLE execution (
                    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
                    paused INTEGER NOT NULL CHECK(paused IN (0,1)),
                    mode TEXT NOT NULL CHECK(mode IN ('shadow','paper','live'))
                );
                CREATE TABLE clients (
                    id TEXT PRIMARY KEY, capabilities TEXT NOT NULL
                );
                CREATE TABLE requests (
                    actor TEXT NOT NULL, id TEXT NOT NULL, digest TEXT NOT NULL,
                    response TEXT NOT NULL, PRIMARY KEY(actor,id)
                );
                CREATE TABLE audit (
                    sequence INTEGER PRIMARY KEY AUTOINCREMENT, created_ns INTEGER NOT NULL,
                    actor TEXT NOT NULL, action TEXT NOT NULL, details TEXT NOT NULL,
                    previous_hash TEXT NOT NULL, hash TEXT NOT NULL
                );
                CREATE TABLE runtime (
                    component TEXT PRIMARY KEY, updated_ns INTEGER NOT NULL, status TEXT NOT NULL
                );
            """)
        with store.connection(write=True) as conn:
            body = canonical(limits.model_dump())
            conn.execute(
                "INSERT INTO policy VALUES (1,?,?,?,?,?)",
                (
                    body,
                    hashlib.sha256(body.encode()).hexdigest(),
                    actor,
                    "initial pilot",
                    time.time_ns(),
                ),
            )
            conn.execute("INSERT INTO execution VALUES (1,0,'shadow')")
            for client in clients:
                conn.execute(
                    "INSERT INTO clients VALUES (?,?)",
                    (client.client_id, canonical(client.capabilities)),
                )
            store._audit(conn, actor, "initialize", {"revision": 1, "mode": "shadow"})
        return store

    @staticmethod
    def _audit(conn, actor: str, action: str, details: dict) -> None:
        previous = conn.execute("SELECT hash FROM audit ORDER BY sequence DESC LIMIT 1").fetchone()
        previous_hash = previous["hash"] if previous else "0" * 64
        now = time.time_ns()
        payload = canonical(
            {
                "at_ns": now,
                "actor": actor,
                "action": action,
                "details": details,
                "previous": previous_hash,
            }
        )
        conn.execute(
            "INSERT INTO audit(created_ns,actor,action,details,previous_hash,hash) "
            "VALUES (?,?,?,?,?,?)",
            (
                now,
                actor,
                action,
                canonical(details),
                previous_hash,
                hashlib.sha256(payload.encode()).hexdigest(),
            ),
        )

    @staticmethod
    def _require(conn, actor: str, capability: str) -> None:
        row = conn.execute("SELECT capabilities FROM clients WHERE id=?", (actor,)).fetchone()
        if row is None or capability not in json.loads(row["capabilities"]):
            raise ControlError("client lacks the required capability")

    def capabilities(self, actor: str) -> list[str]:
        with self.connection() as conn:
            row = conn.execute("SELECT capabilities FROM clients WHERE id=?", (actor,)).fetchone()
            if row is None:
                raise ControlError("unknown client")
            return json.loads(row["capabilities"])

    @staticmethod
    def _policy(conn) -> dict:
        row = conn.execute("SELECT * FROM policy ORDER BY revision DESC LIMIT 1").fetchone()
        if row is None:
            raise ControlError("risk policy missing")
        body = row["limits"]
        if hashlib.sha256(body.encode()).hexdigest() != row["digest"]:
            raise ControlError("risk policy integrity check failed")
        limits = RiskLimits.model_validate_json(body)
        return {
            "schema_version": 1,
            "revision": row["revision"],
            "limits": limits.model_dump(),
            "sha256": row["digest"],
            "updated_ns": row["created_ns"],
        }

    def policy(self, actor: str) -> dict:
        with self.connection() as conn:
            self._require(conn, actor, "risk:read")
            return self._policy(conn)

    def settings(self, actor: str) -> dict:
        """Offline test analogue: one transaction, never a production fallback."""
        with self.connection() as conn:
            self._require(conn, actor, "risk:read")
            self._require(conn, actor, "status:read")
            execution = conn.execute("SELECT * FROM execution WHERE singleton=1").fetchone()
            capabilities = conn.execute(
                "SELECT capabilities FROM clients WHERE id=?", (actor,)
            ).fetchone()
            return {
                "schema_version": 1,
                "authority": "offline-test-sqlite",
                "currency": "USD",
                "money_scale": 1_000_000,
                "policy": self._policy(conn),
                "execution": {
                    "mode": execution["mode"],
                    "paused": bool(execution["paused"]),
                    "live_admitted": False,
                    "live_blockers": ["offline_test_store"],
                },
                "client": {"id": actor, "capabilities": json.loads(capabilities["capabilities"])},
                "decisions": None,
                "paper_costs": None,
                "semantics": {"execution": "offline tests only"},
            }

    def status(self, actor: str) -> dict:
        with self.connection() as conn:
            self._require(conn, actor, "status:read")
            state = conn.execute("SELECT * FROM execution WHERE singleton=1").fetchone()
            if state is None:
                raise ControlError("execution state missing")
            return {
                "schema_version": 1,
                "mode": state["mode"],
                "paused": bool(state["paused"]),
                "policy_revision": self._policy(conn)["revision"],
                "live_admitted": False,
                "live_blockers": [
                    "dedicated_broker_credentials_not_validated",
                    "account_execution_recovery_not_certified",
                    "strategy_evidence_not_admitted",
                ],
                "runtime": [
                    {
                        "component": r["component"],
                        "updated_ns": r["updated_ns"],
                        "status": json.loads(r["status"]),
                    }
                    for r in conn.execute("SELECT * FROM runtime ORDER BY component")
                ],
            }

    def update_policy(self, actor: str, update: PolicyUpdate) -> dict:
        body = canonical(update.model_dump())
        digest = hashlib.sha256(body.encode()).hexdigest()
        with self.connection(write=True) as conn:
            self._require(conn, actor, "risk:write")
            existing = conn.execute(
                "SELECT digest,response FROM requests WHERE actor=? AND id=?",
                (actor, update.request_id),
            ).fetchone()
            if existing:
                if existing["digest"] != digest:
                    raise ControlError("request id already belongs to different contents")
                return json.loads(existing["response"])
            current = self._policy(conn)
            if current["revision"] != update.expected_revision:
                raise ControlError("risk policy revision changed; read it again")
            limits = canonical(update.limits.model_dump())
            revision = current["revision"] + 1
            conn.execute(
                "INSERT INTO policy VALUES (?,?,?,?,?,?)",
                (
                    revision,
                    limits,
                    hashlib.sha256(limits.encode()).hexdigest(),
                    actor,
                    update.reason,
                    time.time_ns(),
                ),
            )
            response = self._policy(conn)
            conn.execute(
                "INSERT INTO requests VALUES (?,?,?,?)",
                (actor, update.request_id, digest, canonical(response)),
            )
            # A risk change never arms live trading or clears a pause/loss circuit breaker.
            self._audit(
                conn, actor, "risk.update", {"revision": revision, "sha256": response["sha256"]}
            )
            return response

    def pause(self, actor: str) -> dict:
        with self.connection(write=True) as conn:
            self._require(conn, actor, "execution:pause")
            state = conn.execute("SELECT paused FROM execution WHERE singleton=1").fetchone()
            if not state["paused"]:
                conn.execute("UPDATE execution SET paused=1 WHERE singleton=1")
                self._audit(conn, actor, "execution.pause", {})
            return {
                "paused": True,
                "effect": "block new submissions; does not cancel existing orders",
            }

    def audit(self, actor: str, after: int = 0, limit: int = 100) -> list[dict]:
        if after < 0 or not 1 <= limit <= 200:
            raise ControlError("audit range is invalid")
        with self.connection() as conn:
            self._require(conn, actor, "audit:read")
            return [
                {**dict(row), "details": json.loads(row["details"])}
                for row in conn.execute(
                    "SELECT * FROM audit WHERE sequence>? ORDER BY sequence LIMIT ?", (after, limit)
                )
            ]

    def heartbeat(self, component: str, status: dict) -> None:
        """Runtime API, intentionally absent from MCP tools."""
        if component not in {"crypto-research", "crypto-shadow", "crypto-execution"}:
            raise ControlError("unknown runtime component")
        body = canonical(status)
        if len(body) > 4096:
            raise ControlError("runtime status exceeds size bound")
        with self.connection(write=True) as conn:
            conn.execute(
                "INSERT INTO runtime VALUES (?,?,?) ON CONFLICT(component) DO UPDATE SET "
                "updated_ns=excluded.updated_ns,status=excluded.status",
                (component, time.time_ns(), body),
            )
