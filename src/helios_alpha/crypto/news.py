"""Bounded public news intake. This journal is evidence, never trading authority."""

from __future__ import annotations

import hashlib
import html
import json
import re
import sqlite3
from dataclasses import dataclass
from datetime import datetime
from email.utils import parsedate_to_datetime
from urllib.parse import urlsplit, urlunsplit

from defusedxml import ElementTree

from helios_alpha.control.golem_store import read_private_json
from helios_alpha.control.store import canonical, private_root

from .fomc import needs_source_facts

MAX_FEED_BYTES = 512 * 1024
MAX_NEWS_AGE_MS = 120_000
MAX_ACTIVE_NEWS = 200


def fresh_news(items: list[dict], at_ms: int) -> list[dict]:
    """Original publication and receipt times both bound the eligibility window."""
    if type(at_ms) is not int or at_ms < 0:
        raise ValueError("invalid news evaluation timestamp")
    return [
        item
        for item in items
        if isinstance(item, dict)
        and item.get("eligible") is True
        and type(item.get("observed_ms")) is int
        and type(item.get("published_ms")) is int
        and 0 <= at_ms - item["observed_ms"] <= MAX_NEWS_AGE_MS
        and 0 <= at_ms - item["published_ms"] <= MAX_NEWS_AGE_MS
    ]


@dataclass(frozen=True)
class Feed:
    id: str
    url: str
    host: str
    category: str


# Fixed origins: headline URLs and feed contents never choose network destinations.
FEEDS = (
    Feed(
        "fed-monetary",
        "https://www.federalreserve.gov/feeds/press_monetary.xml",
        "www.federalreserve.gov",
        "monetary_policy",
    ),
    Feed("sec-press", "https://www.sec.gov/news/pressreleases.rss", "www.sec.gov", "regulation"),
    Feed("kraken", "https://blog.kraken.com/feed", "blog.kraken.com", "exchange"),
    Feed(
        "coindesk",
        "https://www.coindesk.com/arc/outboundfeeds/rss",
        "www.coindesk.com",
        "reporting",
    ),
    Feed("cointelegraph", "https://cointelegraph.com/rss", "cointelegraph.com", "reporting"),
    Feed("decrypt", "https://decrypt.co/feed", "decrypt.co", "reporting"),
)


def stamp_ms(value: str) -> int:
    try:
        stamp = datetime.fromisoformat(value.strip().replace("Z", "+00:00"))
    except ValueError:
        stamp = parsedate_to_datetime(value.strip())
    if stamp.utcoffset() is None:
        raise ValueError("news timestamp lacks timezone")
    return int(stamp.timestamp() * 1000)


def text_only(value: str, limit: int) -> str:
    text = html.unescape(re.sub(r"<[^>]*>", " ", value))
    return " ".join(text.split())[:limit]


def parse_feed(feed: Feed, raw: bytes) -> list[dict]:
    if len(raw) > MAX_FEED_BYTES:
        raise ValueError("news feed exceeds bound")
    root = ElementTree.fromstring(raw, forbid_dtd=True, forbid_entities=True, forbid_external=True)
    atom = "{http://www.w3.org/2005/Atom}"
    entries = root.findall("./channel/item") if root.tag == "rss" else root.findall(atom + "entry")
    if root.tag not in ("rss", atom + "feed") or not entries or len(entries) > 200:
        raise ValueError("invalid or empty news feed")
    result = {}
    for entry in entries:
        if root.tag == "rss":
            title, link = entry.findtext("title", ""), entry.findtext("link", "")
            published = entry.findtext("pubDate", "")
        else:
            title = entry.findtext(atom + "title", "")
            links = [
                node.get("href", "")
                for node in entry.findall(atom + "link")
                if node.get("rel", "alternate") == "alternate"
            ]
            link = links[0] if links else ""
            # An edit's updated timestamp must not rejuvenate an old story.
            published = entry.findtext(atom + "published", "")
        try:
            parts = urlsplit(link.strip())
            port = parts.port
        except ValueError:
            continue
        if (
            parts.scheme != "https"
            or parts.hostname != feed.host
            or parts.username is not None
            or parts.password is not None
            or port not in (None, 443)
            or len(link) > 2048
        ):
            continue
        url = urlunsplit(("https", feed.host, parts.path, "", ""))
        title = text_only(title, 500)
        if not title:
            continue
        try:
            published_ms = stamp_ms(published)
        except (ValueError, TypeError, OverflowError):
            published_ms = None
        identity = hashlib.sha256(f"{feed.id}\n{url}".encode()).hexdigest()
        item = dict(
            schema_version=1,
            news_id=identity,
            source=feed.id,
            source_category=feed.category,
            url=url,
            title=title,
            published_ms=published_ms,
            synthetic=False,
        )
        # Ambiguous duplicate URLs in a single feed are ineligible.
        if identity in result and result[identity] != item:
            item["published_ms"] = None
        result[identity] = item
    return list(result.values())


class NewsJournal:
    """Transactional feed baseline, deduplication, and immutable publish outbox."""

    def __init__(self, root):
        root = private_root(root)
        path = root / "news-intake.sqlite3"
        if path.is_symlink():
            raise ValueError("news journal must not be a symlink")
        self.db = sqlite3.connect(path)
        path.chmod(0o600)
        self.db.row_factory = sqlite3.Row
        self.db.executescript(
            """
            PRAGMA journal_mode=WAL;
            PRAGMA synchronous=FULL;
            CREATE TABLE IF NOT EXISTS feeds(
              id TEXT PRIMARY KEY, last_ok_ms INTEGER NOT NULL, etag TEXT, modified TEXT);
            CREATE TABLE IF NOT EXISTS news(
              id TEXT PRIMARY KEY, observed_ms INTEGER NOT NULL, data TEXT NOT NULL,
              pending INTEGER NOT NULL, delivered INTEGER NOT NULL DEFAULT 0);
            CREATE TABLE IF NOT EXISTS entry_attempts(
              id TEXT PRIMARY KEY, data TEXT NOT NULL, completed INTEGER NOT NULL DEFAULT 0);
            CREATE TABLE IF NOT EXISTS source_details(
              news_id TEXT PRIMARY KEY);
            CREATE INDEX IF NOT EXISTS recent_published_news ON news(delivered,observed_ms);
            """
        )

    def headers(self, feed: Feed) -> dict:
        row = self.db.execute("SELECT * FROM feeds WHERE id=?", (feed.id,)).fetchone()
        if row is None:
            return {}
        return {
            name: row[key]
            for name, key in (("If-None-Match", "etag"), ("If-Modified-Since", "modified"))
            if row[key]
        }

    def ingest(self, feed: Feed, items: list[dict], now_ms: int, headers: dict) -> dict:
        baseline = self.db.execute("SELECT 1 FROM feeds WHERE id=?", (feed.id,)).fetchone() is None
        fresh = 0
        inserted = 0
        with self.db:
            self.db.execute(
                "DELETE FROM news WHERE observed_ms<? AND delivered=1",
                (now_ms - 30 * 86400_000,),
            )
            count = self.db.execute("SELECT COUNT(*) FROM news").fetchone()[0]
            if count + len(items) > 10_000:
                raise ValueError("news journal capacity reached")
            for item in items:
                published = item["published_ms"]
                eligible = (
                    not baseline
                    and published is not None
                    and 0 <= now_ms - published <= MAX_NEWS_AGE_MS
                )
                record = {
                    **item,
                    "observed_ms": now_ms,
                    "eligible": eligible,
                    "intake_reason": "startup_baseline"
                    if baseline
                    else ("fresh" if eligible else "stale_or_undated"),
                }
                waiting = eligible and needs_source_facts(item)
                if waiting:
                    record.pop("source_facts", None)
                    record["intake_reason"] = "awaiting_source_facts"
                added = self.db.execute(
                    "INSERT OR IGNORE INTO news(id,observed_ms,data,pending,delivered)"
                    " VALUES(?,?,?,?,?)",
                    (
                        item["news_id"],
                        now_ms,
                        canonical(record),
                        int(eligible and not waiting),
                        int(not eligible),
                    ),
                ).rowcount
                if added and waiting:
                    self.db.execute("INSERT INTO source_details VALUES(?)", (item["news_id"],))
                inserted += added
                fresh += added * int(eligible)
            self.db.execute(
                "INSERT INTO feeds VALUES(?,?,?,?) ON CONFLICT(id) DO UPDATE SET"
                " last_ok_ms=excluded.last_ok_ms,etag=excluded.etag,modified=excluded.modified",
                (feed.id, now_ms, headers.get("etag"), headers.get("last-modified")),
            )
        return {"baseline": baseline, "new_items": inserted, "fresh_items": fresh}

    def pending(self) -> list[dict]:
        return [
            json.loads(row[0])
            for row in self.db.execute(
                "SELECT data FROM news WHERE pending=1 AND delivered=0 "
                "ORDER BY observed_ms,id LIMIT 200"
            )
        ]

    def delivered(self, news_id: str):
        with self.db:
            self.db.execute("UPDATE news SET delivered=1 WHERE id=?", (news_id,))

    def recent_published(self, at_ms: int, sources: set[str]) -> list[dict]:
        """Recover acknowledged headlines, preserving their immutable freshness clocks."""
        if type(at_ms) is not int or at_ms < 0:
            raise ValueError("invalid news recovery timestamp")
        result = []
        for row in self.db.execute(
            "SELECT data FROM news WHERE pending=1 AND delivered=1 AND observed_ms BETWEEN ? AND ? "
            "ORDER BY observed_ms DESC,id DESC LIMIT 10000",
            (at_ms - MAX_NEWS_AGE_MS, at_ms),
        ):
            item = json.loads(row["data"])
            if fresh_news([item], at_ms) and item.get("source") in sources:
                result.append(item)
                if len(result) == MAX_ACTIVE_NEWS:
                    break
        return list(reversed(result))

    def close(self):
        self.db.close()

    def pending_source_details(self):
        return [
            json.loads(row[0])
            for row in self.db.execute(
                "SELECT news.data FROM source_details "
                "JOIN news ON news.id=source_details.news_id "
                "WHERE news.delivered=0 AND news.pending=0 ORDER BY news.observed_ms LIMIT 200"
            )
        ]

    def finish_source_details(self, news_id, facts, at_ms):
        """Finalize once, before outbox publication; preserve the original intake clocks."""
        with self.db:
            row = self.db.execute(
                "SELECT news.data FROM source_details "
                "JOIN news ON news.id=source_details.news_id WHERE news.id=?",
                (news_id,),
            ).fetchone()
            if row is None:
                return False
            item = json.loads(row[0])
            if type(at_ms) is not int or at_ms < item["observed_ms"]:
                raise ValueError("source detail clock regression")
            fresh = bool(fresh_news([item], at_ms))
            if facts is None:
                if fresh:
                    raise ValueError("fresh source details cannot be discarded")
            elif (
                not isinstance(facts, dict)
                or facts.get("status") != "parsed"
                or facts.get("kind") != "fomc_target_range"
                or facts.get("source_url") != item["url"]
                or type(facts.get("fetched_ms")) is not int
                or not item["observed_ms"] <= facts["fetched_ms"] <= at_ms
                or len(canonical(facts).encode()) > 4096
            ):
                raise ValueError("source details do not match staged news")
            item["source_facts"] = facts or {
                "status": "unavailable",
                "reason": "freshness_window_expired",
            }
            item["eligible"] = fresh and facts is not None
            item["intake_reason"] = "fresh" if item["eligible"] else "stale_during_source_fetch"
            self.db.execute(
                "UPDATE news SET data=?,pending=?,delivered=? WHERE id=?",
                (canonical(item), int(item["eligible"]), int(not item["eligible"]), news_id),
            )
            self.db.execute("DELETE FROM source_details WHERE news_id=?", (news_id,))
            return True

    def entry_attempted(self, identity: str) -> bool:
        return (
            self.db.execute("SELECT 1 FROM entry_attempts WHERE id=?", (identity,)).fetchone()
            is not None
        )

    def claim_entry(self, identity: str, event: dict) -> bool:
        """At most one immutable proposal per story/symbol, including after restart."""
        with self.db:
            if self.db.execute("SELECT COUNT(*) FROM entry_attempts").fetchone()[0] >= 10_000:
                raise ValueError("entry evidence capacity reached")
            return bool(
                self.db.execute(
                    "INSERT OR IGNORE INTO entry_attempts(id,data) VALUES(?,?)",
                    (identity, canonical(event)),
                ).rowcount
            )

    def pending_entries(self) -> list[tuple[str, dict]]:
        return [
            (row[0], json.loads(row[1]))
            for row in self.db.execute(
                "SELECT id,data FROM entry_attempts WHERE completed=0 ORDER BY rowid LIMIT 100"
            )
        ]

    def entry_completed(self, identity: str):
        with self.db:
            self.db.execute("UPDATE entry_attempts SET completed=1 WHERE id=?", (identity,))


async def fetch_feed(client, feed: Feed, headers: dict) -> tuple[int, dict, bytes]:
    async with client.stream("GET", feed.url, headers=headers) as response:
        raw = bytearray()
        if response.status_code == 200:
            async for chunk in response.aiter_bytes():
                raw.extend(chunk)
                if len(raw) > MAX_FEED_BYTES:
                    raise ValueError("news feed exceeds bound")
        return response.status_code, dict(response.headers), bytes(raw)


def configured_feeds(root) -> tuple[Feed, ...]:
    config = read_private_json(root / "runtime.json")["news"]
    ids = config["feeds"]
    available = {feed.id: feed for feed in FEEDS}
    if not ids or len(set(ids)) != len(ids) or not set(ids) <= available.keys():
        raise ValueError("unsupported news source")
    return tuple(available[name] for name in ids)


def polling_intervals(config: dict, feeds) -> dict[str, int]:
    """Resolve bounded per-source intervals without changing news eligibility."""
    default = config.get("poll_seconds")
    overrides = config.get("poll_seconds_by_source", {})
    ids = {feed.id for feed in feeds}
    if (
        type(default) is not int
        or not 15 <= default <= 300
        or not isinstance(overrides, dict)
        or not set(overrides) <= ids
        or any(type(value) is not int or not 15 <= value <= 300 for value in overrides.values())
    ):
        raise ValueError("invalid news polling intervals")
    return {identity: overrides.get(identity, default) for identity in ids}
