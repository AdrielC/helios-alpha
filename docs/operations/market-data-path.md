# First market-data and broker path

The lowest-cost operational path is **Alpaca Basic, paper trading first, then a one-dollar
fractional long-only canary**. Databento stays in the architecture as the higher-fidelity replay
and futures feed, but it is not the first subscription to buy.

This decision was verified against provider documentation on **August 30, 2026**. Pricing and
entitlements can change, so recheck them before opening an account or authorizing payment.

## Decision

| Phase | Data | Broker | Instrument | Capital | What it proves |
|---|---|---|---|---:|---|
| Local | deterministic fixtures | paper adapter | synthetic | $0 | Causality, replay, checkpoints, commands |
| Paper | Alpaca Basic IEX | Alpaca paper | `SPY` fractional | $0 | Credentials, live marks, order lifecycle, reconciliation |
| Cash canary | Alpaca Basic, with execution evidence captured | Alpaca live | `SPY` fractional long only | $1 | Real acknowledgement, fill, position, flatten, incident path |
| Strategy shadow | NOAA/NASA plus market feed | no order authority | `SMH`, broad risk assets | $0 | Publication latency, revisions, signal stability, costs |
| Strategy canary | upgraded feed selected from evidence | certified broker | predeclared fractional instrument | bounded by risk policy | End-to-end event-shock operation |
| Futures research | Databento historical CME | none initially | liquid micro futures | usage based | Higher-resolution event studies and replay |

`SPY` is the plumbing canary, not a claim that space weather predicts the S&P 500. A later `SMH`
or other thematic canary needs its own predeclared hypothesis, cost model, and shadow evidence.

## Why Alpaca first

Alpaca documents a free Basic market-data plan, paper trading, IEX-only US equity coverage, a
30-symbol WebSocket limit, and restricted access to the latest 15 minutes of historical data.
That is enough to certify the operational path, but not enough to claim venue-quality market
reconstruction. Its paid Algo Trader Plus plan adds SIP coverage and was listed at $99 per month
when this decision was recorded. See Alpaca's [market-data overview](https://docs.alpaca.markets/docs/about-market-data-api)
and [market-data FAQ](https://docs.alpaca.markets/docs/market-data-faq).

Alpaca also documents fractional stock trading from one dollar, with paper and live support.
Fractional short sales are not supported, so the first cash canary is explicitly long only. See
[fractional trading](https://docs.alpaca.markets/docs/fractional-trading).

## Where Databento fits

Databento historical CME access is usage based and does not require a recurring subscription. New
accounts were advertising $125 of historical credits when this decision was recorded. Use that
budget only after the event contract and required schema are fixed, then download the smallest
reproducible slice.

Databento's Standard live CME plan was listed at $199 per month. Its live protocol supports up to
24 hours of intraday replay and replay-to-live behavior, which fits the Helio source handoff model
well. Buy it after the paper path proves that market-data quality is the next binding constraint,
not before. See [Databento pricing](https://databento.com/pricing) and the
[Live API reference](https://databento.com/docs/api-reference-live?live=raw).

Databento symbology can change across continuous, parent, and raw instrument identifiers. Persist
the resolved mapping beside each replay manifest. See the
[symbology conventions](https://databento.com/docs/standards-and-conventions/symbology).

## Admission gates

No live-capital flag changes because an account exists. The cash canary stays closed until all of
these are true:

1. The source adapter emits event time, availability time, observation time, partition, and exact
   resumable offset.
2. Backfill-to-live handoff is gap tested and a restart resumes from the last committed prefix.
3. Order intent is journaled before send and reconciled by stable client order identity.
4. The operator command service is authenticated, idempotent, CSRF protected, and sequence gated.
5. Venue session, stale-data, position, gross, daily-order, and kill-switch limits are active.
6. A paper incident drill proves cancel, flatten, disconnect, and restart behavior.
7. The deployment digest and rollback path are recorded.

Alpaca Basic is the cheapest useful path to operational evidence. It is not the final answer for
historical depth, SIP reconstruction, or latency-sensitive execution.
