"""Bounded public-market reads through the monitor's existing Robinhood session."""

from datetime import UTC, datetime
from decimal import Decimal

from .lab import Journal, settings

READ_TOOLS = frozenset(
    {"get_equity_quotes", "get_option_chains", "get_option_instruments", "get_option_quotes"}
)


def cents(value):
    amount = Decimal(str(value)) * 100
    if not amount.is_finite() or amount != amount.to_integral_value():
        raise ValueError("unsupported sub-cent quote")
    return int(amount)


def milliseconds(value):
    dt = datetime.fromisoformat(value.replace("Z", "+00:00"))
    if dt.tzinfo is None:
        raise ValueError("timestamp requires timezone")
    return int(dt.timestamp() * 1000)


def regular_session(at, late_close):
    import exchange_calendars as xcals
    import pandas as pd

    dt = pd.Timestamp(at, unit="ms", tz="UTC")
    calendar = xcals.get_calendar("XNYS")
    day = dt.normalize().tz_localize(None)
    if not calendar.is_session(day):
        return False
    close = calendar.session_close(day) + pd.Timedelta(minutes=15 if late_close else 0)
    return calendar.session_open(day) <= dt <= close


async def capture(session, root, now_ms):
    from helios_alpha.control.broker_sync import response_data, write_private

    config = settings(root)
    journal = Journal(root)
    step = "configuration"
    try:
        last = journal.db.execute(
            "SELECT data FROM ng_health WHERE source='robinhood-options'"
        ).fetchone()
        if last:
            import json

            if (
                0
                <= now_ms() - json.loads(last[0])["checked_ms"]
                < config["quote_poll_seconds"] * 1000
            ):
                return

        async def read(name, args):
            nonlocal step
            step = name
            if name not in READ_TOOLS:
                raise ValueError("market reads only")
            return response_data(await session.call_tool(name, args))

        quote = (await read("get_equity_quotes", {"symbols": [config["symbol"]]}))["results"][0][
            "quote"
        ]
        # Use the regular-session reference to choose strikes; never call it a fresh mark.
        spot = cents(quote["last_trade_price"])
        if spot <= 0:
            raise ValueError("missing underlying reference")
        chains = (await read("get_option_chains", {"underlying_symbol": config["symbol"]}))[
            "chains"
        ]
        today = datetime.fromtimestamp(now_ms() / 1000, UTC).date()
        eligible = [
            (
                abs((datetime.fromisoformat(expiry).date() - today).days - config["target_dte"]),
                expiry,
                chain,
            )
            for chain in chains
            if chain["can_open_position"]
            for expiry in chain["expiration_dates"]
            if config["min_dte"]
            <= (datetime.fromisoformat(expiry).date() - today).days
            <= config["max_dte"]
        ]
        if not eligible:
            raise ValueError("no research expiry")
        expiry = min(eligible, key=lambda e: (e[0], e[1]))[1]
        instruments = []
        late = {}
        for _, exp, chain in eligible:
            if exp != expiry:
                continue
            page = await read(
                "get_option_instruments",
                {"chain_id": chain["id"], "expiration_dates": expiry, "state": "active"},
            )
            rows = page["instruments"] or []
            pages = 1
            while page.get("next"):
                if pages >= 4:
                    raise ValueError("incomplete option universe")
                page = await read(
                    "get_option_instruments",
                    {
                        "chain_id": chain["id"],
                        "expiration_dates": expiry,
                        "state": "active",
                        "cursor": page["next"],
                    },
                )
                rows.extend(page["instruments"] or [])
                pages += 1
            instruments.extend(rows)
            late[chain["id"]] = chain.get("late_close_state") == "enabled"
        selected = []
        for kind in ("call", "put"):
            selected.extend(
                sorted(
                    [
                        c
                        for c in instruments
                        if c["type"] == kind and c["tradability"] == "tradable"
                    ],
                    key=lambda c: (abs(cents(c["strike_price"]) - spot), c["id"]),
                )[:3]
            )
        if not selected:
            raise ValueError("empty option universe")
        data = await read("get_option_quotes", {"instrument_ids": [c["id"] for c in selected]})
        at = now_ms()
        step = "normalize_quotes"
        normalized, ticks = [], []
        for c in selected:
            normalized.append(
                {
                    "id": c["id"],
                    "symbol": config["symbol"],
                    "kind": c["type"],
                    "strike_cents": cents(c["strike_price"]),
                    "multiplier": int(Decimal(c["trade_value_multiplier"])),
                    "expiry": c["expiration_date"],
                    "available_ms": at,
                    "close_by_ms": milliseconds(c["sellout_datetime"]),
                }
            )
        by_id = {c["id"]: c for c in selected}
        for row in data["results"]:
            q = row["quote"]
            if q["instrument_id"] not in by_id or not q.get("updated_at"):
                continue
            observed = milliseconds(q["updated_at"])
            c = by_id[q["instrument_id"]]
            ticks.append(
                {
                    "contract_id": c["id"],
                    "observed_ms": observed,
                    "available_ms": at,
                    "bid_cents": cents(q["bid_price"]),
                    "ask_cents": cents(q["ask_price"]),
                    "bid_size": q["bid_size"],
                    "ask_size": q["ask_size"],
                    "regular_session": regular_session(observed, late[c["chain_id"]]),
                }
            )
        if not ticks:
            raise ValueError("no option quotes")
        journal.record_options(normalized, ticks, at)
        journal.health("robinhood-options", "receiving", at)
    except Exception as error:
        # Fixed stage/type and code locations are useful without logging OAuth URLs or bodies.
        import traceback

        write_private(
            root / "natgas-option-read-error.json",
            {
                "at_ms": now_ms(),
                "step": step,
                "error_type": type(error).__name__,
                "frames": [
                    {"function": f.name, "line": f.lineno}
                    for f in traceback.extract_tb(error.__traceback__)[-4:]
                ],
            },
        )
        raise
    finally:
        journal.close()
