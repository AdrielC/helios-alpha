import type { TimelineLane, TimeSeriesData, TimeSeriesDescriptor, TimeSeriesScalarPoint } from "./time-series-port";

export interface IndicatorSettings {
  readonly sourceId: string;
  readonly ewma: boolean;
  readonly zscore: boolean;
  readonly deviation: boolean;
  readonly macd: boolean;
  readonly span: number;
  readonly window: number;
  readonly fast: number;
  readonly slow: number;
  readonly signal: number;
}
export type IndicatorKind = "ewma" | "zscore" | "deviation" | "macd" | "signal" | "histogram";
export const defaultIndicators = (sourceId: string): IndicatorSettings => ({ sourceId, ewma: true, zscore: true, deviation: false, macd: false, span: 20, window: 60, fast: 12, slow: 26, signal: 9 });
export const indicatorId = (sourceId: string, kind: IndicatorKind): string => `indicator:${encodeURIComponent(sourceId)}:${kind}`;
export function indicatorSource(id: string): string {
  if (!id.startsWith("indicator:")) return id;
  try { return decodeURIComponent(id.slice(10, id.lastIndexOf(":"))); } catch { return id; }
}
export function indicatorError(settings: IndicatorSettings): string {
  if (![settings.span, settings.window, settings.fast, settings.slow, settings.signal].every(n => Number.isInteger(n) && n >= 2 && n <= 500)) return "Periods must be whole numbers from 2 to 500.";
  if (settings.fast >= settings.slow) return "MACD fast period must be shorter than its slow period.";
  return "";
}
export function parseIndicators(value: unknown, catalog: readonly TimeSeriesDescriptor[]): IndicatorSettings[] {
  if (!Array.isArray(value)) return [];
  const validIds = new Set(catalog.map(item => item.id));
  const result: IndicatorSettings[] = [];
  for (const candidate of value.slice(0, 12)) {
    if (!candidate || typeof candidate !== "object") continue;
    const item = candidate as IndicatorSettings;
    if (!validIds.has(item.sourceId) || result.some(saved => saved.sourceId === item.sourceId) || indicatorError(item)) continue;
    if (![item.ewma, item.zscore, item.deviation, item.macd].every(flag => typeof flag === "boolean")) continue;
    if (item.ewma || item.zscore || item.deviation || item.macd) result.push({ sourceId: item.sourceId, ewma: item.ewma, zscore: item.zscore, deviation: item.deviation, macd: item.macd, span: item.span, window: item.window, fast: item.fast, slow: item.slow, signal: item.signal });
  }
  return result;
}
export function indicatorDescriptors(source: TimeSeriesDescriptor, settings: IndicatorSettings): TimeSeriesDescriptor[] {
  const rows: [IndicatorKind, string, string, boolean][] = [
    ["ewma", `EWMA ${settings.span}`, "#7db8ff", settings.ewma],
    ["zscore", `Z-score ${settings.window}`, "#bb9aff", settings.zscore],
    ["deviation", `Std dev ${settings.window}`, "#63d4cb", settings.deviation],
    ["histogram", "Histogram", "#5dbca3", settings.macd],
    ["macd", `MACD ${settings.fast}/${settings.slow}`, "#7db8ff", settings.macd],
    ["signal", `Signal ${settings.signal}`, "#f6c577", settings.macd],
  ];
  return rows.filter(row => row[3]).map(([kind, label, color]) => ({ ...source,
    id: indicatorId(source.id, kind), label: `${source.label} · ${label}`, shortLabel: label,
    unit: kind === "zscore" ? "σ" : source.unit,
    precision: kind === "zscore" ? 2 : Math.min(12, Math.max(4, source.precision + 2)),
    color, render: kind === "histogram" ? "histogram" : "line", defaultVisible: false,
    provenance: `${source.label} · ${kind === "zscore" || kind === "deviation" ? "trailing population statistics" : "exponential smoothing"}`,
  }));
}
export function indicatorLanes(settings: IndicatorSettings): TimelineLane[] {
  const id = (kind: IndicatorKind) => indicatorId(settings.sourceId, kind);
  return [
    { id: `lane-${settings.sourceId}`, seriesIds: [settings.sourceId, ...(settings.ewma ? [id("ewma")] : [])], weight: 1.4 },
    ...(settings.zscore ? [{ id: `lane-${id("zscore")}`, seriesIds: [id("zscore")], weight: 1 }] : []),
    ...(settings.deviation ? [{ id: `lane-${id("deviation")}`, seriesIds: [id("deviation")], weight: 1 }] : []),
    ...(settings.macd ? [{ id: `lane-${id("macd")}`, seriesIds: [id("histogram"), id("macd"), id("signal")], weight: 1.4 }] : []),
  ];
}

/** Causal studies over retained observations. Periods count samples, never interpolated time. */
export function deriveIndicators(source: TimeSeriesData, settings: IndicatorSettings): TimeSeriesData[] {
  if (indicatorError(settings)) throw new Error(indicatorError(settings));
  const descriptors = indicatorDescriptors(source.descriptor, settings);
  const output = new Map(descriptors.map(descriptor => [descriptor.id, [] as TimeSeriesScalarPoint[]]));
  // Preserve gaps. Revisions at one timestamp replace that sample rather than count twice.
  const samples = [...new Map(source.points.filter(point => Number.isFinite(Date.parse(point.timestamp)) && Number.isFinite(Date.parse(point.availableAt)) && Number.isFinite(point.kind === "ohlc" ? point.close : point.value)).map(point => [point.timestamp, point])).values()].sort((a, b) => Date.parse(a.timestamp) - Date.parse(b.timestamp));
  let ewma = 0, fast = 0, slow = 0, signal = 0, available = 0;
  const values: number[] = [];
  const availability: number[] = [];
  for (const [index, point] of samples.entries()) {
    const value = point.kind === "ohlc" ? point.close : point.value;
    available = Math.max(available, Date.parse(point.availableAt));
    const emit = (kind: IndicatorKind, value: number, availableAt = available) => {
      if (!Number.isFinite(value)) return;
      output.get(indicatorId(settings.sourceId, kind))?.push({ kind: "scalar", timestamp: point.timestamp, availableAt: new Date(availableAt).toISOString(), value, ...(kind === "histogram" ? { color: value >= 0 ? "#5dbca399" : "#ec8f8f99" } : {}) });
    };
    ewma = index === 0 ? value : ewma + 2 / (settings.span + 1) * (value - ewma);
    fast = index === 0 ? value : fast + 2 / (settings.fast + 1) * (value - fast);
    slow = index === 0 ? value : slow + 2 / (settings.slow + 1) * (value - slow);
    if (index + 1 >= settings.span) emit("ewma", ewma);
    if (index + 1 >= settings.slow) {
      const macd = fast - slow;
      signal = index + 1 === settings.slow ? macd : signal + 2 / (settings.signal + 1) * (macd - signal);
      emit("macd", macd);
      if (index + 1 >= settings.slow + settings.signal - 1) { emit("signal", signal); emit("histogram", macd - signal); }
    }
    if (settings.zscore || settings.deviation) {
      values.push(value); availability.push(Date.parse(point.availableAt));
      if (values.length > settings.window) { values.shift(); availability.shift(); }
      if (values.length === settings.window) {
        // Center before summing to preserve variance in tiny fluxes and large-offset series.
        const origin = values[0];
        const meanOffset = values.reduce((sum, item) => sum + (item - origin), 0) / values.length;
        const deviation = Math.sqrt(values.reduce((sum, item) => sum + ((item - origin) - meanOffset) ** 2, 0) / values.length);
        const statsAvailable = Math.max(...availability);
        emit("deviation", deviation, statsAvailable);
        emit("zscore", deviation === 0 ? 0 : ((value - origin) - meanOffset) / deviation, statsAvailable);
      }
    }
  }
  return descriptors.map(descriptor => ({ descriptor, points: output.get(descriptor.id)! }));
}
