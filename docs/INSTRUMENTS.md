# Internal instrument ids and provider mapping

## Model

1. **`config/instruments.yaml`** — canonical **`id`** per line of business (e.g. `VIX`, `SPY`). Under each id, optional **`yfinance`**, **`polygon`**, … keys hold that vendor’s symbol.
2. **`config/assets.yaml`** — **`universe`** groups list only **ids** (no carets, no `I:` prefixes).
3. **Parquet / event study** — the **`ticker`** column is always the **canonical id**.

Add a new name:

- Append an entry under `instruments:` with `id` and each provider you use.
- Add the `id` to a bucket in `assets.yaml`.

## Example

```yaml
# instruments.yaml
instruments:
  - id: VIX
    yfinance: ^VIX
    polygon: I:VIX   # confirm against your Polygon license
```

## Polygon URL encoding

Symbols like `I:VIX` are path-encoded when calling Polygon’s REST API.
