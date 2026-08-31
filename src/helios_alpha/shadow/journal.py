from __future__ import annotations

import json
import sqlite3
from collections.abc import Iterable
from datetime import UTC, datetime
from pathlib import Path

from helios_alpha.shadow.models import (
    RetrievedJson,
    ShadowCandidate,
    ShadowCheckpoint,
    ShadowContractError,
    ShadowObservation,
    canonical_json,
)


def _encode_time(value: datetime) -> str:
    return value.astimezone(UTC).isoformat().replace("+00:00", "Z")


def _decode_time(value: str) -> datetime:
    return datetime.fromisoformat(value.replace("Z", "+00:00")).astimezone(UTC)


class ShadowJournal:
    """Append-only SQLite journal with atomic source checkpoint advancement."""

    def __init__(self, path: Path) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        self._connection = sqlite3.connect(path, isolation_level=None)
        self._connection.row_factory = sqlite3.Row
        self._connection.execute("PRAGMA journal_mode=WAL")
        self._connection.execute("PRAGMA synchronous=FULL")
        self._connection.execute("PRAGMA foreign_keys=ON")
        self._create_schema()

    def close(self) -> None:
        self._connection.close()

    def __enter__(self) -> ShadowJournal:
        return self

    def __exit__(self, *_: object) -> None:
        self.close()

    def _create_schema(self) -> None:
        self._connection.executescript(
            """
            CREATE TABLE IF NOT EXISTS shadow_snapshots (
                snapshot_id TEXT PRIMARY KEY,
                source_id TEXT NOT NULL,
                source_url TEXT NOT NULL,
                observed_at TEXT NOT NULL,
                body_sha256 TEXT NOT NULL,
                etag TEXT,
                last_modified TEXT
            );
            CREATE TABLE IF NOT EXISTS shadow_observations (
                sequence INTEGER PRIMARY KEY AUTOINCREMENT,
                source_id TEXT NOT NULL,
                source_offset INTEGER NOT NULL,
                natural_key TEXT NOT NULL,
                revision INTEGER NOT NULL,
                event_time TEXT NOT NULL,
                available_at TEXT NOT NULL,
                observed_at TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                payload_sha256 TEXT NOT NULL,
                source_url TEXT NOT NULL,
                snapshot_id TEXT NOT NULL REFERENCES shadow_snapshots(snapshot_id),
                quality_flags_json TEXT NOT NULL,
                UNIQUE(source_id, source_offset),
                UNIQUE(source_id, natural_key, revision)
            );
            CREATE INDEX IF NOT EXISTS shadow_observations_natural_key
                ON shadow_observations(source_id, natural_key, revision DESC);
            CREATE INDEX IF NOT EXISTS shadow_observations_available
                ON shadow_observations(source_id, available_at, source_offset);
            CREATE TABLE IF NOT EXISTS shadow_checkpoints (
                source_id TEXT PRIMARY KEY,
                last_offset INTEGER NOT NULL,
                observed_at TEXT NOT NULL,
                max_event_time TEXT,
                snapshot_id TEXT NOT NULL REFERENCES shadow_snapshots(snapshot_id),
                body_sha256 TEXT NOT NULL
            );
            """
        )

    def ingest(
        self,
        source_id: str,
        document: RetrievedJson,
        candidates: Iterable[ShadowCandidate],
    ) -> list[ShadowObservation]:
        if not source_id.strip():
            raise ShadowContractError("source_id must not be empty")
        ordered = sorted(
            candidates,
            key=lambda candidate: (
                candidate.available_at or document.observed_at,
                candidate.event_time,
                candidate.natural_key,
            ),
        )
        natural_keys = [candidate.natural_key for candidate in ordered]
        if len(set(natural_keys)) != len(natural_keys):
            raise ShadowContractError("one source snapshot emitted duplicate natural keys")
        for candidate in ordered:
            available_at = candidate.available_at or document.observed_at
            if available_at > document.observed_at:
                raise ShadowContractError("available_at must not exceed observed_at")

        inserted: list[ShadowObservation] = []
        connection = self._connection
        connection.execute("BEGIN IMMEDIATE")
        try:
            connection.execute(
                """
                INSERT OR IGNORE INTO shadow_snapshots
                    (
                        snapshot_id, source_id, source_url, observed_at,
                        body_sha256, etag, last_modified
                    )
                VALUES (?, ?, ?, ?, ?, ?, ?)
                """,
                (
                    document.snapshot_id,
                    source_id,
                    document.source_url,
                    _encode_time(document.observed_at),
                    document.body_sha256,
                    document.etag,
                    document.last_modified,
                ),
            )
            checkpoint = connection.execute(
                "SELECT last_offset, max_event_time FROM shadow_checkpoints WHERE source_id = ?",
                (source_id,),
            ).fetchone()
            last_offset = int(checkpoint["last_offset"]) if checkpoint else 0
            max_event_time = (
                _decode_time(str(checkpoint["max_event_time"]))
                if checkpoint and checkpoint["max_event_time"]
                else None
            )

            for candidate in ordered:
                previous = connection.execute(
                    """
                    SELECT revision, payload_sha256
                    FROM shadow_observations
                    WHERE source_id = ? AND natural_key = ?
                    ORDER BY revision DESC
                    LIMIT 1
                    """,
                    (source_id, candidate.natural_key),
                ).fetchone()
                if previous and previous["payload_sha256"] == candidate.payload_sha256:
                    continue
                revision = int(previous["revision"]) + 1 if previous else 1
                last_offset += 1
                available_at = candidate.available_at or document.observed_at
                quality_json = canonical_json(sorted(set(candidate.quality_flags)))
                cursor = connection.execute(
                    """
                    INSERT INTO shadow_observations (
                        source_id, source_offset, natural_key, revision, event_time,
                        available_at, observed_at, payload_json, payload_sha256,
                        source_url, snapshot_id, quality_flags_json
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                    """,
                    (
                        source_id,
                        last_offset,
                        candidate.natural_key,
                        revision,
                        _encode_time(candidate.event_time),
                        _encode_time(available_at),
                        _encode_time(document.observed_at),
                        candidate.payload_json,
                        candidate.payload_sha256,
                        document.source_url,
                        document.snapshot_id,
                        quality_json,
                    ),
                )
                max_event_time = (
                    max(max_event_time, candidate.event_time)
                    if max_event_time
                    else candidate.event_time
                )
                inserted.append(
                    ShadowObservation(
                        sequence=int(cursor.lastrowid),
                        source_id=source_id,
                        source_offset=last_offset,
                        natural_key=candidate.natural_key,
                        revision=revision,
                        event_time=candidate.event_time,
                        available_at=available_at,
                        observed_at=document.observed_at,
                        payload=candidate.payload,
                        payload_sha256=candidate.payload_sha256,
                        source_url=document.source_url,
                        snapshot_id=document.snapshot_id,
                        quality_flags=tuple(sorted(set(candidate.quality_flags))),
                    )
                )

            connection.execute(
                """
                INSERT INTO shadow_checkpoints (
                    source_id, last_offset, observed_at, max_event_time, snapshot_id, body_sha256
                ) VALUES (?, ?, ?, ?, ?, ?)
                ON CONFLICT(source_id) DO UPDATE SET
                    last_offset = excluded.last_offset,
                    observed_at = excluded.observed_at,
                    max_event_time = excluded.max_event_time,
                    snapshot_id = excluded.snapshot_id,
                    body_sha256 = excluded.body_sha256
                """,
                (
                    source_id,
                    last_offset,
                    _encode_time(document.observed_at),
                    _encode_time(max_event_time) if max_event_time else None,
                    document.snapshot_id,
                    document.body_sha256,
                ),
            )
            connection.execute("COMMIT")
        except Exception:
            connection.execute("ROLLBACK")
            raise
        return inserted

    def checkpoint(self, source_id: str) -> ShadowCheckpoint | None:
        row = self._connection.execute(
            "SELECT * FROM shadow_checkpoints WHERE source_id = ?", (source_id,)
        ).fetchone()
        if row is None:
            return None
        return ShadowCheckpoint(
            source_id=str(row["source_id"]),
            last_offset=int(row["last_offset"]),
            observed_at=_decode_time(str(row["observed_at"])),
            max_event_time=(
                _decode_time(str(row["max_event_time"])) if row["max_event_time"] else None
            ),
            snapshot_id=str(row["snapshot_id"]),
            body_sha256=str(row["body_sha256"]),
        )

    def observations_after(
        self, source_id: str, offset: int, limit: int = 1_000
    ) -> list[ShadowObservation]:
        if offset < 0 or not 1 <= limit <= 10_000:
            raise ShadowContractError("invalid observation cursor or limit")
        rows = self._connection.execute(
            """
            SELECT * FROM shadow_observations
            WHERE source_id = ? AND source_offset > ?
            ORDER BY source_offset
            LIMIT ?
            """,
            (source_id, offset, limit),
        ).fetchall()
        return [self._observation(row) for row in rows]

    def latest_observations(self, limit: int = 100_000) -> list[ShadowObservation]:
        if not 1 <= limit <= 1_000_000:
            raise ShadowContractError("invalid latest observation limit")
        rows = self._connection.execute(
            """
            SELECT * FROM (
                SELECT * FROM shadow_observations
                ORDER BY sequence DESC
                LIMIT ?
            )
            ORDER BY sequence
            """,
            (limit,),
        ).fetchall()
        latest: dict[tuple[str, str], sqlite3.Row] = {}
        for row in rows:
            latest[(str(row["source_id"]), str(row["natural_key"]))] = row
        return [
            self._observation(row)
            for row in sorted(latest.values(), key=lambda candidate: int(candidate["sequence"]))
        ]

    def latest_observed_at(self) -> datetime | None:
        row = self._connection.execute(
            "SELECT MAX(observed_at) AS observed_at FROM shadow_checkpoints"
        ).fetchone()
        if row is None or row["observed_at"] is None:
            return None
        return _decode_time(str(row["observed_at"]))

    @staticmethod
    def _observation(row: sqlite3.Row) -> ShadowObservation:
        return ShadowObservation(
            sequence=int(row["sequence"]),
            source_id=str(row["source_id"]),
            source_offset=int(row["source_offset"]),
            natural_key=str(row["natural_key"]),
            revision=int(row["revision"]),
            event_time=_decode_time(str(row["event_time"])),
            available_at=_decode_time(str(row["available_at"])),
            observed_at=_decode_time(str(row["observed_at"])),
            payload=json.loads(str(row["payload_json"])),
            payload_sha256=str(row["payload_sha256"]),
            source_url=str(row["source_url"]),
            snapshot_id=str(row["snapshot_id"]),
            quality_flags=tuple(json.loads(str(row["quality_flags_json"]))),
        )
