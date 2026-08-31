# Atlas Charts

`@helios-alpha/atlas-charts` is the reusable TypeScript chart boundary for Helios operator surfaces. Applications describe panes and series with plain data. The package owns the rendering engine, synchronized time scale, crosshair samples, viewport retention, and financial-series styling.

The public model is independent of Vue, OMS records, strategy concepts, and transport code. That keeps chart composition portable while allowing the rendering adapter to change without rewriting application ports.

## Model

```ts
import { createAtlasChart, type AtlasChartModel } from "@helios-alpha/atlas-charts";

const model: AtlasChartModel = {
  panes: [
    { id: "market", weight: 1.6 },
    { id: "volume", weight: 0.8 },
  ],
  series: [
    {
      id: "price",
      paneId: "market",
      label: "Price",
      kind: "candlestick",
      color: "#78a9ef",
      unit: "USD",
      precision: 2,
      data: [{ time: 1_725_000_000, open: 4980, high: 4984, low: 4979, close: 4983 }],
    },
    {
      id: "volume",
      paneId: "volume",
      label: "Volume",
      kind: "histogram",
      color: "#4f94ee",
      unit: "volume",
      precision: 0,
      data: [{ time: 1_725_000_000, value: 12_842 }],
    },
  ],
};

const chart = createAtlasChart(container, {
  model,
  onCursor: (snapshot) => renderEvidence(snapshot),
  onVisibleRange: (range) => synchronizeNavigator(range),
});
```

`setModel` retains the visible range when the replacement model overlaps the current domain. `setVisibleRange` drives zoom from a global navigator. `setCursor` moves every pane to the same evidence cut.

## Series kinds

- `candlestick` and `bar` accept OHLC points.
- `histogram`, `line`, `area`, and `baseline` accept scalar points.
- Multiple series with one `paneId` are overlays.
- Different `paneId` values create independently resizable panes on one shared time scale.

## Verification

Run `npm run atlas:test` from the repository root. The deterministic suite covers scalar and OHLC separation, cursor sampling, overlay placement, scrubber boundaries, and visible-range retention.

The current adapter uses [TradingView Lightweight Charts](https://www.tradingview.com/lightweight-charts/). Engine-specific types remain inside this package.
