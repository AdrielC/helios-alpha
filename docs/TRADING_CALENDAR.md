# Trading calendar and session alignment

## Goals

1. **Exchange truth** for holidays and session open/close (DST-aware): `exchange_calendars` (`XNYS` by default).
2. **Pandas** for `CustomBusinessDay` / `CustomBusinessHour` when you need offsets in **trading time**.
3. **Pendulum** for parsing and “wall clock” in ingest (`Clock` in `helios_alpha/timekeeping.py`).
4. **Causal backtests**: `pipeline.as_of_date` caps which flare rows and price bars are visible.

## Code map

| Piece | Location |
|-------|----------|
| Calendar wrapper | `helios_alpha/markets/trading_calendar.py` |
| Hydra defaults | `src/helios_alpha/conf/pipeline/default.yaml` (`trading.*`, `paths.markets`) |
| Event merge: RTH session label from flare peak | `merge_events.build_event_table(..., trading_calendar=)` → `event_session_date` |
| Event study: filter to sessions, session-based horizons | `run_event_study(..., as_of=, trading_calendar=, filter_events_to_sessions=, use_session_horizons=)` |
| Polars → Pandas for calendar ops | `helios_alpha/ingest/prices_pandas.py` |

## Custom business hours

- `TradingCalendar.custom_business_hour_regular()` — continuous NYSE RTH (09:30–16:00 **local**), holidays from calendar.
- **Lunch / auction breaks**: XNYS regular schedule in `exchange_calendars` has `NaT` break columns. For venues with real breaks, use `session_break_start_utc` / `session_break_end_utc` per day and build a **multi-interval** model (not a single `CustomBusinessHour`).

## Hydra examples

```bash
helios-pipeline pipeline.as_of_date=2023-12-31 pipeline.end_date=2024-12-31
helios-pipeline pipeline.trading.filter_events_to_sessions=false
```

## Rust (later)

High-value extension points:

- Rolling / streaming join of **minute bars** to **session flags** (bitset per day).
- Fast bootstrap or permutation tests over large panels (Rust + `pyo3` / `polars` UDFs).
- Order-book microstructure features if you add tick data.

Keep Python orchestration; move only hot inner loops to Rust once profiles justify it.
