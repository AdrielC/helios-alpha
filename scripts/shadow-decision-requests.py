#!/usr/bin/env python3
"""Stream committed scientific observations as bounded JSONL decision requests.

This process reads the shadow journal and emits requests only. Pipe its output to
helios-decision to invoke a provider; keep TTL and question versions fixed on replay.
"""

import argparse
import hashlib
import json
import sqlite3
import sys
import time
from datetime import datetime
from pathlib import Path


def epoch_ms(value):
    return int(datetime.fromisoformat(value.replace("Z", "+00:00")).timestamp() * 1000)


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False)


def make_request(row, questions, ttl_ms):
    source = dict(row)
    source["payload"] = json.loads(source.pop("payload_json"))
    source["quality_flags"] = json.loads(source.pop("quality_flags_json"))
    # Machine observation time is an additional causal boundary. Publication alone
    # does not mean this machine had fetched the observation.
    available = max(epoch_ms(source["available_at"]), epoch_ms(source["observed_at"]))
    identity = hashlib.sha256(
        canonical({"source": source, "questions": questions, "ttl_ms": ttl_ms}).encode()
    ).hexdigest()
    return {
        "schema_version": 1,
        "request_id": f"shadow:{identity}",
        "effective_at_ms": epoch_ms(source["event_time"]),
        "available_at_ms": available,
        "deadline_ms": available + ttl_ms,
        "input": {"type": "state", "value": {"observation": source}},
        "questions": questions,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--journal", type=Path, default=Path("data/shadow/space-weather.sqlite3"))
    parser.add_argument(
        "--questions", type=Path, default=Path("rust/fixtures/decisions/scientific-questions.json")
    )
    parser.add_argument("--after-sequence", type=int, default=0)
    parser.add_argument("--ttl-ms", type=int, default=120_000)
    parser.add_argument("--follow", action="store_true")
    parser.add_argument("--poll-seconds", type=float, default=1.0)
    parser.add_argument(
        "--include-expired",
        action="store_true",
        help="For offline inspection; runner still enforces deadlines.",
    )
    args = parser.parse_args()
    if args.ttl_ms <= 0 or args.poll_seconds <= 0 or args.after_sequence < 0:
        parser.error("TTL and poll interval must be positive; sequence must be nonnegative.")
    questions = json.loads(args.questions.read_text())
    connection = sqlite3.connect(args.journal.resolve().as_uri() + "?mode=ro", uri=True)
    connection.row_factory = sqlite3.Row
    cursor = args.after_sequence
    try:
        while True:
            rows = connection.execute(
                "SELECT * FROM shadow_observations WHERE sequence > ? ORDER BY sequence LIMIT 64",
                (cursor,),
            ).fetchall()
            for row in rows:
                request = make_request(row, questions, args.ttl_ms)
                cursor = row["sequence"]
                now = time.time_ns() // 1_000_000
                if args.include_expired or request["deadline_ms"] > now:
                    print(canonical(request), flush=True)
            if rows:
                continue
            if not args.follow:
                break
            time.sleep(args.poll_seconds)
    except BrokenPipeError:
        sys.stdout = open("/dev/null", "w")
    finally:
        connection.close()


if __name__ == "__main__":
    main()
