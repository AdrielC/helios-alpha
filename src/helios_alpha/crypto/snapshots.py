"""Private inputs for reproducing local proposals. No broker or model integration."""

from __future__ import annotations

import copy
import hashlib
import json
import os
import sqlite3
import tempfile

from helios_alpha.control.store import ControlError, canonical, private_root

from .evidence import HEX_ID, _check_file, _reader
from .strategy import PrivateStrategy

# The supported window contains 200 headlines of up to 500 characters and
# 2048-character URLs. JSON escaping can use 12 bytes per Unicode character;
# include room for the price history, paper fills and capture metadata.
MAX_BYTES = 8 * 1024 * 1024
MAX_RECORDS = 1000
MAX_ARCHIVE_BYTES = 48 * 1024 * 1024
MAX_VERSIONS = 64
RETENTION_MS = 30 * 86_400_000
DATABASE = "decision-inputs.sqlite3"
PHASES = {"evaluation", "entry_proposal", "exit_proposal"}


def _digest(value):
    return hashlib.sha256(canonical(value).encode()).hexdigest()


def _sync_directory(path):
    descriptor = os.open(path, os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def _immutable_file(path, body):
    """An interrupted write never publishes a partially written strategy version."""
    if path.exists() or path.is_symlink():
        _check_file(path)
        if path.stat().st_size != len(body) or path.read_bytes() != body:
            raise ControlError("archived strategy version differs from its fingerprint")
        return
    fd, temporary = tempfile.mkstemp(prefix=".capture-", dir=path.parent)
    try:
        with os.fdopen(fd, "wb") as handle:
            handle.write(body)
            handle.flush()
            os.fsync(handle.fileno())
        try:
            os.link(temporary, path)
        except FileExistsError:
            _check_file(path)
            if path.stat().st_size != len(body) or path.read_bytes() != body:
                raise ControlError("conflicting strategy archive") from None
        _sync_directory(path.parent)
    finally:
        os.unlink(temporary)


def _version(root, fingerprint):
    if not isinstance(fingerprint, str) or not HEX_ID.fullmatch(fingerprint):
        raise ControlError("invalid strategy fingerprint")
    return root / "strategy-versions" / fingerprint


def _decode(row):
    wire = row["data"]
    if len(wire.encode()) > MAX_BYTES:
        raise ControlError("decision input exceeds bound")
    data = json.loads(wire)
    if (
        _digest(data) != row["id"]
        or data.get("schema_version") != 1
        or data.get("phase") not in PHASES
    ):
        raise ControlError("decision input digest mismatch")
    return {**data, "sequence": row["sequence"], "snapshot_id": row["id"]}


class DecisionSnapshots:
    def __init__(self, root, strategy):
        self.root = private_root(root)
        self.fingerprint = strategy.fingerprint
        if (
            hashlib.sha256(strategy.source_bytes + strategy.parameter_bytes).hexdigest()
            != self.fingerprint
        ):
            raise ControlError("strategy archive fingerprint mismatch")
        parent = private_root(self.root / "strategy-versions", create=True)
        if (
            not (parent / self.fingerprint).exists()
            and sum(1 for _ in parent.iterdir()) >= MAX_VERSIONS
        ):
            raise ControlError("strategy archive version capacity reached")
        version = private_root(parent / self.fingerprint, create=True)
        _immutable_file(version / "strategy.py", strategy.source_bytes)
        _immutable_file(version / "parameters.json", strategy.parameter_bytes)
        _sync_directory(parent)
        _sync_directory(self.root)
        path = self.root / DATABASE
        if not path.exists() and not path.is_symlink():
            fd = os.open(path, os.O_CREAT | os.O_EXCL | os.O_WRONLY | os.O_NOFOLLOW, 0o600)
            os.close(fd)
        _check_file(path)
        self.db = sqlite3.connect(path.as_uri() + "?mode=rw", uri=True, timeout=0.1)
        self.db.row_factory = sqlite3.Row
        self.db.executescript(
            """
            PRAGMA journal_mode=WAL;
            PRAGMA synchronous=FULL;
            PRAGMA wal_autocheckpoint=256;
            PRAGMA journal_size_limit=8388608;
            PRAGMA max_page_count=16384;
            CREATE TABLE IF NOT EXISTS snapshots(
              sequence INTEGER PRIMARY KEY AUTOINCREMENT,
              id TEXT UNIQUE NOT NULL, capture_key TEXT UNIQUE NOT NULL,
              created_ms INTEGER NOT NULL, data TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS snapshot_meta(
              singleton INTEGER PRIMARY KEY CHECK(singleton=1), evicted INTEGER NOT NULL);
            INSERT OR IGNORE INTO snapshot_meta VALUES(1,0);
            """
        )

    def close(self):
        self.db.close()

    def capture(self, context_wire, proposal, phase, channel):
        if phase not in PHASES or not isinstance(context_wire, str):
            raise ValueError("invalid decision capture")
        if len(context_wire.encode()) > MAX_BYTES:
            raise ValueError("decision context exceeds bound")
        context = json.loads(context_wire)
        symbol, at = context["symbol"], context["now_ms"]
        if symbol not in {"BTC-USD", "ETH-USD"} or type(at) is not int or at < 0:
            raise ValueError("invalid decision context identity")
        ids = sorted(item["news_id"] for item in context["news"])
        if len(ids) > 200 or any(not HEX_ID.fullmatch(value) for value in ids):
            raise ValueError("invalid decision news identities")
        if phase == "evaluation":
            if not ids:
                return None
            anchor = {"news": ids, "proposal": proposal}
        elif phase == "entry_proposal":
            if proposal.get("action") != "enter" or proposal.get("news_id") not in ids:
                raise ValueError("entry capture lacks its headline")
            anchor = proposal["news_id"]
        else:
            if proposal.get("action") != "exit":
                raise ValueError("exit capture lacks an exit proposal")
            buys = [
                fill
                for fill in context["paper"]["recent_fills"]
                if fill["symbol"] == symbol and fill["side"] == "buy"
            ]
            anchor = {
                "buy": buys[-1].get("event_id") if buys else None,
                "held": context["paper"]["positions"].get(symbol, 0),
                "reason": proposal["reason"],
            }
        key = _digest([self.fingerprint, symbol, phase, anchor])
        existing = self.db.execute(
            "SELECT id FROM snapshots WHERE capture_key=?", (key,)
        ).fetchone()
        if existing:
            return existing["id"]
        data = {
            "schema_version": 1,
            "strategy_fingerprint": self.fingerprint,
            "phase": phase,
            "reference_channel": channel,
            "synthetic_inputs": channel == "synthetic"
            or any(item.get("synthetic") is True for item in context["news"]),
            "context": context,
            "proposal": proposal,
        }
        wire = canonical(data)
        size = len(wire.encode())
        if size > min(MAX_BYTES, MAX_ARCHIVE_BYTES):
            raise ValueError("decision capture exceeds bound")
        identity = _digest(data)
        with self.db:
            evicted = self.db.execute(
                "DELETE FROM snapshots WHERE created_ms<?", (at - RETENTION_MS,)
            ).rowcount
            count, used = self.db.execute(
                "SELECT COUNT(*),COALESCE(SUM(LENGTH(CAST(data AS BLOB))),0) FROM snapshots"
            ).fetchone()
            cutoff = None
            for row in self.db.execute(
                "SELECT sequence,LENGTH(CAST(data AS BLOB)) AS size "
                "FROM snapshots ORDER BY sequence"
            ):
                if count < MAX_RECORDS and used + size <= MAX_ARCHIVE_BYTES:
                    break
                cutoff = row["sequence"]
                count -= 1
                used -= row["size"]
            if cutoff is not None:
                evicted += self.db.execute(
                    "DELETE FROM snapshots WHERE sequence<=?", (cutoff,)
                ).rowcount
            self.db.execute("UPDATE snapshot_meta SET evicted=evicted+?", (evicted,))
            self.db.execute(
                "INSERT INTO snapshots(id,capture_key,created_ms,data) VALUES(?,?,?,?)",
                (identity, key, at, wire),
            )
        return identity


def read_decision_snapshots(store, client, *, limit=10, before_sequence=None):
    store.status(client)
    if (
        type(limit) is not int
        or not 1 <= limit <= 50
        or (
            before_sequence is not None
            and (type(before_sequence) is not int or before_sequence < 1)
        )
    ):
        raise ControlError("invalid snapshot page bounds")
    path = private_root(store.root) / DATABASE
    result = {
        "schema_version": 1,
        "execution_authority": False,
        "semantics": "Original local proposals; not model review, fill replay, or strategy P&L",
        "selection": (
            "First occurrence of each proposal per active news set; "
            "first entry proposal per headline; first exit proposal per position/reason"
        ),
        "retention": {
            "maximum_records": MAX_RECORDS,
            "maximum_input_bytes": MAX_BYTES,
            "maximum_payload_bytes": MAX_ARCHIVE_BYTES,
            "maximum_age_days_on_capture": RETENTION_MS // 86_400_000,
            "eviction": "oldest first; source versions remain separately archived",
        },
        "state": "not_recording",
        "snapshots": [],
        "next_before_sequence": None,
    }
    if not path.exists() and not path.is_symlink():
        return result
    with _reader(path) as db:
        rows = db.execute(
            "SELECT sequence,id,data FROM snapshots WHERE sequence<? "
            "ORDER BY sequence DESC LIMIT ?",
            (before_sequence if before_sequence is not None else 2**63 - 1, limit + 1),
        ).fetchall()
        result["state"] = "available"
        result["snapshot_count"] = db.execute("SELECT COUNT(*) FROM snapshots").fetchone()[0]
        if db.execute(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='snapshot_meta'"
        ).fetchone():
            result["evicted_snapshots"] = db.execute(
                "SELECT evicted FROM snapshot_meta"
            ).fetchone()[0]
        else:
            result["evicted_snapshots"] = None
        for row in rows[:limit]:
            item = _decode(row)
            result["snapshots"].append(
                {
                    key: item[key]
                    for key in (
                        "sequence",
                        "snapshot_id",
                        "strategy_fingerprint",
                        "phase",
                        "reference_channel",
                        "synthetic_inputs",
                    )
                }
                | {
                    "symbol": item["context"]["symbol"],
                    "at_ms": item["context"]["now_ms"],
                    "proposal": item["proposal"],
                }
            )
        if len(rows) > limit:
            result["next_before_sequence"] = rows[limit - 1]["sequence"]
    return result


def read_decision_input(store, client, snapshot_id):
    store.status(client)
    store.policy(client)
    if not isinstance(snapshot_id, str) or not HEX_ID.fullmatch(snapshot_id):
        raise ControlError("invalid snapshot identity")
    path = private_root(store.root) / DATABASE
    if not path.exists() and not path.is_symlink():
        raise ControlError("decision archive is unavailable")
    with _reader(path) as db:
        row = db.execute(
            "SELECT sequence,id,data FROM snapshots WHERE id=?", (snapshot_id,)
        ).fetchone()
        if row is None:
            raise ControlError("decision input not found")
        return {**_decode(row), "execution_authority": False}


def replay_local_proposal(root, snapshot, *, fee_bps=None):
    """Owner-invoked CLI replay of trusted private code. Never exposed as an MCP read tool."""
    root = private_root(root)
    if fee_bps is not None and (type(fee_bps) is not int or not 0 <= fee_bps <= 1000):
        raise ControlError("invalid fee scenario")
    version = _version(root, snapshot["strategy_fingerprint"])
    strategy = PrivateStrategy(
        version / "strategy.py",
        version / "parameters.json",
        expected_fingerprint=snapshot["strategy_fingerprint"],
    )
    context = copy.deepcopy(snapshot["context"])
    replayed = strategy.propose(context)
    result = {
        "schema_version": 1,
        "snapshot_id": snapshot["snapshot_id"],
        "execution_authority": False,
        "not_strategy_pnl": True,
        "scope": "local proposal only; model review and fills are not replayed",
        "matches_original": replayed == snapshot["proposal"],
        "expected_proposal": snapshot["proposal"],
        "replayed_proposal": replayed,
        "synthetic_inputs": snapshot["synthetic_inputs"],
    }
    if fee_bps is not None:
        if not result["matches_original"]:
            raise ControlError("original proposal did not reproduce; scenario withheld")
        context = copy.deepcopy(snapshot["context"])
        context["costs"]["fee_bps"] = fee_bps
        strategy = PrivateStrategy(
            version / "strategy.py",
            version / "parameters.json",
            expected_fingerprint=snapshot["strategy_fingerprint"],
        )
        result["fee_scenario"] = {
            "fee_bps": fee_bps,
            "proposal": strategy.propose(context),
            "changes_settings": False,
        }
    return result
