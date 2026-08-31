export type AtlasSeriesKind = "candlestick" | "bar" | "histogram" | "line" | "area" | "baseline";

export interface AtlasScalarPoint {
  readonly time: number;
  readonly value: number;
  readonly color?: string;
}

export interface AtlasOhlcPoint {
  readonly time: number;
  readonly open: number;
  readonly high: number;
  readonly low: number;
  readonly close: number;
}

export type AtlasPoint = AtlasScalarPoint | AtlasOhlcPoint;

export interface AtlasPaneDefinition {
  readonly id: string;
  readonly weight?: number;
}

export interface AtlasSeriesDefinition {
  readonly id: string;
  readonly paneId: string;
  readonly label: string;
  readonly kind: AtlasSeriesKind;
  readonly color: string;
  readonly unit: string;
  readonly precision: number;
  readonly data: readonly AtlasPoint[];
}

export interface AtlasChartModel {
  readonly panes: readonly AtlasPaneDefinition[];
  readonly series: readonly AtlasSeriesDefinition[];
}

export interface AtlasVisibleRange {
  readonly from: number;
  readonly to: number;
}

export interface AtlasScalarSample {
  readonly kind: "scalar";
  readonly value: number;
}

export interface AtlasOhlcSample {
  readonly kind: "ohlc";
  readonly open: number;
  readonly high: number;
  readonly low: number;
  readonly close: number;
}

export type AtlasSample = AtlasScalarSample | AtlasOhlcSample;

export interface AtlasCursorSnapshot {
  readonly time: number;
  readonly samples: Readonly<Record<string, AtlasSample>>;
}

export interface AtlasChartOptions {
  readonly model: AtlasChartModel;
  readonly visibleRange?: AtlasVisibleRange;
  readonly onCursor?: (snapshot: AtlasCursorSnapshot | undefined) => void;
  readonly onVisibleRange?: (range: AtlasVisibleRange | undefined) => void;
  readonly onPaneWeights?: (weights: readonly number[]) => void;
}

export interface AtlasChartController {
  setModel(model: AtlasChartModel): void;
  setCursor(time: number): void;
  clearCursor(): void;
  setVisibleRange(range: AtlasVisibleRange): void;
  getVisibleRange(): AtlasVisibleRange | undefined;
  getPaneWeights(): readonly number[];
  destroy(): void;
}

export function isAtlasOhlcPoint(point: AtlasPoint): point is AtlasOhlcPoint {
  return "open" in point;
}
