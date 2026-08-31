import {
  isAtlasOhlcPoint,
  type AtlasChartModel,
  type AtlasOhlcPoint,
  type AtlasPoint,
  type AtlasSample,
  type AtlasScalarPoint,
  type AtlasVisibleRange,
} from "./contracts";

export function atlasScalarPoints(points: readonly AtlasPoint[]): readonly AtlasScalarPoint[] {
  return points.filter((point): point is AtlasScalarPoint => !isAtlasOhlcPoint(point));
}

export function atlasOhlcPoints(points: readonly AtlasPoint[]): readonly AtlasOhlcPoint[] {
  return points.filter(isAtlasOhlcPoint);
}

export function nearestAtlasPoint(points: readonly AtlasPoint[], time: number): AtlasPoint | undefined {
  return points.reduce<AtlasPoint | undefined>((best, point) => !best || Math.abs(point.time - time) < Math.abs(best.time - time) ? point : best, undefined);
}

export function atlasSampleFromDatum(data: unknown): AtlasSample | undefined {
  if (!data || typeof data !== "object") return undefined;
  const datum = data as Record<string, unknown>;
  if ([datum.open, datum.high, datum.low, datum.close].every((value) => typeof value === "number" && Number.isFinite(value))) {
    return { kind: "ohlc", open: datum.open as number, high: datum.high as number, low: datum.low as number, close: datum.close as number };
  }
  if (typeof datum.value === "number" && Number.isFinite(datum.value)) return { kind: "scalar", value: datum.value };
  return undefined;
}

export function atlasModelRange(model: AtlasChartModel): AtlasVisibleRange | undefined {
  const times = model.series.flatMap((series) => series.data.map((point) => point.time));
  if (!times.length) return undefined;
  return { from: Math.min(...times), to: Math.max(...times) };
}

export function retainedVisibleRange(previous: AtlasVisibleRange | undefined, domain: AtlasVisibleRange | undefined): AtlasVisibleRange | undefined {
  if (!domain) return undefined;
  if (!previous) return domain;
  const from = Math.max(previous.from, domain.from);
  const to = Math.min(previous.to, domain.to);
  return from < to ? { from, to } : domain;
}

export function atlasPaneIndexById(model: AtlasChartModel): ReadonlyMap<string, number> {
  return new Map(model.panes.map((pane, index) => [pane.id, index]));
}

export function clampScrubRange(edge: "start" | "end", value: number, start: number, end: number, minimumGap = 15): { start: number; end: number } {
  const bounded = Math.max(0, Math.min(1_000, value));
  if (edge === "start") return { start: Math.min(bounded, end - minimumGap), end };
  return { start, end: Math.max(bounded, start + minimumGap) };
}
