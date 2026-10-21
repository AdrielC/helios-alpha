"""Historical OPRA NBBO ingestion. Originals and replay inputs remain outside Git.

Wire format: Databento CSV, cmbp-1, map_symbols=true, pretty_px/pretty_ts=false.
Protocol references: https://databento.com/docs/datasets/opra-pillar and the
Databento DBN Cmbp1Msg/FlagSet definitions. No OHLC or midpoint fill conversion.
"""

from __future__ import annotations

import csv
import hashlib
import io
import os
import re
import tempfile
from collections import Counter
from datetime import datetime
from pathlib import Path
from statistics import median
from typing import Literal

from pydantic import Field, model_validator

from helios_alpha.control.store import canonical, private_root

from .broker import regular_session
from .replay import Contract, Plan, Quote, ReplayDataset, Strict

NS_PER_MS = 1_000_000
NANODOLLARS_PER_CENT = 10_000_000
UNDEF_PRICE = 2**63 - 1
BAD_FLAGS = 4 | 8 | 32  # MAYBE_BAD_BOOK, BAD_TS_RECV, SNAPSHOT
MAX_FILE_BYTES = 64 * 1024 * 1024
OCC = re.compile(r"([A-Z][A-Z0-9 ]{5})(\d{6})([CP])(\d{8})")


def raw_contract(symbol: str):
    match = OCC.fullmatch(symbol)
    if not match or match[1] != "UNG   ":
        raise ValueError("expected a standard UNG OCC symbol; adjusted contracts unsupported")
    expiry = datetime.strptime(match[2], "%y%m%d").date().isoformat()
    strike_mills = int(match[4])
    if strike_mills <= 0 or strike_mills % 10:
        raise ValueError("unsupported option strike precision")
    return expiry, "call" if match[3] == "C" else "put", strike_mills // 10


def timestamp_ns(value: str):
    # Only explicit timezone ISO timestamps in query settings, never local machine time.
    import pandas as pd

    stamp = pd.Timestamp(value)
    if stamp.tzinfo is None:
        raise ValueError("query timestamps require an explicit timezone")
    return stamp.value


class HistoryQuery(Strict):
    schema_version: Literal[1] = 1
    provider: Literal["databento"] = "databento"
    dataset: Literal["OPRA.PILLAR"] = "OPRA.PILLAR"
    record_schema: Literal["cmbp-1", "definition"] = "cmbp-1"
    stype_in: Literal["raw_symbol", "parent"] = "raw_symbol"
    symbols: list[str] = Field(min_length=1, max_length=32)
    start: str
    end: str
    record_limit: int = Field(default=250000, ge=1, le=1000000)

    @model_validator(mode="after")
    def bounded(self):
        start, end = timestamp_ns(self.start), timestamp_ns(self.end)
        if not 0 < end - start <= 86400 * 1_000_000_000:
            raise ValueError("query must cover at most one day; use explicit daily jobs")
        if start < timestamp_ns("2023-03-28T00:00:00Z"):
            raise ValueError("intraday replay requires OPRA capture clocks from March 28, 2023")
        if len(set(self.symbols)) != len(self.symbols):
            raise ValueError("duplicate symbols")
        if self.stype_in == "parent":
            if self.record_schema != "definition" or self.symbols != ["UNG.OPT"]:
                raise ValueError("parent queries are limited to UNG contract discovery")
        else:
            for symbol in self.symbols:
                raw_contract(symbol)
        return self

    def params(self):
        return {
            "dataset": self.dataset,
            "schema": self.record_schema,
            "stype_in": self.stype_in,
            "symbols": ",".join(self.symbols),
            "start": str(timestamp_ns(self.start)),
            "end": str(timestamp_ns(self.end)),
        }


class ContractManifest(Strict):
    """Metadata must be available as of selection, including verified deliverables.

    Databento OPRA definitions do not guarantee complete deliverable metadata. Require
    an explicit sourced contract manifest instead of inventing a 100-share multiplier.
    IDs are exact padded OCC symbols, not unstable provider instrument IDs.
    """

    schema_version: Literal[1] = 1
    provenance: str = Field(min_length=1, max_length=2000)
    contracts: list[Contract] = Field(min_length=1, max_length=32)

    @model_validator(mode="after")
    def validate_symbols(self):
        if len({c.id for c in self.contracts}) != len(self.contracts):
            raise ValueError("duplicate contract metadata")
        for c in self.contracts:
            expiry, kind, strike = raw_contract(c.id)
            if (expiry, kind, strike) != (c.expiry, c.kind, c.strike_cents):
                raise ValueError("OCC symbol disagrees with contract metadata")
            # Only standard deliverables in this single-leg replay implementation.
            if c.multiplier != 100:
                raise ValueError("adjusted deliverables unsupported")
            if c.close_by_ms > expiry_close_ms(c.expiry):
                raise ValueError("close_by exceeds conservative expiry session close")
        return self


def expiry_close_ms(expiry):
    import exchange_calendars as xcals

    calendar = xcals.get_calendar("XNYS")
    day = calendar.date_to_session(expiry, direction="previous")
    # End at the equity session close, including holidays/early closes. Do not infer
    # a 16:15 extension or an exercise cutoff from OPRA's midnight expiration field.
    return int(calendar.session_close(day).value // NS_PER_MS)


def bounded_read(path: Path):
    if not path.is_file() or path.is_symlink() or path.stat().st_size > MAX_FILE_BYTES:
        raise ValueError("expected a bounded regular input file")
    raw = path.read_bytes()
    if len(raw) > MAX_FILE_BYTES:
        raise ValueError("input exceeds size bound")
    return raw


def archive_blob(root: Path, raw: bytes, suffix: str):
    """Content-addressed originals; no overwrite, credential, or arbitrary suffix."""
    if suffix not in {"csv", "json"} or len(raw) > MAX_FILE_BYTES:
        raise ValueError("invalid archive blob")
    directory = private_root(root / "options-history", create=True)
    identity = hashlib.sha256(raw).hexdigest()
    path = directory / f"{identity}.{suffix}"
    # Publish only after the full blob is durable. A crash during writing must not
    # leave a partial file at a valid content hash or poison the next import.
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(dir=directory, prefix=".import-", delete=False) as stream:
            temporary = Path(stream.name)
            stream.write(raw)
            stream.flush()
            os.fsync(stream.fileno())
        try:
            os.link(temporary, path, follow_symlinks=False)
        except FileExistsError:
            if path.is_symlink() or bounded_read(path) != raw:
                raise ValueError("archive integrity mismatch") from None
        fd = os.open(directory, os.O_RDONLY)
        try:
            os.fsync(fd)
        finally:
            os.close(fd)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)
    return path


QUOTE_FIELDS = {
    "ts_recv",
    "ts_event",
    "rtype",
    "publisher_id",
    "instrument_id",
    "action",
    "flags",
    "bid_px_00",
    "ask_px_00",
    "bid_sz_00",
    "ask_sz_00",
    "symbol",
}


def csv_rows(raw: bytes, required: set[str], limit: int):
    if len(raw) > MAX_FILE_BYTES:
        raise ValueError("input exceeds size bound")
    reader = csv.DictReader(io.StringIO(raw.decode("utf-8-sig")))
    if (
        not reader.fieldnames
        or not required.issubset(reader.fieldnames)
        or len(reader.fieldnames) != len(set(reader.fieldnames))
    ):
        raise ValueError(
            "CSV lacks required raw Databento fields; OHLC and pretty formats unsupported"
        )
    for sequence, row in enumerate(reader):
        if sequence >= limit:
            raise ValueError("record limit reached; split the query instead of truncating")
        if None in row or any(v is None for v in row.values()):
            raise ValueError("malformed CSV record")
        yield sequence, row


def integer(value: str):
    if not re.fullmatch(r"-?\d+", value):
        raise ValueError("expected raw integer price/timestamp/size encoding")
    return int(value)


def normalize_quotes(raw, query: HistoryQuery, metadata: ContractManifest, *, feed_latency_ms=0):
    if (
        query.record_schema != "cmbp-1"
        or type(feed_latency_ms) is not int
        or not 0 <= feed_latency_ms <= 60000
    ):
        raise ValueError("expected cmbp-1 quotes and a bounded feed latency assumption")
    contracts = {c.id: c for c in metadata.contracts}
    if set(query.symbols) != set(contracts):
        raise ValueError("quote query and contract metadata must match exactly")
    start, end = timestamp_ns(query.start), timestamp_ns(query.end)
    rejected, quotes, seen, source_count = Counter(), [], set(), 0
    previous_receive = -1
    for sequence, row in csv_rows(raw, QUOTE_FIELDS, query.record_limit):
        source_count += 1
        symbol = row["symbol"]
        if symbol not in contracts or integer(row["rtype"]) != 177:
            raise ValueError("unexpected symbol or record schema in quote file")
        event, received = integer(row["ts_event"]), integer(row["ts_recv"])
        if not start <= received < end or received < previous_receive:
            raise ValueError("quotes outside query window or not in receive order")
        previous_receive = received
        flags = integer(row["flags"])
        if not 0 <= flags <= 255:
            raise ValueError("invalid record flags")
        # OPRA consolidated quote records use publisher 30; venue trade records do not.
        if integer(row["publisher_id"]) != 30 or row["action"] not in {"A", "C", "M"}:
            rejected["non_quote_record"] += 1
            continue
        if flags & BAD_FLAGS or not 0 < event <= received:
            rejected["unreliable_clock_or_book"] += 1
            continue
        bid, ask = integer(row["bid_px_00"]), integer(row["ask_px_00"])
        bid_size, ask_size = integer(row["bid_sz_00"]), integer(row["ask_sz_00"])
        if (
            bid == UNDEF_PRICE
            or ask == UNDEF_PRICE
            or not 0 <= bid <= ask
            or ask <= 0
            or not 0 < bid_size < 2**32 - 1
            or not 0 < ask_size < 2**32 - 1
        ):
            rejected["non_executable_book"] += 1
            continue
        if bid % NANODOLLARS_PER_CENT or ask % NANODOLLARS_PER_CENT:
            raise ValueError("sub-cent option quote unsupported; do not round to a better fill")
        identity = (symbol, event, received, bid, ask, bid_size, ask_size)
        if identity in seen:
            rejected["duplicate_record"] += 1
            continue
        seen.add(identity)
        available_ns = received + feed_latency_ms * NS_PER_MS
        observed_ms, available_ms = event // NS_PER_MS, (available_ns + NS_PER_MS - 1) // NS_PER_MS
        quotes.append(
            Quote(
                contract_id=symbol,
                observed_ms=observed_ms,
                available_ms=available_ms,
                observed_ns=event,
                available_ns=available_ns,
                source_sequence=sequence,
                bid_cents=bid // NANODOLLARS_PER_CENT,
                ask_cents=ask // NANODOLLARS_PER_CENT,
                bid_size=bid_size,
                ask_size=ask_size,
                regular_session=regular_session(observed_ms, False),
            )
        )
    if source_count >= query.record_limit:
        raise ValueError("record cap reached; completeness cannot be established")
    if not quotes:
        raise ValueError("no usable NBBO quotes")
    missing = set(contracts) - {q.contract_id for q in quotes}
    if missing:
        raise ValueError("one or more selected contracts have no usable quotes")
    coverage = []
    for symbol in sorted(contracts):
        rows = [q for q in quotes if q.contract_id == symbol]
        spreads = sorted(q.ask_cents - q.bid_cents for q in rows)
        stamps = [q.available_clock() for q in rows]
        coverage.append(
            {
                "contract": symbol,
                "quotes": len(rows),
                "first_ms": rows[0].observed_ms,
                "last_ms": rows[-1].observed_ms,
                "median_spread_cents": median(spreads),
                "p95_spread_cents": spreads[(95 * len(spreads) + 99) // 100 - 1],
                "max_gap_ms": max((b - a for a, b in zip(stamps, stamps[1:])), default=0)
                // NS_PER_MS,
                "regular_session_quotes": sum(q.regular_session for q in rows),
            }
        )
    return quotes, {
        "source_records": source_count,
        "accepted_quotes": len(quotes),
        "rejected": dict(rejected),
        "coverage": coverage,
        "spread_weighting": "quote observations, not time weighted",
        "clock_basis": "OPRA event time; Databento capture receive time plus assumed feed latency",
        "feed_latency_ms": feed_latency_ms,
        "completeness": "file validated; exchange completeness requires provider delivery metadata",
    }


def import_quotes(root, raw, query, metadata, plans: list[Plan], *, feed_latency_ms=0):
    root = private_root(root)
    original = archive_blob(root, raw, "csv")  # Keep malformed/partial originals for diagnosis too.
    quotes, quality = normalize_quotes(raw, query, metadata, feed_latency_ms=feed_latency_ms)
    contracts = {c.id: c for c in metadata.contracts}
    for plan in plans:
        if plan.contract_id not in contracts:
            raise ValueError("plan selects a contract absent from the frozen universe")
        if contracts[plan.contract_id].available_ms > plan.available_ms:
            raise ValueError("plan selects metadata not yet available")
    provenance = canonical(
        {
            "provider": query.provider,
            "dataset": query.dataset,
            "schema": query.record_schema,
            "raw_sha256": original.stem,
            "metadata_provenance": metadata.provenance,
            "feed_latency_ms": feed_latency_ms,
            "availability": "provider capture clock, not a measured retail broker receive clock",
        }
    )
    dataset = ReplayDataset(
        data_class="recorded",
        provenance=provenance,
        contracts=list(contracts.values()),
        quotes=quotes,
        plans=plans,
    )
    destination = archive_blob(root, canonical(dataset.model_dump()).encode(), "json")
    receipt = {
        "schema_version": 1,
        "execution_authority": False,
        "profitability_established": False,
        "query": query.model_dump(),
        "raw_sha256": original.stem,
        "dataset_sha256": destination.stem,
        "contract_metadata": metadata.model_dump(),
        "plans": len(plans),
        "quality": quality,
        "ready_for_replay": bool(plans),
        "limitation": "NBBO does not guarantee fills; causal events and selection are required.",
    }
    receipt_path = archive_blob(root, canonical(receipt).encode(), "json")
    return destination, receipt_path, receipt
