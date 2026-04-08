# Pendulum and pandas

**Policy:** Use **Pendulum** for parsing ISO strings, building UTC instants, timezone normalization, and calendar math. Use stdlib **`datetime.date`** only at IO boundaries (Polars `Date`, Hydra YAML, yfinance).

## Helpers

All live in `helios_alpha.utils.time`:

- `parse_iso_z` / `parse_date_iso` — API and config strings → Pendulum or `date`
- `utc_datetime`, `start_of_utc_day`, `from_unix_epoch_*` — construction without `datetime(..., tzinfo=UTC)`
- `in_utc` — normalize any datetime-like to UTC Pendulum
- `to_pandas_timestamp` — Pendulum → `pandas.Timestamp` (UTC) for `exchange_calendars`

Pendulum documents pandas compatibility (e.g. `Period` / conversions); for us the main bridge is **explicit** `to_pandas_timestamp` when a library requires `Timestamp`.

## Clock

`helios_alpha.timekeeping` exposes `now_utc()` as `pendulum.DateTime`. `today_utc()` remains a stdlib `date` for simple comparisons with Polars `Date` columns.
