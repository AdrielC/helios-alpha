import assert from "node:assert/strict";
import test from "node:test";
import {
  atlasModelRange,
  atlasOhlcPoints,
  atlasPaneIndexById,
  atlasSampleFromDatum,
  atlasScalarPoints,
  clampScrubRange,
  nearestAtlasPoint,
  retainedVisibleRange,
  type AtlasChartModel,
  type AtlasPoint,
} from "../src/index";

const points: readonly AtlasPoint[] = [
  { time: 10, value: 2, color: "#fff" },
  { time: 20, open: 2, high: 4, low: 1, close: 3 },
  { time: 30, value: 7 },
];

test("separates scalar and OHLC points without lossy coercion", () => {
  assert.deepEqual(atlasScalarPoints(points), [points[0], points[2]]);
  assert.deepEqual(atlasOhlcPoints(points), [points[1]]);
});

test("creates deterministic cursor samples and nearest observations", () => {
  assert.deepEqual(nearestAtlasPoint(points, 23), points[1]);
  assert.deepEqual(atlasSampleFromDatum(points[0]), { kind: "scalar", value: 2 });
  assert.deepEqual(atlasSampleFromDatum(points[1]), { kind: "ohlc", open: 2, high: 4, low: 1, close: 3 });
  assert.equal(atlasSampleFromDatum({ value: Number.NaN }), undefined);
});

test("retains visible ranges across model changes and clamps changed domains", () => {
  assert.deepEqual(retainedVisibleRange({ from: 20, to: 40 }, { from: 0, to: 100 }), { from: 20, to: 40 });
  assert.deepEqual(retainedVisibleRange({ from: 80, to: 120 }, { from: 0, to: 100 }), { from: 80, to: 100 });
  assert.deepEqual(retainedVisibleRange({ from: 120, to: 140 }, { from: 0, to: 100 }), { from: 0, to: 100 });
});

test("maps overlays to one pane while preserving independent panes", () => {
  const model: AtlasChartModel = {
    panes: [{ id: "market" }, { id: "risk" }],
    series: [
      { id: "price", paneId: "market", label: "Price", kind: "candlestick", color: "#fff", unit: "USD", precision: 2, data: points.slice(1, 2) },
      { id: "volume", paneId: "market", label: "Volume", kind: "histogram", color: "#fff", unit: "volume", precision: 0, data: points.slice(0, 1) },
      { id: "risk", paneId: "risk", label: "Risk", kind: "line", color: "#fff", unit: "%", precision: 2, data: points.slice(2) },
    ],
  };
  const panes = atlasPaneIndexById(model);
  assert.equal(panes.get("market"), 0);
  assert.equal(panes.get("risk"), 1);
  assert.deepEqual(atlasModelRange(model), { from: 10, to: 30 });
});

test("clamps both scrub handles and enforces the minimum interval", () => {
  assert.deepEqual(clampScrubRange("start", 990, 100, 600), { start: 585, end: 600 });
  assert.deepEqual(clampScrubRange("end", -20, 400, 800), { start: 400, end: 415 });
  assert.deepEqual(clampScrubRange("end", 1_200, 400, 800), { start: 400, end: 1_000 });
});
