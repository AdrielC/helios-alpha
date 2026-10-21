"""Public prediction-market discovery and bounded order-book observations."""

from __future__ import annotations

import asyncio
import json
import math
import re
import time
from email.utils import parsedate_to_datetime
from urllib.parse import quote

import httpx

from .news import stamp_ms, text_only

ORIGINS = {
    "polymarket": "https://gamma-api.polymarket.com",
    "clob": "https://clob.polymarket.com",
    "kalshi": "https://external-api.kalshi.com/trade-api/v2",
}


def finite(value):
    if isinstance(value, bool):
        raise ValueError("invalid numeric observation")
    result = float(value)
    if not math.isfinite(result):
        raise ValueError("nonfinite observation")
    return result


async def public_json(client, origin, path, params=None):
    async with client.stream("GET", ORIGINS[origin] + path, params=params) as response:
        response.raise_for_status()
        body = bytearray()
        async for chunk in response.aiter_bytes():
            body.extend(chunk)
            if len(body) > 2_000_000:
                raise ValueError("public response exceeds bound")
        received = time.time_ns() // 1_000_000
        age = max(0, finite(response.headers.get("age", "0")))
        date = response.headers.get("date")
        asof = (
            min(received, int(parsedate_to_datetime(date).timestamp() * 1000)) if date else received
        )
        return json.loads(body), received, min(asof, received - int(age * 1000))


def matched_topic(title, config):
    title = title.lower()
    return next(
        (
            topic
            for topic in config["topics"]
            if any(term.lower() in title for term in topic["terms"])
        ),
        None,
    )


def registry_item(provider, market, config, now):
    if provider == "polymarket":
        if market.get("closed") or not market.get("active") or not market.get("acceptingOrders"):
            return None
        outcomes = market.get("outcomes", [])
        outcomes = json.loads(outcomes) if isinstance(outcomes, str) else outcomes
        if outcomes != ["Yes", "No"]:
            return None
        tokens = market.get("clobTokenIds", [])
        tokens = json.loads(tokens) if isinstance(tokens, str) else tokens
        if len(tokens) != 2 or not all(re.fullmatch(r"\d{1,100}", str(token)) for token in tokens):
            return None
        identity, title = str(market["id"]), market["question"]
        end = stamp_ms(market["endDate"])
        volume = finite(market.get("volume24hr", 0))
        event = next(iter(market.get("events", [])), {})
        event_id = str(event.get("id", identity))
        url = "https://polymarket.com/event/" + quote(
            event.get("slug", market.get("slug", "")), safe=""
        )
        extra = {
            "token_id": str(tokens[0]),
            "volume_unit": "USD",
            "event_title": text_only(event.get("title", event_id), 300),
        }
    else:
        if market.get("status") != "active" or market.get("market_type") != "binary":
            return None
        identity, title = market["ticker"], market["title"]
        subtitle = text_only(market.get("yes_sub_title", ""), 120)
        if subtitle and subtitle.lower() not in title.lower():
            title += " · " + subtitle
        end = stamp_ms(market["close_time"])
        volume = finite(market.get("volume_24h_fp", 0))
        event_id = market["event_ticker"]
        url = "https://kalshi.com/markets/" + quote(identity.split("-")[0].lower(), safe="")
        extra = {"volume_unit": "contracts", "event_title": event_id}
    topic = matched_topic(title, config)
    if (
        not topic
        or not re.fullmatch(r"[A-Za-z0-9_.-]{1,160}", identity)
        or not now < end <= now + 120 * 86400_000
    ):
        return None
    return dict(
        id=f"{provider}:{identity}",
        market_id=identity,
        event_id=event_id,
        provider=provider,
        title=text_only(title, 300),
        url=url,
        topic=topic["id"],
        symbols=topic["symbols"],
        closes_ms=end,
        volume_24h=max(0, volume),
        resolution=text_only(
            market.get("description", "")
            if provider == "polymarket"
            else "\n".join(
                str(market.get(key, "")) for key in ("rules_primary", "rules_secondary")
            ),
            4000,
        ),
        **extra,
    )


async def discover(client, provider, config, now):
    rows = []
    if provider == "polymarket":
        for slug in config["polymarket_tags"]:
            tag, _, _ = await public_json(client, provider, "/tags/slug/" + quote(slug, safe=""))
            markets, _, _ = await public_json(
                client,
                provider,
                "/markets",
                {
                    "active": "true",
                    "closed": "false",
                    "tag_id": str(tag["id"]),
                    "limit": 50,
                    "order": "volume24hr",
                    "ascending": "false",
                },
            )
            rows.extend(markets)
    else:
        for series in config["kalshi_series"]:
            result, _, _ = await public_json(
                client,
                provider,
                "/markets",
                {"series_ticker": series, "status": "open", "limit": 100},
            )
            rows.extend(result["markets"])
    result = {}
    for row in rows:
        try:
            item = registry_item(provider, row, config, now)
            if item:
                result[item["id"]] = item
        except (ValueError, KeyError, TypeError):
            continue
    selected = sorted(result.values(), key=lambda item: (-item["volume_24h"], item["id"]))[
        : config["markets_per_provider"]
    ]
    if provider == "kalshi":
        sem = asyncio.Semaphore(3)

        async def enrich(event_id):
            async with sem:
                try:
                    body, _, _ = await public_json(
                        client, provider, "/events/" + quote(event_id, safe="")
                    )
                    return event_id, text_only(body["event"]["title"], 300)
                except (httpx.HTTPError, ValueError, KeyError, TypeError):
                    return event_id, event_id

        titles = dict(
            await asyncio.gather(*(enrich(key) for key in {m["event_id"] for m in selected}))
        )
        for item in selected:
            item["event_title"] = titles[item["event_id"]]
    return selected


async def observe(client, item):
    if item["provider"] == "polymarket":
        data, received, asof = await public_json(
            client, "clob", "/book", {"token_id": item["token_id"]}
        )
        bids = [(finite(row["price"]), finite(row["size"])) for row in data.get("bids", [])]
        asks = [(finite(row["price"]), finite(row["size"])) for row in data.get("asks", [])]
        # A book's last-change timestamp is distinct from the HTTP snapshot observation.
        changed = int(data["timestamp"]) if data.get("timestamp") else None
    else:
        data, received, asof = await public_json(
            client,
            "kalshi",
            "/markets/" + quote(item["market_id"], safe="") + "/orderbook",
            {"depth": 20},
        )
        book = data["orderbook_fp"]
        bids = [(finite(price), finite(size)) for price, size in book.get("yes_dollars", [])]
        asks = [(1 - finite(price), finite(size)) for price, size in book.get("no_dollars", [])]
        changed = None
    if not bids or not asks:
        raise ValueError("one-sided book")
    bid, bid_size = max(bids, key=lambda row: row[0])
    ask, ask_size = min(asks, key=lambda row: row[0])
    if not 0 < bid <= ask < 1 or min(bid_size, ask_size) <= 0:
        raise ValueError("invalid two-sided book")
    return dict(
        observed_ms=asof,
        received_ms=received,
        changed_ms=changed,
        probability=(bid + ask) / 2,
        bid=bid,
        ask=ask,
        depth_usd=min(bid * bid_size, (1 - ask) * ask_size),
        spread_pp=(ask - bid) * 100,
    )
