import { test } from "node:test";
import assert from "node:assert/strict";
import { defaultIndicators, deriveIndicators, indicatorId, indicatorSource, parseIndicators, type IndicatorSettings } from "../../src/operations/series-indicators";
import type { TimeSeriesData, TimeSeriesDescriptor, TimeSeriesScalarPoint } from "../../src/operations/time-series-port";
const descriptor: TimeSeriesDescriptor = {id: "test:flux", label: "Flux", shortLabel: "Flux", domain: "source", unit: "W/m2", precision: 9, color: "#fff", render: "line", provenance: "fixture", freshness: "observed", defaultVisible: true};
const settings = {...defaultIndicators(descriptor.id), deviation: true, macd: true, span: 3, window: 3, fast: 2, slow: 3, signal: 2};
function source(values: number[]): TimeSeriesData { return {descriptor, points: values.map((value, i) => ({kind: "scalar", value, timestamp: new Date(1_700_000_000_000 + i * 60_000).toISOString(), availableAt: new Date(1_700_000_000_000 + i * 60_000).toISOString()}))}; }
function results(values: number[], config: IndicatorSettings = settings) { return new Map(deriveIndicators(source(values), config).map(series => [series.descriptor.id.split(":").at(-1), series.points as TimeSeriesScalarPoint[]])); }
const close = (actual: number, expected: number, tolerance = 1e-10) => assert.ok(Math.abs(actual - expected) <= tolerance, `${actual} != ${expected}`);
test("EWMA and population statistics match hand-calculated values after warm-up", () => {
  const output = results([1,2,3,4,5]);
  assert.deepEqual(output.get("ewma")!.map(p => p.value), [2.25,3.125,4.0625]);
  output.get("deviation")!.forEach(p => close(p.value, Math.sqrt(2/3)));
  output.get("zscore")!.forEach(p => close(p.value, Math.sqrt(1.5)));
  assert.equal(output.get("signal")!.length, 2);
  assert.equal(output.get("histogram")!.length, 2);
  close(output.get("macd")![0].value, 11/36);
  close(output.get("signal")![0].value, (11/36)/3 + (85/216)*2/3);
  close(output.get("histogram")![0].value, 85/216 - output.get("signal")![0].value);
});
test("constant signals are exactly zero-centered, including tiny fluxes", () => {
  for (const value of [0,1e-12,1e-7,42,1e12]) {
    const output = results(Array(90).fill(value));
    for (const kind of ["zscore","deviation","macd","signal","histogram"]) assert.ok(output.get(kind)!.every(point => point.value === 0));
  }
});
test("z-score is scale invariant and deviation scales with the source across generated signals", () => {
  let seed = 12345;
  for (let trial = 0; trial < 40; trial++) {
    const values = Array.from({length: 80}, () => { seed = (seed * 1664525 + 1013904223) >>> 0; return seed / 2**32 * 10; });
    const base = results(values);
    const tiny = results(values.map(value => value * 1e-9));
    const shifted = results(values.map(value => value + 10000));
    base.get("zscore")!.forEach((point, i) => { close(tiny.get("zscore")![i].value, point.value); close(shifted.get("zscore")![i].value, point.value, 1e-8); });
    base.get("deviation")!.forEach((point, i) => close(tiny.get("deviation")![i].value / 1e-9, point.value));
  }
});
test("future observations never revise earlier indicators and availability follows dependencies", () => {
  const initial = source([1,2,3,4,5]);
  const extended = source([1,2,3,4,5,100000,-900000]);
  const before = deriveIndicators(initial, settings), after = deriveIndicators(extended, settings);
  before.forEach((series, i) => assert.deepEqual(after[i].points.slice(0, series.points.length), series.points));
  const delayed = {...initial, points: initial.points.map((point, i) => i === 0 ? {...point, availableAt: new Date(1_700_001_000_000).toISOString()} : point)};
  const delayedResults = new Map(deriveIndicators(delayed, settings).map(series => [series.descriptor.id.split(":").at(-1), series.points]));
  assert.equal(delayedResults.get("ewma")!.at(-1)!.availableAt, delayed.points[0].availableAt);
  assert.equal(delayedResults.get("zscore")![0].availableAt, delayed.points[0].availableAt);
  assert.equal(delayedResults.get("zscore")!.at(-1)!.availableAt, initial.points.at(-1)!.availableAt);
});
test("insufficient history stays empty; gaps are not interpolated; invalid settings are discarded", () => {
  for (const points of [[],[5],[5,6]]) assert.ok(deriveIndicators(source(points), settings).every(series => !series.points.length));
  const gapped = source([1,2,3,4]);
  const output = deriveIndicators({...gapped, points: [gapped.points[0],gapped.points[2],gapped.points[3]]}, settings);
  assert.equal(output[0].points[0].timestamp, gapped.points[3].timestamp);
  assert.deepEqual(parseIndicators([settings,{...settings,sourceId:"missing"},{...settings,fast:30},{...settings,span:1},{...settings,window:2.5}], [descriptor]), [settings]);
  assert.equal(indicatorSource(indicatorId(descriptor.id, "ewma")), descriptor.id);
});
