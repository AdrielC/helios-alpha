"""Bind model review to the actual evidence selected by the private strategy."""

from __future__ import annotations

import hashlib
import json
import re

from helios_alpha.control.store import canonical

from .news import MAX_NEWS_AGE_MS, fresh_news
from .strategy import MAX_NEWS_EVIDENCE


def prepare_news_review(proposal: dict, context: dict, fingerprint: str, at_ms: int) -> dict:
    anchor = proposal["news_id"]
    identities = proposal.get("evidence_news_ids", [anchor])
    if (
        not isinstance(identities, list)
        or not 1 <= len(identities) <= MAX_NEWS_EVIDENCE
        or any(
            not isinstance(identity, str) or not re.fullmatch(r"[a-f0-9]{64}", identity)
            for identity in identities
        )
        or len(set(identities)) != len(identities)
        or anchor not in identities
    ):
        raise ValueError("invalid review evidence selection")
    identities = sorted(identities)
    evidence = {}
    for item in context["news"]:
        identity = item["news_id"]
        if identity not in identities:
            continue
        if identity in evidence and evidence[identity] != item:
            raise ValueError("conflicting review evidence")
        evidence[identity] = item
    if set(evidence) != set(identities) or len(fresh_news(list(evidence.values()), at_ms)) != len(
        identities
    ):
        raise ValueError("review evidence is unavailable or stale")
    expires = min(
        min(item["published_ms"], item["observed_ms"]) + MAX_NEWS_AGE_MS
        for item in evidence.values()
    )
    if at_ms >= expires:
        raise ValueError("review evidence expired")
    # Exclude moving prices from the cache identity; the execution path rechecks
    # price confirmation. Changed supporting facts require a separate review.
    evidence_digest = hashlib.sha256(
        canonical([evidence[identity] for identity in identities]).encode()
    ).hexdigest()
    identity = hashlib.sha256(
        canonical(
            {
                "kind": "news-v2",
                "news_id": anchor,
                "evidence_digest": evidence_digest,
                "symbol": context["symbol"],
                "strategy": fingerprint,
            }
        ).encode()
    ).hexdigest()
    report_fields = (
        "news_id", "source", "source_category", "title", "published_ms", "observed_ms",
        "url", "source_facts",
    )
    event = {
        **evidence[anchor],
        "symbol": context["symbol"],
        "quote": context["quote"],
        "reference_samples": context["market"]["samples"][-3:],
        "evidence_news_ids": identities,
        "other_reports": [
            {key: evidence[other][key] for key in report_fields if key in evidence[other]}
            for other in identities if other != anchor
        ],
    }
    return {
        "identity": identity,
        "expires_ms": expires,
        # Review runs asynchronously. Later changes to caller-owned objects
        # cannot alter the evidence already bound to this identity.
        "event": json.loads(canonical(event)),
    }
