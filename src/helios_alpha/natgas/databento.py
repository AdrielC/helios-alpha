"""Small Databento historical client with explicit, single-use download approval.

Metadata requests are free. A streaming request can be billed, including a failed
or interrupted request, so it is never retried automatically. The estimate gate
is not a provider-enforced dollar cap. No quote here grants trading authority.
"""

from __future__ import annotations

import json
import os
import time
from decimal import Decimal, InvalidOperation
from pathlib import Path

import httpx

from helios_alpha.control.broker_sync import write_private
from helios_alpha.control.store import canonical, private_root

from .history import MAX_FILE_BYTES, HistoryQuery, archive_blob, csv_rows, timestamp_ns
from .lab import digest

GATEWAY = "https://hist.databento.com"


class HistoryError(ValueError):
    """Value-free provider errors: never log request headers or response bodies."""


def load_key(root: Path):
    value = os.environ.get("DATABENTO_API_KEY")
    if value is None:
        path = private_root(root) / "market-data-credentials.json"
        if path.is_symlink() or not path.is_file() or path.stat().st_mode & 0o077:
            raise HistoryError("Databento key is not configured privately")
        if path.stat().st_size > 16384 or path.stat().st_uid != os.getuid():
            raise HistoryError("invalid credential file")
        value = json.loads(path.read_text()).get("DATABENTO_API_KEY")
    if not isinstance(value, str) or not 8 <= len(value) <= 4096 or any(c.isspace() for c in value):
        raise HistoryError("Databento key is not configured")
    return value


def dollars(value):
    if type(value) not in (str, int, float, Decimal):
        raise HistoryError("invalid provider cost estimate")
    try:
        amount = Decimal(str(value))
    except InvalidOperation:
        raise HistoryError("invalid provider cost estimate") from None
    if not amount.is_finite() or amount < 0:
        raise HistoryError("invalid provider cost estimate")
    return amount


class DatabentoHistory:
    def __init__(self, key, *, transport=None):
        self.client = httpx.Client(
            base_url=GATEWAY,
            auth=httpx.BasicAuth(key, ""),
            timeout=httpx.Timeout(60, connect=15),
            follow_redirects=False,
            transport=transport,
        )

    def close(self):
        self.client.close()

    def metadata(self, method, query):
        if method not in {"get_record_count", "get_cost"}:
            raise HistoryError("unsupported metadata operation")
        try:
            response = self.client.post(f"/v0/metadata.{method}", data=query.params())
            if response.status_code != 200 or len(response.content) > 16384:
                raise HistoryError("Databento metadata unavailable; check access and query")
            return response.json()
        except (httpx.HTTPError, json.JSONDecodeError):
            raise HistoryError("Databento metadata request failed") from None

    def estimate(self, query: HistoryQuery, *, now_ns=None):
        now_ns = time.time_ns() if now_ns is None else now_ns
        if timestamp_ns(query.end) > now_ns:
            raise HistoryError("only completed historical windows can be estimated")
        count = self.metadata("get_record_count", query)
        if type(count) is not int or not 0 < count < query.record_limit:
            raise HistoryError("empty query or record cap reached; narrow the window or symbols")
        cost = dollars(self.metadata("get_cost", query))
        estimate = {
            "schema_version": 1,
            "query": query.model_dump(),
            "query_digest": digest(query.model_dump()),
            "records": count,
            "estimated_cost_usd": str(cost),
            "estimated_at_ns": now_ns,
            "expires_at_ns": now_ns + 3600 * 1_000_000_000,
            "budget_enforcement": "estimate comparison; not a provider-enforced dollar cap",
            "execution_authority": False,
        }
        return {**estimate, "estimate_digest": digest(estimate)}

    def fetch(self, root, estimate, approved_usd, *, now_ns=None):
        """Require the exact reviewed estimate and fresh matching metadata before billing.

        Durable claim precedes network I/O. Failed requests require a new estimate,
        preventing a restart/retry from silently charging for the same stream again.
        """
        root = private_root(root)
        now_ns = time.time_ns() if now_ns is None else now_ns
        body = {k: v for k, v in estimate.items() if k != "estimate_digest"}
        if estimate.get("estimate_digest") != digest(body):
            raise HistoryError("estimate integrity mismatch")
        if not estimate["estimated_at_ns"] <= now_ns < estimate["expires_at_ns"]:
            raise HistoryError("estimate expired; request a new estimate")
        query = HistoryQuery(**estimate["query"])
        if estimate["query_digest"] != digest(query.model_dump()):
            raise HistoryError("query integrity mismatch")
        if dollars(estimate["estimated_cost_usd"]) > dollars(approved_usd):
            raise HistoryError("estimate exceeds approved amount")
        fresh = self.estimate(query, now_ns=now_ns)
        if fresh["records"] != estimate["records"] or dollars(
            fresh["estimated_cost_usd"]
        ) != dollars(estimate["estimated_cost_usd"]):
            raise HistoryError("data volume or price changed; review a new estimate")
        directory = private_root(root / "options-history", create=True)
        claim = directory / f"download-{estimate['estimate_digest']}.json"
        try:
            fd = os.open(claim, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
        except FileExistsError:
            raise HistoryError(
                "this estimate was already used; inspect its download receipt"
            ) from None
        with os.fdopen(fd, "w") as stream:
            stream.write(
                canonical({"status": "started", "estimate": estimate, "started_ns": now_ns})
            )
            stream.flush()
            os.fsync(stream.fileno())
        directory_fd = os.open(directory, os.O_RDONLY)
        try:
            os.fsync(directory_fd)
        finally:
            os.close(directory_fd)
        params = {
            **query.params(),
            "encoding": "csv",
            "compression": "none",
            "stype_out": "instrument_id",
            "pretty_px": "false",
            "pretty_ts": "false",
            "map_symbols": "true",
            "limit": str(query.record_limit),
        }
        raw = bytearray()
        try:
            with self.client.stream("POST", "/v0/timeseries.get_range", data=params) as response:
                if response.status_code != 200:
                    raise HistoryError("Databento download unavailable; request was not retried")
                for chunk in response.iter_bytes():
                    if len(raw) + len(chunk) > MAX_FILE_BYTES:
                        raise HistoryError(
                            "download exceeded local size bound; request was not retried"
                        )
                    raw.extend(chunk)
            original = archive_blob(root, bytes(raw), "csv")
            count = sum(1 for _ in csv_rows(bytes(raw), {"ts_event", "rtype"}, query.record_limit))
            if count != estimate["records"] or count >= query.record_limit:
                raise HistoryError(
                    "record count mismatch; original archived, incomplete data rejected"
                )
            receipt = {
                "status": "downloaded",
                "estimate": estimate,
                "raw_sha256": original.stem,
                "records": count,
                "received_ns": time.time_ns(),
                "billing": "estimate only; consult provider usage for actual charge or credit",
            }
            write_private(claim, receipt)
            return original, receipt
        except (httpx.HTTPError, ValueError, UnicodeError):
            # Retain the bounded partial payload as well as the durable attempt marker.
            partial = archive_blob(root, bytes(raw), "csv") if raw else None
            write_private(
                claim,
                {
                    "status": "failed",
                    "estimate": estimate,
                    "partial_sha256": partial.stem if partial else None,
                    "error": "download incomplete or failed; may have incurred a charge",
                },
            )
            raise HistoryError(
                "download failed; inspect private receipt before any new request"
            ) from None
