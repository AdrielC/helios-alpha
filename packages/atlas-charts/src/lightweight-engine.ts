import {
  AreaSeries,
  BarSeries,
  CandlestickSeries,
  ColorType,
  CrosshairMode,
  HistogramSeries,
  LineSeries,
  LineStyle,
  createChart,
  type IChartApi,
  type ISeriesApi,
  type MouseEventParams,
  type SeriesType,
  type Time,
  type UTCTimestamp,
} from "lightweight-charts";
import {
  isAtlasOhlcPoint,
  type AtlasChartController,
  type AtlasChartModel,
  type AtlasChartOptions,
  type AtlasCursorSnapshot,
  type AtlasPoint,
  type AtlasSeriesDefinition,
  type AtlasVisibleRange,
} from "./contracts";
import { atlasChartTheme } from "./theme";
import { atlasModelRange, atlasOhlcPoints, atlasPaneIndexById, atlasSampleFromDatum, atlasScalarPoints, nearestAtlasPoint, retainedVisibleRange } from "./model";

type AnySeries = ISeriesApi<SeriesType, Time>;

function utc(seconds: number): UTCTimestamp {
  return Math.floor(seconds) as UTCTimestamp;
}

function rgba(hex: string, alpha: number): string {
  const value = hex.replace("#", "");
  const full = value.length === 3 ? value.split("").map((character) => character + character).join("") : value;
  const number = Number.parseInt(full, 16);
  return `rgba(${number >> 16}, ${(number >> 8) & 255}, ${number & 255}, ${alpha})`;
}

function formatPrice(series: AtlasSeriesDefinition): { type: "price"; precision: number; minMove: number } | { type: "volume" } {
  if (series.kind === "histogram" && series.unit.toLowerCase().includes("volume")) return { type: "volume" };
  return { type: "price", precision: series.precision, minMove: 10 ** -series.precision };
}

function scalarData(points: readonly AtlasPoint[]) {
  return atlasScalarPoints(points).map((point) => ({ time: utc(point.time), value: point.value, color: point.color }));
}

function ohlcData(points: readonly AtlasPoint[]) {
  return atlasOhlcPoints(points).map((point) => ({ time: utc(point.time), open: point.open, high: point.high, low: point.low, close: point.close }));
}

function addSeries(chart: IChartApi, definition: AtlasSeriesDefinition, paneIndex: number): AnySeries {
  const common = {
    title: "",
    priceFormat: formatPrice(definition),
    priceLineVisible: false,
    lastValueVisible: true,
    crosshairMarkerVisible: true,
  };

  if (definition.kind === "candlestick") {
    const series = chart.addSeries(CandlestickSeries, {
      ...common,
      upColor: atlasChartTheme.up,
      downColor: atlasChartTheme.down,
      borderUpColor: atlasChartTheme.up,
      borderDownColor: atlasChartTheme.down,
      wickUpColor: rgba(atlasChartTheme.up, 0.82),
      wickDownColor: rgba(atlasChartTheme.down, 0.82),
    }, paneIndex);
    series.setData(ohlcData(definition.data));
    return series as unknown as AnySeries;
  }

  if (definition.kind === "bar") {
    const series = chart.addSeries(BarSeries, {
      ...common,
      upColor: atlasChartTheme.up,
      downColor: atlasChartTheme.down,
      thinBars: true,
    }, paneIndex);
    series.setData(ohlcData(definition.data));
    return series as unknown as AnySeries;
  }

  if (definition.kind === "histogram") {
    const series = chart.addSeries(HistogramSeries, {
      ...common,
      color: definition.color,
      base: 0,
    }, paneIndex);
    series.setData(scalarData(definition.data));
    return series as unknown as AnySeries;
  }

  if (definition.kind === "area") {
    const series = chart.addSeries(AreaSeries, {
      ...common,
      lineColor: definition.color,
      lineWidth: 1,
      topColor: rgba(definition.color, 0.18),
      bottomColor: rgba(definition.color, 0.01),
      crosshairMarkerRadius: 3,
    }, paneIndex);
    series.setData(scalarData(definition.data));
    return series as unknown as AnySeries;
  }

  const series = chart.addSeries(LineSeries, {
    ...common,
    color: definition.color,
    lineWidth: 1,
    lineStyle: definition.kind === "baseline" ? LineStyle.Dashed : LineStyle.Solid,
    crosshairMarkerRadius: 3,
  }, paneIndex);
  series.setData(scalarData(definition.data));
  return series as unknown as AnySeries;
}

export function createAtlasChart(container: HTMLElement, options: AtlasChartOptions): AtlasChartController {
  const chart = createChart(container, {
    autoSize: true,
    layout: {
      background: { type: ColorType.Solid, color: atlasChartTheme.ground },
      textColor: atlasChartTheme.text,
      fontFamily: '"Recursive Variable", ui-monospace, monospace',
      fontSize: 9,
      attributionLogo: false,
      panes: {
        separatorColor: atlasChartTheme.border,
        separatorHoverColor: rgba(atlasChartTheme.blue, 0.35),
        enableResize: true,
      },
    },
    grid: {
      vertLines: { color: atlasChartTheme.grid, style: LineStyle.Solid },
      horzLines: { color: atlasChartTheme.grid, style: LineStyle.Solid },
    },
    crosshair: {
      mode: CrosshairMode.Normal,
      vertLine: { color: atlasChartTheme.crosshair, width: 1, style: LineStyle.Dashed, labelBackgroundColor: atlasChartTheme.crosshair },
      horzLine: { color: rgba(atlasChartTheme.crosshair, 0.32), width: 1, style: LineStyle.Dotted, labelBackgroundColor: atlasChartTheme.border },
    },
    leftPriceScale: { visible: false },
    rightPriceScale: {
      visible: true,
      borderColor: atlasChartTheme.border,
      minimumWidth: 58,
      scaleMargins: { top: 0.12, bottom: 0.12 },
    },
    timeScale: {
      borderColor: atlasChartTheme.border,
      timeVisible: true,
      secondsVisible: true,
      rightOffset: 0,
      barSpacing: 5,
      minBarSpacing: 0.6,
      fixLeftEdge: true,
      lockVisibleTimeRangeOnResize: true,
    },
    handleScroll: { mouseWheel: true, pressedMouseMove: true, horzTouchDrag: true, vertTouchDrag: false },
    handleScale: { axisPressedMouseMove: true, mouseWheel: true, pinch: true },
    kineticScroll: { mouse: true, touch: true },
  });

  let model = options.model;
  let seriesById = new Map<string, AnySeries>();
  let idBySeries = new Map<unknown, string>();
  let suppressCrosshairUntil = 0;

  const paneWeights = (): readonly number[] => chart.panes().map((pane) => pane.getStretchFactor());
  const reportPaneWeights = (): void => {
    window.requestAnimationFrame(() => options.onPaneWeights?.(paneWeights()));
  };

  const render = (next: AtlasChartModel): void => {
    for (const handle of seriesById.values()) chart.removeSeries(handle);
    seriesById = new Map();
    idBySeries = new Map();
    model = next;
    const paneIndexById = atlasPaneIndexById(next);
    for (const definition of next.series) {
      const paneIndex = paneIndexById.get(definition.paneId) ?? 0;
      const handle = addSeries(chart, definition, paneIndex);
      seriesById.set(definition.id, handle);
      idBySeries.set(handle, definition.id);
    }
    for (const [index, pane] of chart.panes().entries()) pane.setStretchFactor(next.panes[index]?.weight ?? 1);
  };

  const onCrosshair = (param: MouseEventParams<Time>): void => {
    if (performance.now() < suppressCrosshairUntil) return;
    if (typeof param.time !== "number") {
      options.onCursor?.(undefined);
      return;
    }
    const samples: Record<string, NonNullable<ReturnType<typeof atlasSampleFromDatum>>> = {};
    for (const [handle, data] of param.seriesData.entries()) {
      const id = idBySeries.get(handle);
      const sample = atlasSampleFromDatum(data);
      if (id && sample) samples[id] = sample;
    }
    options.onCursor?.({ time: param.time, samples });
  };

  render(model);
  if (options.visibleRange) chart.timeScale().setVisibleRange({ from: utc(options.visibleRange.from), to: utc(options.visibleRange.to) });
  else chart.timeScale().fitContent();
  chart.subscribeCrosshairMove(onCrosshair);
  container.addEventListener("pointerup", reportPaneWeights);
  chart.timeScale().subscribeVisibleTimeRangeChange((range) => {
    options.onVisibleRange?.(range && typeof range.from === "number" && typeof range.to === "number" ? { from: range.from, to: range.to } : undefined);
  });

  return {
    setModel(next) {
      const previous = chart.timeScale().getVisibleRange();
      const retained = retainedVisibleRange(
        previous && typeof previous.from === "number" && typeof previous.to === "number" ? { from: previous.from, to: previous.to } : undefined,
        atlasModelRange(next),
      );
      render(next);
      if (retained) chart.timeScale().setVisibleRange({ from: utc(retained.from), to: utc(retained.to) });
      else chart.timeScale().fitContent();
    },
    setCursor(time) {
      const first = model.series[0];
      const handle = first ? seriesById.get(first.id) : undefined;
      if (!first || !handle) return;
      const nearest = nearestAtlasPoint(first.data, time);
      if (!nearest) return;
      suppressCrosshairUntil = performance.now() + 100;
      chart.setCrosshairPosition(isAtlasOhlcPoint(nearest) ? nearest.close : nearest.value, utc(nearest.time), handle);
    },
    clearCursor() {
      chart.clearCrosshairPosition();
    },
    setVisibleRange(range) {
      chart.timeScale().setVisibleRange({ from: utc(range.from), to: utc(range.to) });
    },
    getVisibleRange() {
      const range = chart.timeScale().getVisibleRange();
      return range && typeof range.from === "number" && typeof range.to === "number" ? { from: range.from, to: range.to } : undefined;
    },
    getPaneWeights() {
      return paneWeights();
    },
    destroy() {
      chart.unsubscribeCrosshairMove(onCrosshair);
      container.removeEventListener("pointerup", reportPaneWeights);
      chart.remove();
    },
  };
}
