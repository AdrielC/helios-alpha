# Market data providers (licensed / paid)

**Recommendation for this repo:** **[Polygon.io](https://polygon.io/)** — Stocks **Developer** tier is usually the sweet spot for **cheap + good API** (REST aggregates, minute bars when you upgrade, clear docs). You bring your own license; set `HELIOS_POLYGON_API_KEY`.

| Provider | Why consider | Caveats |
|----------|----------------|---------|
| **Polygon.io** | Simple REST, aggregates (daily/minute), US equities/ETFs, many quant shops use it | Real-time / full SIP often needs higher tier; read their redistribution terms |
| **Alpaca Market Data** | Easy if you already trade on Alpaca; bundled data with brokerage | Coverage/terms tied to Alpaca account |
| **Tiingo** | Inexpensive end-of-day + some intraday | Different API shape; check your use case |
| **IEX Cloud** | Was popular for starter pricing | **Verify current product** — IEX scaled back consumer API; don’t assume old pricing |

**Not a vendor of record for serious compliance:** Yahoo/`yfinance` stays as **unofficial** fallback in `prices.py`.

## This codebase

- **Polygon** (optional): `helios_alpha.ingest.polygon` + `pipeline.market.provider=polygon`
- **Fallback:** `pipeline.market.provider=yfinance` (default)

Always store API keys in env (e.g. `.env` with `HELIOS_POLYGON_API_KEY`), never in git.
