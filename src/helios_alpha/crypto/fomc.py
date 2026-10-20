"""Read factual policy decisions from a narrowly recognized official statement."""

from __future__ import annotations

import hashlib
import re
import unicodedata
from datetime import datetime
from fractions import Fraction
from html.parser import HTMLParser
from zoneinfo import ZoneInfo

MAX_STATEMENT_BYTES = 256 * 1024
TITLE = "Federal Reserve issues FOMC statement"
URL = re.compile(
    r"https://www\.federalreserve\.gov/newsevents/pressreleases/monetary(\d{8})[a-z]\.htm\Z"
)
NUMBER = r"(?:\d{1,2}[- ]\d{1,2}/\d{1,2}|\d{1,2}/\d{1,2}|\d{1,2}(?:\.\d{1,2})?)"
DECISION = re.compile(
    r"^(?:In support of its goals,\s*)?The Committee decided to "
    r"(?P<action>maintain|lower|reduce|raise|increase) the target range for the federal funds rate "
    rf"(?:(?:by (?P<change>{NUMBER}) percentage points? to)|at|to) "
    rf"(?P<lower>{NUMBER}) to (?P<upper>{NUMBER}) percent\b",
    re.IGNORECASE,
)


def needs_source_facts(item):
    return (
        item.get("source") == "fed-monetary"
        and item.get("source_category") == "monetary_policy"
        and item.get("title", "").casefold() == TITLE.casefold()
        and isinstance(item.get("url"), str)
        and URL.fullmatch(item["url"]) is not None
    )


def _text(value):
    for fraction, replacement in (("¼", " 1/4"), ("½", " 1/2"), ("¾", " 3/4")):
        value = value.replace(fraction, replacement)
    value = unicodedata.normalize("NFKC", value).replace("⁄", "/")
    for dash in ("–", "—", "‑", "−"):
        value = value.replace(dash, "-")
    return " ".join(value.split())


class _Paragraphs(HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.blocks = []
        self.block = None
        self.ignored = 0

    def finish(self):
        if self.block is not None:
            tag, classes, pieces = self.block
            self.blocks.append((tag, classes, _text("".join(pieces))))
            self.block = None

    def handle_starttag(self, tag, attrs):
        if tag in {"script", "style", "noscript"}:
            self.ignored += 1
        if self.ignored:
            return
        if tag in {"p", "h3"}:
            self.finish()
            self.block = (tag, set((dict(attrs).get("class") or "").split()), [])
        elif tag == "br" and self.block is not None:
            self.block[2].append(" ")

    def handle_endtag(self, tag):
        if tag in {"script", "style", "noscript"}:
            self.ignored = max(0, self.ignored - 1)
            return
        if not self.ignored and self.block is not None and self.block[0] == tag:
            self.finish()

    def handle_data(self, data):
        if not self.ignored and self.block is not None:
            self.block[2].append(data)


def _basis_points(value):
    mixed = re.fullmatch(r"(\d{1,2})[- ](\d{1,2}/\d{1,2})", value)
    try:
        rate = (Fraction(mixed[1]) + Fraction(mixed[2]) if mixed else Fraction(value)) * 100
    except ZeroDivisionError as error:
        raise ValueError("invalid policy rate fraction") from error
    if rate.denominator != 1 or not 0 <= rate <= 10_000:
        raise ValueError("unsupported policy rate precision or range")
    return int(rate)


def parse_statement(item, raw: bytes, fetched_ms: int):
    """Reject ambiguous, mismatched or unsupported pages; never infer a market surprise."""
    if not needs_source_facts(item) or len(raw) > MAX_STATEMENT_BYTES:
        raise ValueError("unsupported statement origin or size")
    published, observed = item.get("published_ms"), item.get("observed_ms")
    if (
        any(type(value) is not int for value in (published, observed, fetched_ms))
        or not 0 <= published <= observed <= fetched_ms
    ):
        raise ValueError("invalid statement availability clocks")
    date = datetime.strptime(URL.fullmatch(item["url"])[1], "%Y%m%d").date()
    if datetime.fromtimestamp(published / 1000, ZoneInfo("America/New_York")).date() != date:
        raise ValueError("statement URL and publication date disagree")
    parser = _Paragraphs()
    parser.feed(raw.decode("utf-8"))
    parser.close()
    parser.finish()
    titles = [text for tag, classes, text in parser.blocks if tag == "h3" and "title" in classes]
    dates = [
        text for tag, classes, text in parser.blocks if tag == "p" and "article__time" in classes
    ]
    if (
        titles != [TITLE]
        or len(dates) != 1
        or datetime.strptime(dates[0], "%B %d, %Y").date() != date
    ):
        raise ValueError("statement page identity mismatch")
    matches = []
    for tag, _, text in parser.blocks:
        if tag == "p" and (match := DECISION.match(text)):
            if len(re.findall(r"The Committee decided to", text, re.IGNORECASE)) != 1:
                raise ValueError("multiple operative decisions")
            matches.append(match)
    if len(matches) != 1:
        raise ValueError("missing or ambiguous operative decision")
    match = matches[0]
    action = {
        "maintain": "maintain", "lower": "lower", "reduce": "lower",
        "raise": "raise", "increase": "raise",
    }[match["action"].lower()]
    lower, upper = _basis_points(match["lower"]), _basis_points(match["upper"])
    if not 0 <= lower < upper <= 10_000 or upper - lower > 100:
        raise ValueError("unsupported target range")
    change = _basis_points(match["change"]) if match["change"] else None
    if (action == "maintain" and change is not None) or change == 0:
        raise ValueError("inconsistent policy action")
    signed = 0 if action == "maintain" else (
        (-change if action == "lower" else change) if change is not None else None
    )
    return {
        "schema_version": 1,
        "kind": "fomc_target_range",
        "status": "parsed",
        "action": action,
        "target_lower_bps": lower,
        "target_upper_bps": upper,
        "stated_change_bps": signed,
        "statement_date": date.isoformat(),
        "source_url": item["url"],
        "content_sha256": hashlib.sha256(raw).hexdigest(),
        "fetched_ms": fetched_ms,
        "decision_text": match[0],
        "market_surprise": "not_measured",
    }


async def fetch_statement(client, item, clock):
    if not needs_source_facts(item):
        raise ValueError("unsupported statement origin")
    async with client.stream("GET", item["url"], timeout=3.0, follow_redirects=False) as response:
        if response.status_code != 200:
            raise ValueError("statement fetch unsuccessful")
        if response.headers.get("content-type", "").split(";")[0].strip().lower() != "text/html":
            raise ValueError("unsupported statement content type")
        raw = bytearray()
        async for chunk in response.aiter_bytes():
            raw.extend(chunk)
            if len(raw) > MAX_STATEMENT_BYTES:
                raise ValueError("statement exceeds bound")
    return parse_statement(item, bytes(raw), clock())
