<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import {
  createAtlasChart,
  clampScrubRange,
  type AtlasChartController,
  type AtlasChartModel,
  type AtlasCursorSnapshot,
  type AtlasPoint,
  type AtlasSeriesKind,
} from "@helios-alpha/atlas-charts";
import type { OperationsSnapshot } from "../operations/operations-port";
import type { InvestigationPort, InvestigationResult } from "../operations/investigation-port";
import {
  defaultTimelineWorkspace,
  type ForecastBundle,
  type MarkerKind,
  type SeriesTransform,
  type TimelineLane,
  type TimelineMarker,
  type TimelineWorkspace,
  type TimeSeriesData,
  type TimeSeriesDescriptor,
  type TimeSeriesPoint,
  type TimeSeriesPort,
  type TimeSeriesWindow,
} from "../operations/time-series-port";

const props = defineProps<{
  snapshot: OperationsSnapshot;
  port: TimeSeriesPort;
  investigationPort: InvestigationPort;
  selectedOrderId: string;
  selectedForecastId?: string;
}>();
const emit = defineEmits<{ selectOrder: [id: string] }>();

const chartHost = ref<HTMLElement>();
const catalog = ref<readonly TimeSeriesDescriptor[]>([]);
const forecastBundles = ref<readonly ForecastBundle[]>([]);
const dataWindow = ref<TimeSeriesWindow>();
const workspace = ref<TimelineWorkspace>();
const loading = ref(true);
const error = ref("");
const seriesOpen = ref(false);
const search = ref("");
const cursorRatio = ref(630);
const cursorTime = ref<number>();
const selectionStart = ref(430);
const selectionEnd = ref(650);
const visibleRange = ref<{ from: number; to: number }>();
const investigating = ref(false);
const investigation = ref<InvestigationResult>();
const investigationError = ref("");
const investigationDialog = ref<HTMLDialogElement>();
const draggedLaneId = ref<string>();
let chart: AtlasChartController | undefined;
let loadedWindowMinutes: number | undefined;
let contextLoadGeneration = 0;

const activeSeriesIds = computed(() => workspace.value?.lanes.flatMap((lane) => lane.seriesIds) ?? []);
const activeForecastIds = computed(() => workspace.value?.forecastBundleIds ?? []);
const activeForecasts = computed(() => forecastBundles.value.filter((bundle) => activeForecastIds.value.includes(bundle.id)));
const descriptorById = computed(() => new Map(catalog.value.map((series) => [series.id, series])));
const seriesById = computed(() => new Map(dataWindow.value?.series.map((series) => [series.descriptor.id, series]) ?? []));
const lanes = computed(() => (workspace.value?.lanes ?? []).filter((lane) => lane.seriesIds.some((id) => seriesById.value.has(id))));
const filteredCatalog = computed(() => {
  const needle = search.value.trim().toLowerCase();
  return needle ? catalog.value.filter((series) => `${series.label} ${series.domain} ${series.provenance}`.toLowerCase().includes(needle)) : catalog.value;
});
const selectionStyle = computed(() => ({ left: `${selectionStart.value / 10}%`, width: `${(selectionEnd.value - selectionStart.value) / 10}%` }));
const cursorStyle = computed(() => ({ left: `${cursorRatio.value / 10}%` }));
const interval = computed(() => {
  if (!dataWindow.value) return undefined;
  const from = Date.parse(dataWindow.value.from);
  const span = Date.parse(dataWindow.value.to) - from;
  return {
    from: from + span * selectionStart.value / 1_000,
    to: from + span * selectionEnd.value / 1_000,
  };
});
const selectedMarker = computed(() => {
  if (!cursorTime.value || !dataWindow.value?.markers.length) return undefined;
  return availableMarkers.value.at(-1);
});
const availableMarkers = computed(() => {
  if (!cursorTime.value || !dataWindow.value) return [];
  return dataWindow.value.markers
    .filter((marker) => Date.parse(marker.timestamp) <= cursorTime.value! && Date.parse(marker.availableAt) <= cursorTime.value!)
    .toSorted((left, right) => Date.parse(left.timestamp) - Date.parse(right.timestamp));
});
const cutFacts = computed<Readonly<Record<string, string | number | boolean>>>(() => availableMarkers.value.reduce<Record<string, string | number | boolean>>((facts, marker) => ({ ...facts, ...marker.attributes }), {}));
const linkedOrderId = computed(() => {
  const marker = selectedMarker.value;
  if (!marker) return undefined;
  if (props.snapshot.orders.some((order) => order.clientOrderId === marker.entityId)) return marker.entityId;
  return props.snapshot.fills.find((fill) => fill.executionId === marker.entityId)?.clientOrderId;
});
const selectedPrice = computed(() => availableSeriesValueAt("market-ohlc")?.value);
const navigatorPoints = computed(() => {
  const series = seriesById.value.get("market-ohlc") ?? dataWindow.value?.series[0];
  if (!series?.points.length) return "";
  const values = series.points.map(pointValue);
  const min = Math.min(...values);
  const max = Math.max(...values);
  return values.map((value, index) => `${index / Math.max(1, values.length - 1) * 100},${20 - (value - min) / Math.max(0.0001, max - min) * 18}`).join(" ");
});
const navigatorViewportStyle = computed(() => {
  if (!dataWindow.value || !visibleRange.value) return { left: "0%", width: "100%" };
  const from = Date.parse(dataWindow.value.from);
  const span = Date.parse(dataWindow.value.to) - from;
  return {
    left: `${Math.max(0, Math.min(100, (visibleRange.value.from - from) / Math.max(1, span) * 100))}%`,
    width: `${Math.max(0, Math.min(100, (visibleRange.value.to - visibleRange.value.from) / Math.max(1, span) * 100))}%`,
  };
});
const plotSelectionStyle = computed(() => {
  if (!interval.value || !visibleRange.value) return selectionStyle.value;
  const span = visibleRange.value.to - visibleRange.value.from;
  const left = (interval.value.from - visibleRange.value.from) / Math.max(1, span) * 100;
  const right = (interval.value.to - visibleRange.value.from) / Math.max(1, span) * 100;
  if (right <= 0 || left >= 100) return { display: "none" };
  const clippedLeft = Math.max(0, left);
  return { left: `${clippedLeft}%`, width: `${Math.min(100, right) - clippedLeft}%` };
});
const registerRows = computed(() => ({ gridTemplateRows: lanes.value.map((lane) => `${laneWeight(lane)}fr`).join(" ") }));
const openAlerts = computed(() => props.snapshot.alerts.filter((alert) => alert.status !== "resolved"));
const reconciliationRows = computed(() => props.snapshot.orders.map((order) => ({
  id: order.clientOrderId,
  label: `${order.instrument} / ${shortId(order.clientOrderId)}`,
  status: order.reconciliation,
})));
const markerRails: readonly { readonly id: string; readonly label: string; readonly kinds: readonly MarkerKind[] }[] = [
  { id: "orders", label: "Orders", kinds: ["order", "ack", "replace", "cancel"] },
  { id: "fills", label: "Fills", kinds: ["fill"] },
  { id: "system", label: "System events", kinds: ["alert", "model", "risk"] },
];

function parseWorkspace(): TimelineWorkspace | undefined {
  try {
    const parsed = JSON.parse(window.localStorage.getItem(workspaceStorageKey()) ?? "null") as Partial<TimelineWorkspace> | null;
    if (!parsed || parsed.schemaVersion !== 2 || !Array.isArray(parsed.lanes)) return undefined;
    const validIds = new Set(catalog.value.map((series) => series.id));
    const parsedLanes = parsed.lanes.flatMap((lane) => {
      if (!lane || typeof lane.id !== "string" || !Array.isArray(lane.seriesIds)) return [];
      const seriesIds = lane.seriesIds.filter((id: unknown): id is string => typeof id === "string" && validIds.has(id));
      return seriesIds.length ? [{ id: lane.id, seriesIds }] : [];
    });
    const transform = ["raw", "indexed", "percent_change", "z_score"].includes(parsed.transform ?? "") ? parsed.transform as SeriesTransform : "raw";
    const windowMinutes = [1, 5, 10, 15, 60, 240, 1_440].includes(parsed.windowMinutes ?? 0) ? Number(parsed.windowMinutes) : 60;
    const forecastBundleIds = Array.isArray(parsed.forecastBundleIds)
      ? parsed.forecastBundleIds.filter((id: unknown): id is string => typeof id === "string" && forecastBundles.value.some((bundle) => bundle.id === id))
      : [];
    return { schemaVersion: 2, transform, lanes: parsedLanes, windowMinutes, forecastBundleIds };
  } catch {
    return undefined;
  }
}

function persistWorkspace(): void {
  if (workspace.value) window.localStorage.setItem(workspaceStorageKey(), JSON.stringify(workspace.value));
}

function workspaceStorageKey(): string {
  const context = props.snapshot.context;
  return `helios:operator:market-atlas:v2:${context.organizationId}:${context.workspaceId}:${context.accountId}`;
}

async function loadWindow(): Promise<void> {
  if (!workspace.value) return;
  const generation = contextLoadGeneration;
  const context = props.snapshot.context;
  loading.value = true;
  error.value = "";
  try {
    const windowChanged = loadedWindowMinutes !== workspace.value.windowMinutes;
    const toMs = Number.isFinite(Date.parse(props.snapshot.observedAt)) ? Date.parse(props.snapshot.observedAt) : Date.now();
    const fromMs = toMs - workspace.value.windowMinutes * 60_000;
    const nextWindow = await props.port.query({
      context,
      seriesIds: activeSeriesIds.value,
      from: new Date(fromMs).toISOString(),
      to: new Date(toMs).toISOString(),
      maxPoints: 480,
    });
    if (generation !== contextLoadGeneration || props.snapshot.context.accountId !== context.accountId) return;
    dataWindow.value = nextWindow;
    if (windowChanged || !visibleRange.value) visibleRange.value = { from: fromMs, to: toMs };
    loadedWindowMinutes = workspace.value.windowMinutes;
    moveCursor(cursorRatio.value);
  } catch (cause) {
    if (generation !== contextLoadGeneration) return;
    error.value = cause instanceof Error ? cause.message : "Market Atlas could not be loaded";
  } finally {
    if (generation !== contextLoadGeneration) return;
    loading.value = false;
    await nextTick();
    renderChart();
  }
}

function updateWorkspace(next: TimelineWorkspace, reload = true): void {
  workspace.value = next;
  persistWorkspace();
  if (reload) void loadWindow();
  else void nextTick(renderChart);
}

function setTransform(transform: SeriesTransform): void {
  if (workspace.value) updateWorkspace({ ...workspace.value, transform }, false);
}

function setWindowMinutes(windowMinutes: number): void {
  if (!workspace.value || workspace.value.windowMinutes === windowMinutes) return;
  selectionStart.value = 0;
  selectionEnd.value = 1_000;
  cursorRatio.value = 1_000;
  visibleRange.value = undefined;
  updateWorkspace({ ...workspace.value, windowMinutes });
}

function toggleSeries(id: string): void {
  if (!workspace.value) return;
  const active = activeSeriesIds.value.includes(id);
  const without = workspace.value.lanes.flatMap((lane) => {
    const seriesIds = lane.seriesIds.filter((current) => current !== id);
    return seriesIds.length ? [{ ...lane, seriesIds }] : [];
  });
  updateWorkspace({ ...workspace.value, lanes: active ? without : [...without, { id: `lane-${id}`, seriesIds: [id] }] });
}

function placeSeries(id: string, target: string): void {
  if (!workspace.value || !activeSeriesIds.value.includes(id)) return;
  const without = workspace.value.lanes.flatMap((lane) => {
    const seriesIds = lane.seriesIds.filter((current) => current !== id);
    return seriesIds.length ? [{ ...lane, seriesIds }] : [];
  });
  if (target === "own") {
    updateWorkspace({ ...workspace.value, lanes: [...without, { id: `lane-${id}`, seriesIds: [id] }] });
    return;
  }
  updateWorkspace({ ...workspace.value, lanes: without.map((lane) => lane.id === target ? { ...lane, seriesIds: [...lane.seriesIds, id] } : lane) });
}

function bundleReferencesSeries(bundleId: string, seriesId: string): boolean {
  return forecastBundles.value.find((bundle) => bundle.id === bundleId)?.seriesIds.includes(seriesId) ?? false;
}

function bundleIdsForSeries(seriesId: string): readonly string[] {
  return forecastBundles.value.filter((bundle) => bundle.seriesIds.includes(seriesId)).map((bundle) => bundle.id);
}

function toggleForecastBundle(bundle: ForecastBundle): void {
  if (!workspace.value) return;
  const active = activeForecastIds.value.includes(bundle.id);
  const forecastBundleIds = active
    ? activeForecastIds.value.filter((id) => id !== bundle.id)
    : [...activeForecastIds.value, bundle.id];
  let nextLanes = [...workspace.value.lanes];
  if (active) {
    nextLanes = nextLanes.flatMap((lane) => {
      const seriesIds = lane.seriesIds.filter((seriesId) =>
        !bundle.seriesIds.includes(seriesId) || forecastBundleIds.some((id) => bundleReferencesSeries(id, seriesId)),
      );
      return seriesIds.length ? [{ ...lane, seriesIds }] : [];
    });
  } else {
    for (const seriesId of bundle.seriesIds) {
      if (!nextLanes.some((lane) => lane.seriesIds.includes(seriesId))) nextLanes.push({ id: `lane-${seriesId}`, seriesIds: [seriesId] });
    }
  }
  updateWorkspace({ ...workspace.value, lanes: nextLanes, forecastBundleIds });
}

function focusForecastBundle(bundleId: string): void {
  const bundle = forecastBundles.value.find((candidate) => candidate.id === bundleId);
  if (!workspace.value || !bundle) return;
  updateWorkspace({
    ...workspace.value,
    forecastBundleIds: [bundle.id],
    lanes: bundle.seriesIds.map((seriesId) => ({ id: `lane-${seriesId}`, seriesIds: [seriesId] })),
  });
}

function reorderLane(sourceId: string | undefined, targetId: string): void {
  if (!workspace.value || !sourceId || sourceId === targetId) return;
  const lanes = [...workspace.value.lanes];
  const sourceIndex = lanes.findIndex((lane) => lane.id === sourceId);
  const targetIndex = lanes.findIndex((lane) => lane.id === targetId);
  if (sourceIndex < 0 || targetIndex < 0) return;
  const [source] = lanes.splice(sourceIndex, 1);
  lanes.splice(targetIndex, 0, source);
  draggedLaneId.value = undefined;
  updateWorkspace({ ...workspace.value, lanes }, false);
}

function moveLane(id: string, delta: number): void {
  if (!workspace.value) return;
  const lanes = [...workspace.value.lanes];
  const index = lanes.findIndex((lane) => lane.id === id);
  const target = Math.max(0, Math.min(lanes.length - 1, index + delta));
  if (index < 0 || target === index) return;
  const [lane] = lanes.splice(index, 1);
  lanes.splice(target, 0, lane);
  updateWorkspace({ ...workspace.value, lanes }, false);
}

function handlePaneWeights(weights: readonly number[]): void {
  if (!workspace.value || weights.length !== lanes.value.length) return;
  const weightById = new Map(lanes.value.map((lane, index) => [lane.id, weights[index]]));
  workspace.value = { ...workspace.value, lanes: workspace.value.lanes.map((lane) => ({ ...lane, weight: weightById.get(lane.id) ?? lane.weight })) };
  persistWorkspace();
}

function resetWorkspace(): void {
  updateWorkspace(defaultTimelineWorkspace(catalog.value));
}

function laneFor(id: string): TimelineLane | undefined {
  return workspace.value?.lanes.find((lane) => lane.seriesIds.includes(id));
}

function laneLabel(lane: TimelineLane): string {
  return lane.seriesIds.map((id) => descriptorById.value.get(id)?.shortLabel ?? id).join(" + ");
}

function laneWeight(lane: TimelineLane): number {
  return lane.weight ?? Math.max(...lane.seriesIds.map((id) => descriptorById.value.get(id)?.paneWeight ?? 1));
}

function effectiveTransform(lane: TimelineLane): SeriesTransform {
  if (!workspace.value) return "raw";
  const units = new Set(lane.seriesIds.map((id) => descriptorById.value.get(id)?.unit));
  return workspace.value.transform === "raw" && units.size > 1 ? "indexed" : workspace.value.transform;
}

function pointValue(point: TimeSeriesPoint): number {
  return point.kind === "ohlc" ? point.close : point.value;
}

function transformNumber(value: number, first: number, mean: number, deviation: number, transform: SeriesTransform): number {
  if (transform === "raw") return value;
  if (transform === "indexed") return value / (first || 1) * 100;
  if (transform === "percent_change") return (value / (first || 1) - 1) * 100;
  return (value - mean) / (deviation || 1);
}

function chartPoints(series: TimeSeriesData, lane: TimelineLane): readonly AtlasPoint[] {
  const values = series.points.map(pointValue);
  const first = values[0] || 1;
  const mean = values.reduce((total, value) => total + value, 0) / Math.max(1, values.length);
  const variance = values.reduce((total, value) => total + (value - mean) ** 2, 0) / Math.max(1, values.length - 1);
  const deviation = Math.sqrt(variance) || 1;
  const transform = effectiveTransform(lane);
  return series.points.map((point) => {
    const time = Date.parse(point.timestamp) / 1_000;
    if (point.kind === "ohlc") return {
      time,
      open: transformNumber(point.open, first, mean, deviation, transform),
      high: transformNumber(point.high, first, mean, deviation, transform),
      low: transformNumber(point.low, first, mean, deviation, transform),
      close: transformNumber(point.close, first, mean, deviation, transform),
    };
    return { time, value: transformNumber(point.value, first, mean, deviation, transform), color: point.color };
  });
}

function buildModel(): AtlasChartModel {
  return {
    panes: lanes.value.map((lane) => ({ id: lane.id, weight: laneWeight(lane) })),
    series: lanes.value.flatMap((lane) => lane.seriesIds.flatMap((id) => {
      const series = seriesById.value.get(id);
      if (!series) return [];
      return [{
        id,
        paneId: lane.id,
        label: series.descriptor.label,
        kind: series.descriptor.render as AtlasSeriesKind,
        color: series.descriptor.color,
        unit: effectiveTransform(lane) === "raw" ? series.descriptor.unit : effectiveTransform(lane),
        precision: effectiveTransform(lane) === "raw" ? series.descriptor.precision : 2,
        data: chartPoints(series, lane),
      }];
    })),
  };
}

function renderChart(): void {
  if (!lanes.value.length) {
    chart?.destroy();
    chart = undefined;
    chartHost.value?.replaceChildren();
    return;
  }
  if (!chartHost.value || !dataWindow.value) return;
  const model = buildModel();
  const desiredCursor = cursorTime.value ?? Date.parse(dataWindow.value.to);
  if (chart) chart.setModel(model);
  else chart = createAtlasChart(chartHost.value, { model, onCursor: handleChartCursor, onVisibleRange: handleVisibleRange, onPaneWeights: handlePaneWeights });
  cursorTime.value = desiredCursor;
  chart.setCursor(desiredCursor / 1_000);
}

function handleVisibleRange(range: { from: number; to: number } | undefined): void {
  if (!range || !dataWindow.value) return;
  visibleRange.value = { from: range.from * 1_000, to: range.to * 1_000 };
}

function handleChartCursor(snapshot: AtlasCursorSnapshot | undefined): void {
  if (!snapshot || !dataWindow.value) return;
  cursorTime.value = snapshot.time * 1_000;
  const from = Date.parse(dataWindow.value.from);
  const span = Date.parse(dataWindow.value.to) - from;
  cursorRatio.value = Math.round(Math.min(1, Math.max(0, (cursorTime.value - from) / Math.max(1, span))) * 1_000);
}

function moveCursor(value: number): void {
  cursorRatio.value = value;
  if (!dataWindow.value) return;
  const from = Date.parse(dataWindow.value.from);
  cursorTime.value = from + (Date.parse(dataWindow.value.to) - from) * value / 1_000;
  chart?.setCursor(cursorTime.value / 1_000);
}

function changeSelection(edge: "start" | "end", value: number): void {
  const next = clampScrubRange(edge, value, selectionStart.value, selectionEnd.value);
  selectionStart.value = next.start;
  selectionEnd.value = next.end;
  moveCursor(Math.round((selectionStart.value + selectionEnd.value) / 2));
}

function zoomSelection(): void {
  if (interval.value) chart?.setVisibleRange({ from: interval.value.from / 1_000, to: interval.value.to / 1_000 });
}

function resetViewport(): void {
  if (dataWindow.value) chart?.setVisibleRange({ from: Date.parse(dataWindow.value.from) / 1_000, to: Date.parse(dataWindow.value.to) / 1_000 });
}

function markerStyle(marker: TimelineMarker): Record<string, string> {
  if (!dataWindow.value) return { left: "0%" };
  const range = visibleRange.value ?? { from: Date.parse(dataWindow.value.from), to: Date.parse(dataWindow.value.to) };
  const ratio = (Date.parse(marker.timestamp) - range.from) / Math.max(1, range.to - range.from);
  return ratio < 0 || ratio > 1 ? { display: "none" } : { left: `${ratio * 100}%` };
}

function seriesValueAt(id: string): { value: number; available: boolean; point: TimeSeriesPoint } | undefined {
  const series = seriesById.value.get(id);
  if (!series?.points.length) return undefined;
  const at = cursorTime.value ?? Date.parse(series.points.at(-1)!.timestamp);
  const point = series.points.reduce((best, candidate) => Math.abs(Date.parse(candidate.timestamp) - at) < Math.abs(Date.parse(best.timestamp) - at) ? candidate : best, series.points[0]);
  return { value: pointValue(point), available: Date.parse(point.availableAt) <= at, point };
}

function availableSeriesValueAt(id: string): { value: number; point: TimeSeriesPoint } | undefined {
  const series = seriesById.value.get(id);
  if (!series?.points.length) return undefined;
  const at = cursorTime.value ?? Date.parse(series.points.at(-1)!.timestamp);
  for (let index = series.points.length - 1; index >= 0; index -= 1) {
    const point = series.points[index];
    if (Date.parse(point.timestamp) <= at && Date.parse(point.availableAt) <= at) return { value: pointValue(point), point };
  }
  return undefined;
}

function seriesDisplay(id: string): string {
  const descriptor = descriptorById.value.get(id);
  const sample = seriesValueAt(id);
  return descriptor && sample ? formatValue(sample.value, descriptor) : "n/a";
}

function availableSeriesDisplay(id: string): string {
  const descriptor = descriptorById.value.get(id);
  const sample = availableSeriesValueAt(id);
  return descriptor && sample ? formatValue(sample.value, descriptor) : "not yet available";
}

function formatValue(value: number, descriptor: TimeSeriesDescriptor): string {
  if (descriptor.unit === "USD") return new Intl.NumberFormat("en-US", { style: "currency", currency: "USD", notation: Math.abs(value) >= 100_000 ? "compact" : "standard", maximumFractionDigits: descriptor.precision }).format(value);
  if (descriptor.unit === "volume") return new Intl.NumberFormat("en-US", { notation: "compact", maximumFractionDigits: 1 }).format(value);
  if (descriptor.unit === "%") return `${(descriptor.id.includes("signal") || descriptor.id.includes("quality") || descriptor.id.includes("risk") ? value * 100 : value).toFixed(descriptor.precision)}%`;
  return `${new Intl.NumberFormat("en-US", { maximumFractionDigits: descriptor.precision }).format(value)}${descriptor.unit}`;
}

function formatTime(timestamp: number | string | undefined, seconds = true): string {
  if (timestamp === undefined) return "n/a";
  let date = new Date(timestamp);
  if (!Number.isFinite(date.getTime()) && typeof timestamp === "string" && /^\d{2}:\d{2}/.test(timestamp)) {
    date = new Date(`${props.snapshot.observedAt.slice(0, 10)}T${timestamp}`);
  }
  if (!Number.isFinite(date.getTime())) return timestamp.toString();
  return new Intl.DateTimeFormat("en-US", { hour: "2-digit", minute: "2-digit", second: seconds ? "2-digit" : undefined, fractionalSecondDigits: seconds ? 3 : undefined, hour12: false }).format(date);
}

function formatDuration(milliseconds: number): string {
  const totalSeconds = Math.max(0, Math.round(milliseconds / 1_000));
  return `${Math.floor(totalSeconds / 60)}m ${String(totalSeconds % 60).padStart(2, "0")}s`;
}

function money(micros: string): string {
  return new Intl.NumberFormat("en-US", { style: "currency", currency: "USD", maximumFractionDigits: 2 }).format(Number(micros) / 1_000_000);
}

function quantity(micros: string): string {
  return new Intl.NumberFormat("en-US", { maximumFractionDigits: 3 }).format(Number(micros) / 1_000_000);
}

function shortId(id: string): string {
  return id.length > 11 ? `${id.slice(0, 5)}…${id.slice(-3)}` : id;
}

function isMeaningfullyLate(id: string): boolean {
  const sample = seriesValueAt(id);
  return Boolean(sample && !sample.available && Date.parse(sample.point.availableAt) - Date.parse(sample.point.timestamp) >= 250);
}

function cutFact(key: string): string {
  const value = cutFacts.value[key];
  return value === undefined ? "Not available at cut" : String(value);
}

function cutFactTone(key: string): string | undefined {
  const value = cutFacts.value[key];
  if (typeof value !== "string") return undefined;
  if (["approve", "accepted", "matched"].includes(value.toLowerCase())) return "authorized";
  if (["hold", "pending", "rejected"].includes(value.toLowerCase())) return "closed";
  return undefined;
}

async function runInvestigation(): Promise<void> {
  if (!dataWindow.value || !cursorTime.value) return;
  if (!investigationDialog.value?.open) investigationDialog.value?.showModal();
  investigating.value = true;
  investigationError.value = "";
  try {
    investigation.value = await props.investigationPort.investigate({
      schemaVersion: 1,
      context: props.snapshot.context,
      snapshotSequence: props.snapshot.sequence,
      from: new Date(interval.value?.from ?? Date.parse(dataWindow.value.from)).toISOString(),
      to: new Date(interval.value?.to ?? Date.parse(dataWindow.value.to)).toISOString(),
      cursor: new Date(cursorTime.value).toISOString(),
      markerId: selectedMarker.value?.id,
      seriesIds: activeSeriesIds.value,
    });
  } catch (cause) {
    investigationError.value = cause instanceof Error ? cause.message : "Investigation failed";
  } finally {
    investigating.value = false;
  }
}

async function loadResearchContext(): Promise<void> {
  const generation = ++contextLoadGeneration;
  const context = props.snapshot.context;
  loading.value = true;
  error.value = "";
  try {
    const [nextCatalog, nextForecastBundles] = await Promise.all([
      props.port.catalog(context),
      props.port.forecastBundles(context),
    ]);
    if (generation !== contextLoadGeneration || props.snapshot.context.accountId !== context.accountId) return;
    catalog.value = nextCatalog;
    forecastBundles.value = nextForecastBundles;
    const nextWorkspace = parseWorkspace() ?? defaultTimelineWorkspace(nextCatalog);
    const selectedBundle = nextForecastBundles.find((bundle) => bundle.id === props.selectedForecastId);
    workspace.value = selectedBundle
      ? { ...nextWorkspace, forecastBundleIds: [selectedBundle.id], lanes: selectedBundle.seriesIds.map((seriesId) => ({ id: `lane-${seriesId}`, seriesIds: [seriesId] })) }
      : nextWorkspace;
    loadedWindowMinutes = undefined;
    visibleRange.value = undefined;
    await loadWindow();
  } catch (cause) {
    if (generation !== contextLoadGeneration) return;
    error.value = cause instanceof Error ? cause.message : "Market Atlas could not be loaded";
    loading.value = false;
  }
}

onMounted(() => { void loadResearchContext(); });

onBeforeUnmount(() => chart?.destroy());

watch(() => props.snapshot.context.accountId, () => { void loadResearchContext(); });

watch(() => props.selectedForecastId, (id) => {
  if (id) focusForecastBundle(id);
});
</script>

<template>
  <section class="market-atlas" aria-labelledby="market-atlas-heading">
    <header class="atlas-titlebar">
      <div><h1 id="market-atlas-heading">Market Atlas</h1><span>{{ activeForecasts.length ? activeForecasts.map((bundle) => bundle.label).join(' + ') : 'Custom workspace' }}</span></div>
      <dl><div><dt>Selected interval</dt><dd>{{ formatTime(interval?.from) }} to {{ formatTime(interval?.to) }} <small>({{ formatDuration((interval?.to ?? 0) - (interval?.from ?? 0)) }})</small></dd></div></dl>
    </header>

    <div class="atlas-toolbar">
      <label class="series-search"><svg viewBox="0 0 16 16" aria-hidden="true"><circle cx="7" cy="7" r="4"/><path d="m10 10 4 4"/></svg><input v-model="search" type="search" placeholder="Find observations" aria-label="Search registered observations" @focus="seriesOpen = true"></label>
      <span class="tool-label">Transform</span>
      <div class="segmented" role="group" aria-label="Series transform">
        <button :aria-pressed="workspace?.transform === 'raw'" @click="setTransform('raw')">Raw</button>
        <button :aria-pressed="workspace?.transform === 'indexed'" @click="setTransform('indexed')">Index</button>
        <button :aria-pressed="workspace?.transform === 'percent_change'" @click="setTransform('percent_change')">%</button>
        <button :aria-pressed="workspace?.transform === 'z_score'" @click="setTransform('z_score')">Z</button>
      </div>
      <span class="tool-label history-label">History</span>
      <div class="window-buttons" role="group" aria-label="Loaded history window">
        <button v-for="option in [{m:5,l:'5m'},{m:10,l:'10m'},{m:60,l:'1h'},{m:240,l:'4h'},{m:1440,l:'1d'}]" :key="option.m" :aria-pressed="workspace?.windowMinutes === option.m" @click="setWindowMinutes(option.m)">{{ option.l }}</button>
      </div>
      <button class="manage-series" :aria-expanded="seriesOpen" @click="seriesOpen = !seriesOpen"><svg viewBox="0 0 16 16" aria-hidden="true"><path d="M2 4h12M2 8h12M2 12h12M5 2v4M11 6v4M7 10v4"/></svg>Observations <b>{{ activeSeriesIds.length }}</b></button>
    </div>

    <div class="forecast-strip" aria-label="Forecast bundles">
      <span>Forecast scope</span>
      <button v-for="bundle in forecastBundles" :key="bundle.id" :aria-pressed="activeForecastIds.includes(bundle.id)" @click="toggleForecastBundle(bundle)"><i :data-state="bundle.state"></i>{{ bundle.label }}<b>{{ bundle.seriesIds.length }}</b></button>
      <button class="forecast-custom" :aria-pressed="activeForecastIds.length === 0" @click="workspace && updateWorkspace({...workspace,forecastBundleIds:[]},false)">Custom</button>
    </div>

    <div v-if="seriesOpen" class="series-drawer">
      <header><div><strong>Workspace composition</strong><span>Add complete forecasts or individual observations. Drag the register to reorder panes.</span></div><button @click="resetWorkspace">Restore default</button></header>
      <div class="bundle-registry">
        <details v-for="bundle in forecastBundles" :key="bundle.id" :open="activeForecastIds.includes(bundle.id)">
          <summary><i :data-state="bundle.state"></i><span><strong>{{ bundle.label }}</strong><small>{{ bundle.thesis }}</small></span><b>{{ bundle.horizon }}</b></summary>
          <div><p><span v-for="seriesId in bundle.seriesIds" :key="seriesId" :data-shared="bundle.sharedSeriesIds.includes(seriesId)">{{ descriptorById.get(seriesId)?.shortLabel }}<b v-if="bundle.sharedSeriesIds.includes(seriesId)">shared</b></span></p><button @click="toggleForecastBundle(bundle)">{{ activeForecastIds.includes(bundle.id) ? 'Remove bundle' : 'Add bundle' }}</button></div>
        </details>
      </div>
      <div class="series-options">
        <article v-for="series in filteredCatalog" :key="series.id" :data-active="activeSeriesIds.includes(series.id)">
          <button class="series-toggle" :aria-pressed="activeSeriesIds.includes(series.id)" @click="toggleSeries(series.id)"><i :style="{ background: series.color }"></i><span><strong>{{ series.label }}</strong><small>{{ series.domain }} · {{ series.provenance }}</small></span><b>{{ activeSeriesIds.includes(series.id) ? 'Remove' : 'Add' }}</b></button>
          <div v-if="activeSeriesIds.includes(series.id)" class="pane-choices" role="group" :aria-label="`Place ${series.label}`"><span>Display</span><button :aria-pressed="laneFor(series.id)?.seriesIds.length === 1" @click="placeSeries(series.id,'own')">Own lane</button><button v-for="lane in workspace?.lanes.filter((candidate) => !candidate.seriesIds.includes(series.id))" :key="lane.id" :aria-pressed="laneFor(series.id)?.id === lane.id" @click="placeSeries(series.id,lane.id)">With {{ laneLabel(lane) }}</button></div>
        </article>
      </div>
    </div>

    <div v-if="loading" class="atlas-state"><i></i>Building synchronized panes</div>
    <div v-else-if="error" class="atlas-state error"><i></i><strong>{{ error }}</strong><button @click="loadWindow">Retry</button></div>
    <div v-else-if="dataWindow" class="atlas-scroll" tabindex="0" aria-label="Market Atlas. Scroll horizontally on narrow screens.">
      <div class="atlas-frame">
        <div class="atlas-main">
          <aside class="series-register" aria-label="Visible time series">
            <div class="series-register-body" :style="registerRows">
              <article v-for="lane in lanes" :key="lane.id" draggable="true" :data-dragging="draggedLaneId === lane.id" @dragstart="draggedLaneId = lane.id" @dragend="draggedLaneId = undefined" @dragover.prevent @drop="reorderLane(draggedLaneId,lane.id)">
                <button class="lane-grip" :aria-label="`Drag ${laneLabel(lane)} pane`"><svg viewBox="0 0 10 16" aria-hidden="true"><circle cx="3" cy="3" r="1"/><circle cx="7" cy="3" r="1"/><circle cx="3" cy="8" r="1"/><circle cx="7" cy="8" r="1"/><circle cx="3" cy="13" r="1"/><circle cx="7" cy="13" r="1"/></svg></button>
                <div class="lane-series"><span v-for="id in lane.seriesIds" :key="id"><i :style="{ background: descriptorById.get(id)?.color }"></i><span><strong>{{ descriptorById.get(id)?.label }}</strong><small>{{ descriptorById.get(id)?.provenance }}<b v-if="bundleIdsForSeries(id).length > 1"> · shared</b></small></span><b>{{ seriesDisplay(id) }}</b></span></div>
                <div class="lane-actions"><button :disabled="lanes[0]?.id === lane.id" :aria-label="`Move ${laneLabel(lane)} up`" @click="moveLane(lane.id,-1)"><svg viewBox="0 0 12 12" aria-hidden="true"><path d="m2.5 7.5 3.5-3 3.5 3"/></svg></button><button :disabled="lanes.at(-1)?.id === lane.id" :aria-label="`Move ${laneLabel(lane)} down`" @click="moveLane(lane.id,1)"><svg viewBox="0 0 12 12" aria-hidden="true"><path d="m2.5 4.5 3.5 3 3.5-3"/></svg></button></div>
              </article>
            </div>
          </aside>

          <div class="chart-column">
            <div v-if="lanes.length" class="chart-stage">
              <div ref="chartHost" class="chart-host" aria-label="Synchronized financial time series"></div>
              <div class="selection-window" :style="plotSelectionStyle"><i></i><i></i></div>
            </div>
            <div v-else class="empty-atlas">
              <span>No series selected</span>
              <strong>Choose the observations that belong in this workspace.</strong>
              <button @click="seriesOpen = true">Choose series</button>
            </div>
            <div class="event-rails" aria-label="Lifecycle events">
              <div v-for="rail in markerRails" :key="rail.id"><strong>{{ rail.label }}</strong><span><button v-for="marker in dataWindow.markers.filter((item) => rail.kinds.includes(item.kind))" :key="marker.id" :data-kind="marker.kind" :style="markerStyle(marker)" :title="marker.detail" @click="moveCursor(Math.round((Date.parse(marker.timestamp)-Date.parse(dataWindow.from))/(Date.parse(dataWindow.to)-Date.parse(dataWindow.from))*1000))"><i></i></button></span></div>
            </div>
            <div class="global-scrubber">
              <div class="navigator"><svg viewBox="0 0 100 20" preserveAspectRatio="none" aria-hidden="true"><polyline :points="navigatorPoints"/></svg><div class="navigator-viewport" :style="navigatorViewportStyle"></div><div class="navigator-selection" :style="selectionStyle"></div><div class="navigator-cursor" :style="cursorStyle"></div><input :value="selectionStart" type="range" min="0" max="1000" aria-label="Selection start" @input="changeSelection('start', Number(($event.target as HTMLInputElement).value))"><input :value="selectionEnd" type="range" min="0" max="1000" aria-label="Selection end" @input="changeSelection('end', Number(($event.target as HTMLInputElement).value))"><input class="cursor-range" :value="cursorRatio" type="range" min="0" max="1000" aria-label="Evidence cursor" @input="moveCursor(Number(($event.target as HTMLInputElement).value))"></div>
              <div class="scrubber-actions"><button aria-label="Move cursor backward" @click="moveCursor(Math.max(selectionStart, cursorRatio - 5))">‹</button><button aria-label="Move cursor forward" @click="moveCursor(Math.min(selectionEnd, cursorRatio + 5))">›</button><time :datetime="cursorTime ? new Date(cursorTime).toISOString() : undefined">{{ formatTime(cursorTime) }}</time><button @click="zoomSelection">Focus selection</button><button @click="resetViewport">Reset view</button></div>
            </div>
          </div>

          <aside class="evidence-rail">
            <header><div><span>Evidence rail</span><strong>{{ selectedMarker?.label ?? 'Observation' }}</strong></div><b>{{ formatTime(cursorTime) }}</b></header>
            <section class="rail-section observation"><h2>Available at decision</h2><dl><div v-for="id in activeSeriesIds.slice(0,8)" :key="id" :data-late="!availableSeriesValueAt(id)"><dt>{{ descriptorById.get(id)?.shortLabel }}</dt><dd>{{ availableSeriesDisplay(id) }}</dd></div></dl></section>
            <section v-if="activeSeriesIds.some(isMeaningfullyLate)" class="rail-section late"><h2>Late arrivals</h2><p v-for="id in activeSeriesIds.filter(isMeaningfullyLate).slice(0,3)" :key="id"><time>{{ formatTime(seriesValueAt(id)?.point.availableAt) }}</time><span>{{ descriptorById.get(id)?.label }}</span></p></section>
            <section class="rail-section decision"><dl><div><dt>Model version</dt><dd>{{ cutFact('modelVersion') }}</dd></div><div><dt>Risk decision</dt><dd :data-tone="cutFactTone('riskDecision')">{{ cutFact('riskDecision') }}</dd></div><div><dt>Order intent</dt><dd>{{ cutFact('orderIntent') }}</dd></div><div><dt>Broker ack</dt><dd :data-tone="cutFactTone('brokerAcknowledgement')">{{ cutFact('brokerAcknowledgement') }}</dd></div><div><dt>Fill result</dt><dd>{{ cutFact('fillResult') }}</dd></div><div><dt>Selected price</dt><dd>{{ selectedPrice === undefined ? 'Not available at cut' : new Intl.NumberFormat('en-US',{style:'currency',currency:'USD'}).format(selectedPrice) }}</dd></div></dl></section>
            <button class="investigate" :disabled="investigating" @click="runInvestigation">{{ investigating ? 'Analyzing selection' : 'Analyze selection' }}</button>
            <section class="rail-section suggestions"><h2>Additional series</h2><button v-for="series in catalog.filter((item) => !activeSeriesIds.includes(item.id)).slice(0,3)" :key="series.id" @click="toggleSeries(series.id)"><i :style="{ background: series.color }"></i><span>{{ series.label }}<small>{{ series.provenance }}</small></span><b>+ Add</b></button></section>
            <button v-if="linkedOrderId" class="linked-record" @click="emit('selectOrder', linkedOrderId)">Open OMS record <span>→</span></button>
            <p v-if="investigationError" class="rail-error">{{ investigationError }}</p>
          </aside>
        </div>

        <div class="ledger-dock">
          <section><header><h2>Orders</h2><span class="live">Live</span></header><div class="ledger-scroll"><table><thead><tr><th>Time</th><th>Symbol</th><th>Side</th><th>Qty</th><th>Type</th><th>Status</th></tr></thead><tbody><tr v-for="order in snapshot.orders.slice(0,4)" :key="order.clientOrderId" :class="{selected:selectedOrderId===order.clientOrderId}"><td><button class="ledger-order-button" @click="emit('selectOrder',order.clientOrderId)">{{ formatTime(order.submittedAt,false) }}</button></td><td>{{ order.instrument }}</td><td :data-tone="order.side">{{ order.side }}</td><td>{{ quantity(order.quantityMicros) }}</td><td>LMT</td><td>{{ order.state.replaceAll('_',' ') }}</td></tr><tr v-if="!snapshot.orders.length"><td class="ledger-empty" colspan="6">No active orders</td></tr></tbody></table></div></section>
          <section><header><h2>Positions</h2><span>{{ snapshot.positions.length }}</span></header><div class="ledger-scroll"><table><thead><tr><th>Symbol</th><th>Net</th><th>Average</th><th>Unrealized</th></tr></thead><tbody><tr v-for="position in snapshot.positions.slice(0,4)" :key="`${position.instrument}:${position.strategy}`"><td>{{ position.instrument }}</td><td>{{ quantity(position.quantityMicros) }}</td><td>{{ money(position.averagePriceMicros) }}</td><td :data-tone="Number(position.unrealizedPnlMicros)>=0?'buy':'sell'">{{ money(position.unrealizedPnlMicros) }}</td></tr><tr v-if="!snapshot.positions.length"><td class="ledger-empty" colspan="4">No open positions</td></tr></tbody></table></div></section>
          <section><header><h2>Alerts</h2><span class="alert-count">{{ openAlerts.length }}</span></header><div class="ledger-scroll"><table><thead><tr><th>Time</th><th>Severity</th><th>Message</th></tr></thead><tbody><tr v-for="alert in openAlerts.slice(0,4)" :key="alert.id"><td>{{ formatTime(alert.openedAt,false) }}</td><td :data-tone="alert.severity">{{ alert.severity }}</td><td>{{ alert.title }}</td></tr><tr v-if="!openAlerts.length"><td class="ledger-empty" colspan="3">No open alerts</td></tr></tbody></table></div></section>
          <section><header><h2>Reconciliation</h2><span :class="snapshot.risk.pendingReconciliations ? 'alert-count' : 'live'">{{ snapshot.risk.pendingReconciliations }}</span></header><div class="reconcile-list"><p v-for="row in reconciliationRows.slice(0,4)" :key="row.id"><span>{{ row.label }}</span><b :data-tone="row.status === 'matched' ? 'buy' : 'warning'">{{ row.status }}</b></p><p v-if="!reconciliationRows.length" class="ledger-empty">No reconciliation records</p></div></section>
        </div>
      </div>
    </div>

    <Teleport to="body">
      <dialog ref="investigationDialog" class="investigation-dialog">
        <header><div><strong>Interval analysis</strong><span>{{ formatTime(interval?.from) }} to {{ formatTime(interval?.to) }}</span></div><button aria-label="Close interval analysis" @click="investigationDialog?.close()"><svg viewBox="0 0 16 16" aria-hidden="true"><path d="m3 3 10 10M13 3 3 13"/></svg></button></header>
        <div v-if="investigating" class="investigation-loading"><i></i><span>Evaluating {{ activeSeriesIds.length }} observations against the decision cut</span></div>
        <template v-else-if="investigation">
          <section><h2>Finding</h2><p>{{ investigation.summary }}</p><small>{{ investigation.limitation }}</small></section>
          <section><h2>Evidence used</h2><div class="investigation-inputs"><span v-for="id in activeSeriesIds" :key="id"><i :style="{background:descriptorById.get(id)?.color}"></i>{{ descriptorById.get(id)?.label }}</span></div></section>
          <section v-if="investigation.citations.length"><h2>Citations</h2><ol><li v-for="citation in investigation.citations" :key="citation.id"><time>{{ formatTime(citation.timestamp) }}</time><strong>{{ citation.label }}</strong><small>{{ citation.sourceId }}</small></li></ol></section>
          <section v-if="investigation.suggestedSeriesIds.some((id) => !activeSeriesIds.includes(id))"><h2>Suggested observations</h2><button v-for="id in investigation.suggestedSeriesIds.filter((candidate) => !activeSeriesIds.includes(candidate))" :key="id" @click="toggleSeries(id)">+ {{ descriptorById.get(id)?.label }}</button></section>
        </template>
        <p v-else-if="investigationError" class="investigation-failure">{{ investigationError }}</p>
        <footer><span>{{ investigation?.modelId ?? investigationPort.name }} · read only</span><button @click="investigationDialog?.close()">Return to atlas</button></footer>
      </dialog>
    </Teleport>
  </section>
</template>

<style scoped>
.market-atlas{min-width:0;color:var(--atlas-ink);background:#050b12;border-bottom:1px solid var(--atlas-rule)}button,input,select{color:inherit}.atlas-titlebar{display:flex;min-height:52px;justify-content:space-between;gap:20px;align-items:center;padding:0 16px;border-bottom:1px solid var(--atlas-rule);background:#07101a}.atlas-titlebar>div{display:flex;gap:12px;align-items:baseline}.atlas-titlebar h1{margin:0;font-size:18px;font-weight:610;letter-spacing:.19em;text-transform:uppercase}.atlas-titlebar>div span{color:var(--atlas-axis);font:8px var(--vp-font-family-mono);text-transform:uppercase}.atlas-titlebar dl,.atlas-titlebar dd{margin:0}.atlas-titlebar dt{color:var(--atlas-axis);font:7px var(--vp-font-family-mono);text-transform:uppercase}.atlas-titlebar dd{margin-top:4px;font:9px var(--vp-font-family-mono)}.atlas-titlebar dd small{color:var(--atlas-axis)}
.atlas-toolbar{display:flex;min-height:44px;gap:8px;align-items:center;padding:0 11px;border-bottom:1px solid var(--atlas-rule);background:#08121c}.series-search{display:flex;width:210px;height:29px;align-items:center;border:1px solid var(--atlas-rule);background:#07101a}.series-search svg{width:13px;margin:0 7px;fill:none;stroke:var(--atlas-axis);stroke-width:1.3}.series-search input{min-width:0;flex:1;height:100%;padding:0;border:0;outline:0;background:transparent;font:9px var(--vp-font-family-mono)}.tool-label{margin-left:2px;color:var(--atlas-axis);font:7px var(--vp-font-family-mono);text-transform:uppercase}.segmented,.window-buttons{display:flex}.segmented button,.window-buttons button{height:29px;min-width:31px;padding:0 8px;color:var(--atlas-axis);font:8px var(--vp-font-family-mono);border:1px solid var(--atlas-rule);border-right:0;background:transparent;cursor:pointer}.segmented button:last-child,.window-buttons button:last-child{border-right:1px solid var(--atlas-rule)}.segmented button[aria-pressed=true],.window-buttons button[aria-pressed=true]{color:#b9d5ff;background:#10213a;box-shadow:inset 0 -1px var(--atlas-blue)}.window-buttons{margin-left:auto}.manage-series{display:flex;height:29px;gap:7px;align-items:center;padding:0 9px;color:#b9d5ff;font:8px var(--vp-font-family-mono);border:1px solid var(--atlas-rule);background:transparent;cursor:pointer}.manage-series svg{width:12px;fill:none;stroke:currentColor;stroke-width:1}.manage-series b{display:grid;min-width:15px;height:15px;place-items:center;background:var(--atlas-blue);color:#07101a;font-size:7px}
.series-drawer{position:relative;z-index:8;border-bottom:1px solid var(--atlas-blue);background:#07101a}.series-drawer>header{display:flex;min-height:44px;justify-content:space-between;align-items:center;padding:0 13px;border-bottom:1px solid var(--atlas-rule)}.series-drawer>header div{display:flex;gap:9px;align-items:baseline}.series-drawer>header strong{font-size:11px}.series-drawer>header span{color:var(--atlas-axis);font:8px var(--vp-font-family-mono)}.series-drawer>header button{color:var(--atlas-blue);font:8px var(--vp-font-family-mono);border:0;background:transparent;cursor:pointer}.series-options{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));max-height:270px;overflow:auto}.series-options article{display:grid;grid-template-columns:minmax(0,1fr) 108px;min-width:0;border-right:1px solid var(--atlas-rule-soft);border-bottom:1px solid var(--atlas-rule-soft)}.series-toggle{display:grid;grid-template-columns:5px minmax(0,1fr) auto;gap:8px;align-items:center;min-height:58px;padding:8px 10px;text-align:left;border:0;background:transparent;cursor:pointer}.series-toggle:hover{background:#0c1925}.series-toggle i{width:5px;height:30px}.series-toggle span{display:grid;gap:3px;min-width:0}.series-toggle strong,.series-toggle small{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.series-toggle strong{font-size:9px}.series-toggle small{color:var(--atlas-axis);font:7px var(--vp-font-family-mono)}.series-toggle b{color:var(--atlas-axis);font:7px var(--vp-font-family-mono);text-transform:uppercase}.series-options article[data-active=true] .series-toggle b{color:var(--atlas-green-ink)}.series-options label{display:grid;align-content:center;gap:4px;padding:0 7px;color:var(--atlas-axis);font:7px var(--vp-font-family-mono);text-transform:uppercase;border-left:1px solid var(--atlas-rule-soft)}.series-options select{min-width:0;height:24px;border:1px solid var(--atlas-rule);background:#08121c;font:7px var(--vp-font-family-mono)}
.atlas-scroll{min-width:0;overflow-x:auto;overscroll-behavior-inline:contain}.atlas-frame{min-width:1120px}.atlas-main{display:grid;grid-template-columns:190px minmax(680px,1fr) 286px;height:636px;border-bottom:1px solid var(--atlas-rule)}.series-register{height:100%;border-right:1px solid var(--atlas-rule);background:#07101a}.series-register-body{display:grid;height:546px}.series-register article{display:grid;grid-template-columns:5px minmax(0,1fr) auto;gap:7px;align-content:center;align-items:center;min-height:0;padding:4px 9px;border-bottom:1px solid var(--atlas-rule-soft)}.series-register article>i{width:5px;height:22px}.series-register article>div{display:grid;gap:2px;min-width:0}.series-register strong,.series-register small{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.series-register strong{font-size:9px;font-weight:550}.series-register small{color:var(--atlas-axis);font:7px var(--vp-font-family-mono)}.series-register article>b{color:var(--atlas-ink);font:8px var(--vp-font-family-mono);font-weight:500}.chart-column{display:grid;grid-template-rows:546px 48px 42px;min-width:0;background:#07101a}.chart-stage{position:relative;min-width:0;height:546px}.chart-host{position:absolute;inset:0}.empty-atlas{display:grid;height:546px;place-content:center;justify-items:center;gap:8px;color:var(--atlas-axis);text-align:center;background-image:linear-gradient(var(--atlas-rule-soft) 1px,transparent 1px),linear-gradient(90deg,var(--atlas-rule-soft) 1px,transparent 1px);background-size:44px 44px}.empty-atlas span{font:7px var(--vp-font-family-mono);letter-spacing:.08em;text-transform:uppercase}.empty-atlas strong{max-width:280px;color:var(--atlas-ink);font-size:11px;font-weight:520}.empty-atlas button{height:28px;padding:0 10px;color:#b9d5ff;font:8px var(--vp-font-family-mono);border:1px solid var(--atlas-blue);background:#0d2240;cursor:pointer}.selection-window{position:absolute;z-index:3;top:0;bottom:0;pointer-events:none;background:rgba(79,148,238,.075);border-inline:1px solid rgba(185,213,255,.82)}.selection-window i{position:absolute;width:5px;height:5px;left:-3px;background:#dfe8ec}.selection-window i:first-child{top:-1px}.selection-window i:last-child{bottom:-1px}.event-rails{display:grid;grid-template-rows:repeat(3,1fr);border-top:1px solid var(--atlas-rule);background:#07101a}.event-rails>div{display:grid;grid-template-columns:78px minmax(0,1fr);min-height:0;border-bottom:1px solid var(--atlas-rule-soft)}.event-rails strong{display:flex;align-items:center;padding-left:8px;color:var(--atlas-axis);font:7px var(--vp-font-family-mono);border-right:1px solid var(--atlas-rule-soft)}.event-rails>div>span{position:relative}.event-rails button{position:absolute;top:50%;width:12px;height:12px;padding:0;transform:translate(-50%,-50%);border:0;background:transparent;cursor:pointer}.event-rails button i{display:block;width:6px;height:6px;margin:auto;transform:rotate(45deg);background:var(--atlas-blue)}.event-rails button[data-kind=fill] i{background:var(--atlas-green)}.event-rails button[data-kind=alert] i{background:var(--atlas-oxide)}.event-rails button[data-kind=model] i{background:var(--atlas-violet)}.event-rails button[data-kind=risk] i{background:var(--atlas-amber)}.event-rails button[data-kind=ack] i{background:var(--atlas-cyan)}.event-rails button[data-kind=replace] i{background:var(--atlas-amber)}
.global-scrubber{display:grid;grid-template-columns:minmax(0,1fr) auto;align-items:center;border-top:1px solid var(--atlas-rule);background:#08121c}.navigator{position:relative;height:26px;margin:0 8px}.navigator svg{position:absolute;inset:3px 0;width:100%;height:20px}.navigator polyline{fill:none;stroke:#315578;stroke-width:.5}.navigator-viewport{position:absolute;z-index:1;top:1px;bottom:1px;border:1px solid rgba(176,186,196,.38);background:rgba(176,186,196,.035)}.navigator-selection{position:absolute;z-index:2;top:2px;bottom:2px;border:1px solid var(--atlas-blue);background:rgba(79,148,238,.15)}.navigator-cursor{position:absolute;z-index:3;top:0;bottom:0;width:1px;background:#dfe8ec}.navigator input{position:absolute;z-index:4;inset:0;width:100%;height:100%;margin:0;pointer-events:none;appearance:none;background:transparent}.navigator input::-webkit-slider-thumb{width:8px;height:24px;pointer-events:auto;appearance:none;border:1px solid #b9d5ff;background:#10213a;cursor:ew-resize}.navigator .cursor-range::-webkit-slider-thumb{width:5px;height:5px;border:0;border-radius:50%;background:#fff}.scrubber-actions{display:flex;gap:5px;align-items:center;padding-right:7px}.scrubber-actions button{height:25px;padding:0 7px;color:var(--atlas-axis);font:7px var(--vp-font-family-mono);border:1px solid var(--atlas-rule);background:transparent;cursor:pointer}.scrubber-actions time{min-width:106px;color:#b9d5ff;font:7px var(--vp-font-family-mono);text-align:center}
.evidence-rail{display:flex;min-width:0;flex-direction:column;border-left:1px solid var(--atlas-rule);background:#08121c}.evidence-rail>header{display:flex;min-height:51px;justify-content:space-between;gap:8px;align-items:center;padding:0 10px;border-bottom:1px solid var(--atlas-rule)}.evidence-rail>header div{display:grid;gap:3px}.evidence-rail>header span,.evidence-rail>header b{color:var(--atlas-axis);font:7px var(--vp-font-family-mono);text-transform:uppercase}.evidence-rail>header strong{font:10px var(--vp-font-family-mono)}.rail-section{border-bottom:1px solid var(--atlas-rule)}.rail-section h2{margin:0;padding:8px 10px 6px;color:var(--atlas-ink);font:8px var(--vp-font-family-mono);letter-spacing:.04em;text-transform:uppercase}.observation dl,.decision dl{margin:0}.observation dl>div,.decision dl>div{display:grid;grid-template-columns:minmax(0,1fr) auto;gap:9px;align-items:center;min-height:23px;padding:0 10px}.observation dt,.decision dt,.observation dd,.decision dd{margin:0;font:7px var(--vp-font-family-mono)}.observation dt,.decision dt{color:var(--atlas-axis)}.observation dd,.decision dd{color:var(--atlas-ink)}.observation [data-late=true] dd{color:var(--atlas-oxide)}.late p{display:grid;grid-template-columns:68px 1fr;gap:6px;margin:0;padding:4px 10px;color:var(--atlas-axis);font:7px var(--vp-font-family-mono)}.late h2{color:var(--atlas-amber)}.late time{color:var(--atlas-oxide)}.decision{padding-bottom:6px}.decision dd{text-align:right}.decision dd[data-tone=authorized]{color:var(--atlas-green-ink)}.decision dd[data-tone=closed]{color:var(--atlas-amber)}.investigate{height:31px;margin:9px 10px;color:#b9d5ff;font:8px var(--vp-font-family-mono);border:1px solid var(--atlas-blue);background:#0d2240;cursor:pointer}.investigate:disabled{opacity:.55}.finding p{margin:0;padding:0 10px 6px;color:var(--atlas-muted);font-size:8px;line-height:1.4}.finding small{display:block;padding:0 10px 7px;color:var(--atlas-axis);font:7px var(--vp-font-family-mono)}.suggestions{margin-top:auto}.suggestions>button{display:grid;grid-template-columns:6px minmax(0,1fr) auto;gap:7px;align-items:start;width:100%;padding:6px 10px;text-align:left;border:0;border-top:1px solid var(--atlas-rule-soft);background:transparent;cursor:pointer}.suggestions button>i{width:6px;height:6px;margin-top:2px}.suggestions button>span{display:grid;gap:2px;font-size:8px}.suggestions small{color:var(--atlas-axis);font:7px var(--vp-font-family-mono)}.suggestions button>b{color:var(--atlas-blue);font:7px var(--vp-font-family-mono)}.linked-record{display:flex;height:29px;justify-content:space-between;align-items:center;padding:0 10px;color:var(--atlas-blue);font:7px var(--vp-font-family-mono);border:0;border-top:1px solid var(--atlas-rule);background:transparent;cursor:pointer}.rail-error{margin:0;padding:6px 10px;color:var(--atlas-oxide);font:7px var(--vp-font-family-mono)}
.ledger-dock{display:grid;grid-template-columns:1fr 1fr .95fr .95fr;min-height:142px;background:#07101a}.ledger-dock>section{min-width:0;border-right:1px solid var(--atlas-rule)}.ledger-dock header{display:flex;height:30px;gap:7px;align-items:center;padding:0 9px;border-bottom:1px solid var(--atlas-rule)}.ledger-dock h2{margin:0;font-size:10px;font-weight:550}.ledger-dock header span{display:grid;min-width:15px;height:15px;place-items:center;color:var(--atlas-axis);font:7px var(--vp-font-family-mono)}.ledger-dock header .live{color:var(--atlas-green-ink);border:1px solid rgba(89,183,124,.4)}.ledger-dock header .alert-count{color:#07101a;background:var(--atlas-amber)}.ledger-scroll{overflow:auto}.ledger-dock table{width:100%;min-width:330px;border-collapse:collapse}.ledger-dock th,.ledger-dock td{height:25px;padding:0 6px;overflow:hidden;color:var(--atlas-muted);font:7px var(--vp-font-family-mono);text-align:left;text-overflow:ellipsis;white-space:nowrap;border-bottom:1px solid var(--atlas-rule-soft)}.ledger-dock th{height:19px;color:var(--atlas-axis);font-size:7px;font-weight:500;text-transform:uppercase}.ledger-dock tbody tr:hover,.ledger-dock tbody tr.selected{background:#10213a}.ledger-order-button{padding:0;color:#b9d5ff;font:inherit;border:0;border-bottom:1px solid rgba(79,148,238,.42);background:transparent;cursor:pointer}.ledger-order-button:hover{color:#fff;border-color:var(--atlas-blue)}.ledger-empty{height:44px!important;color:var(--atlas-axis)!important;text-align:center!important}.ledger-dock [data-tone=buy],.ledger-dock [data-tone=info]{color:var(--atlas-green-ink);text-transform:uppercase}.ledger-dock [data-tone=sell],.ledger-dock [data-tone=critical]{color:var(--atlas-oxide);text-transform:uppercase}.ledger-dock [data-tone=warning]{color:var(--atlas-amber);text-transform:uppercase}.reconcile-list p{display:grid;grid-template-columns:1fr auto;margin:0;padding:6px 9px;color:var(--atlas-muted);font:7px var(--vp-font-family-mono);border-bottom:1px solid var(--atlas-rule-soft)}.reconcile-list b{font-weight:500}.reconcile-list .ledger-empty{display:block;padding-top:17px}
.market-atlas{--atlas-plot-height:max(546px,calc(100dvh - 354px))}.history-label{margin-left:auto}.window-buttons{margin-left:0}.forecast-strip{display:flex;min-height:38px;gap:6px;align-items:center;padding:5px 11px;overflow-x:auto;border-bottom:1px solid var(--atlas-rule);background:#07101a}.forecast-strip>span{margin-right:3px;color:var(--atlas-axis);font:7px var(--vp-font-family-mono);text-transform:uppercase;white-space:nowrap}.forecast-strip button{display:flex;min-height:27px;gap:6px;align-items:center;padding:0 8px;color:var(--atlas-muted);font:8px var(--vp-font-family-mono);white-space:nowrap;border:1px solid var(--atlas-rule);background:transparent;cursor:pointer}.forecast-strip button[aria-pressed=true]{color:#c6dcfb;border-color:#365a83;background:#10213a}.forecast-strip button i{width:6px;height:6px;border-radius:50%;background:var(--atlas-blue)}.forecast-strip button i[data-state=eligible]{background:var(--atlas-green)}.forecast-strip button i[data-state=blocked]{background:var(--atlas-oxide)}.forecast-strip button b{color:var(--atlas-axis);font-size:7px}.forecast-strip .forecast-custom{margin-left:auto}
.bundle-registry{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));border-bottom:1px solid var(--atlas-rule)}.bundle-registry details{min-width:0;border-right:1px solid var(--atlas-rule-soft)}.bundle-registry summary{display:grid;grid-template-columns:7px minmax(0,1fr) auto;gap:8px;align-items:start;min-height:58px;padding:10px;cursor:pointer;list-style:none}.bundle-registry summary::-webkit-details-marker{display:none}.bundle-registry summary:hover{background:#0c1925}.bundle-registry summary>i{width:7px;height:7px;margin-top:3px;border-radius:50%;background:var(--atlas-blue)}.bundle-registry summary>i[data-state=eligible]{background:var(--atlas-green)}.bundle-registry summary>i[data-state=blocked]{background:var(--atlas-oxide)}.bundle-registry summary>span{display:grid;gap:4px}.bundle-registry summary strong{font-size:10px}.bundle-registry summary small{overflow:hidden;color:var(--atlas-muted);font-size:8px;line-height:1.35;text-overflow:ellipsis}.bundle-registry summary>b{color:var(--atlas-axis);font:7px var(--vp-font-family-mono);font-weight:500}.bundle-registry details>div{display:flex;justify-content:space-between;gap:10px;align-items:end;padding:0 10px 10px}.bundle-registry p{display:flex;flex-wrap:wrap;gap:4px;margin:0}.bundle-registry p>span{display:flex;gap:4px;align-items:center;padding:4px 5px;color:var(--atlas-muted);font:7px var(--vp-font-family-mono);border:1px solid var(--atlas-rule-soft)}.bundle-registry p>span[data-shared=true]{color:var(--atlas-cyan)}.bundle-registry p b{color:var(--atlas-axis);font-size:6px;text-transform:uppercase}.bundle-registry details>div>button{min-height:28px;flex:0 0 auto;padding:0 8px;color:var(--atlas-blue);font:7px var(--vp-font-family-mono);text-transform:uppercase;border:1px solid var(--atlas-rule);background:transparent;cursor:pointer}
.series-options{grid-template-columns:repeat(2,minmax(0,1fr));max-height:330px}.series-options article{display:block}.pane-choices{display:flex;gap:4px;align-items:center;padding:6px 8px;overflow-x:auto;border-top:1px solid var(--atlas-rule-soft)}.pane-choices>span{margin-right:2px;color:var(--atlas-axis);font:7px var(--vp-font-family-mono);text-transform:uppercase}.pane-choices button{min-height:25px;padding:0 7px;color:var(--atlas-muted);font:7px var(--vp-font-family-mono);white-space:nowrap;border:1px solid var(--atlas-rule);background:transparent;cursor:pointer}.pane-choices button[aria-pressed=true]{color:var(--atlas-blue);border-color:#365a83;background:#10213a}
.atlas-main{height:calc(var(--atlas-plot-height) + 90px)}.series-register-body{height:var(--atlas-plot-height)}.series-register article{grid-template-columns:16px minmax(0,1fr) 18px;padding:3px 5px}.series-register article[data-dragging=true]{opacity:.45}.lane-grip{display:grid;width:16px;height:30px;place-items:center;padding:0;color:var(--atlas-axis);border:0;background:transparent;cursor:grab}.lane-grip:active{cursor:grabbing}.lane-grip svg{width:9px;fill:currentColor}.lane-series{display:grid;gap:3px;min-width:0}.lane-series>span{display:grid;grid-template-columns:4px minmax(0,1fr) auto;gap:6px;align-items:center;min-width:0}.lane-series>span>i{width:4px;height:18px}.lane-series>span>span{display:grid;min-width:0}.lane-series strong,.lane-series small{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.lane-series strong{font-size:8px}.lane-series small{color:var(--atlas-axis);font:6px var(--vp-font-family-mono)}.lane-series small b{color:var(--atlas-cyan);font-weight:500}.lane-series>span>b{font:7px var(--vp-font-family-mono);font-weight:500}.lane-actions{display:grid;align-content:center}.lane-actions button{display:grid;width:18px;height:18px;place-items:center;padding:0;color:var(--atlas-axis);border:0;background:transparent;cursor:pointer}.lane-actions button:hover{color:var(--atlas-blue);background:var(--atlas-blue-soft)}.lane-actions button:disabled{opacity:.22;cursor:default}.lane-actions svg{width:11px;fill:none;stroke:currentColor;stroke-width:1.25}.chart-column{grid-template-rows:var(--atlas-plot-height) 48px 42px}.chart-stage,.empty-atlas{height:var(--atlas-plot-height)}
.investigation-dialog{width:min(760px,calc(100vw - 32px));max-height:min(780px,calc(100dvh - 32px));padding:0;color:var(--atlas-ink);border:1px solid #365a83;border-radius:0;background:#07101a;box-shadow:0 24px 80px rgba(0,0,0,.52)}.investigation-dialog::backdrop{background:rgba(1,5,9,.78);backdrop-filter:blur(3px)}.investigation-dialog>header,.investigation-dialog>footer{display:flex;min-height:54px;justify-content:space-between;gap:16px;align-items:center;padding:0 16px;border-bottom:1px solid var(--atlas-rule)}.investigation-dialog>header>div{display:grid;gap:3px}.investigation-dialog>header strong{font-size:14px}.investigation-dialog>header span,.investigation-dialog>footer span{color:var(--atlas-axis);font:8px var(--vp-font-family-mono);text-transform:uppercase}.investigation-dialog>header button{display:grid;width:34px;height:34px;place-items:center;color:var(--atlas-muted);border:0;background:transparent;cursor:pointer}.investigation-dialog>header svg{width:14px;fill:none;stroke:currentColor;stroke-width:1.4}.investigation-dialog>section{padding:18px 20px;border-bottom:1px solid var(--atlas-rule)}.investigation-dialog h2{margin:0 0 10px;font-size:10px;font-weight:600;text-transform:uppercase}.investigation-dialog p{max-width:70ch;margin:0;color:var(--atlas-ink);font-size:13px;line-height:1.55}.investigation-dialog section>small{display:block;margin-top:9px;color:var(--atlas-muted);font-size:10px}.investigation-inputs{display:flex;flex-wrap:wrap;gap:5px}.investigation-inputs span{display:flex;gap:6px;align-items:center;padding:6px 7px;color:var(--atlas-muted);font:8px var(--vp-font-family-mono);border:1px solid var(--atlas-rule)}.investigation-inputs i{width:5px;height:12px}.investigation-dialog ol{display:grid;gap:7px;margin:0;padding:0;list-style:none}.investigation-dialog li{display:grid;grid-template-columns:92px minmax(0,1fr) auto;gap:10px;align-items:center}.investigation-dialog li time,.investigation-dialog li small{color:var(--atlas-axis);font:8px var(--vp-font-family-mono)}.investigation-dialog li strong{font-size:10px}.investigation-dialog section>button{min-height:30px;margin:0 5px 5px 0;padding:0 9px;color:var(--atlas-blue);font:8px var(--vp-font-family-mono);border:1px solid var(--atlas-rule);background:transparent;cursor:pointer}.investigation-loading{display:flex;min-height:260px;gap:10px;align-items:center;justify-content:center;color:var(--atlas-muted);font:9px var(--vp-font-family-mono)}.investigation-loading i{width:8px;height:8px;background:var(--atlas-blue);animation:pulse 1.4s ease-in-out infinite}.investigation-failure{padding:28px;color:var(--atlas-oxide)!important}.investigation-dialog>footer{border-top:1px solid var(--atlas-rule);border-bottom:0}.investigation-dialog>footer button{min-height:32px;padding:0 10px;color:#c6dcfb;font:8px var(--vp-font-family-mono);border:1px solid var(--atlas-blue);background:#10213a;cursor:pointer}
.atlas-state{display:flex;min-height:570px;gap:9px;align-items:center;justify-content:center;color:var(--atlas-axis);font:8px var(--vp-font-family-mono);text-transform:uppercase}.atlas-state i{width:7px;height:7px;background:var(--atlas-blue);animation:pulse 1.4s ease-in-out infinite}.atlas-state.error i{background:var(--atlas-oxide)}.atlas-state button{padding:5px 8px;color:var(--atlas-blue);font:8px var(--vp-font-family-mono);border:1px solid var(--atlas-rule);background:transparent;cursor:pointer}button:focus-visible,input:focus-visible,select:focus-visible,.atlas-scroll:focus-visible{outline:2px solid var(--atlas-blue);outline-offset:-2px}@keyframes pulse{50%{opacity:.25}}
@media(max-width:1400px) and (min-width:721px){.atlas-frame{min-width:1000px}.atlas-main{grid-template-columns:174px minmax(584px,1fr) 250px}.scrubber-actions time{min-width:92px}.scrubber-actions button{padding-inline:5px}}@media(max-width:1180px){.bundle-registry,.series-options{grid-template-columns:1fr}}@media(max-width:720px){.market-atlas{--atlas-plot-height:546px}.atlas-titlebar{padding:0 11px}.atlas-titlebar h1{font-size:16px}.atlas-titlebar>div span{display:none}.atlas-titlebar dl{display:none}.atlas-toolbar{overflow-x:auto}.series-search{min-width:180px}.tool-label{display:none}.history-label{display:none}.window-buttons{margin-left:0}.manage-series{min-width:max-content}.series-drawer>header span{display:none}.bundle-registry{grid-template-columns:1fr}.atlas-frame{min-width:1080px}.atlas-main{grid-template-columns:174px 660px 246px}.ledger-dock{grid-template-columns:repeat(4,270px)}.atlas-scroll::before{display:block;padding:5px 10px;color:var(--atlas-axis);font:7px var(--vp-font-family-mono);content:"Swipe horizontally to inspect the complete atlas";text-transform:uppercase;border-bottom:1px solid var(--atlas-rule);background:#08121c}.investigation-dialog{width:calc(100vw - 16px);max-height:calc(100dvh - 16px)}.investigation-dialog li{grid-template-columns:1fr}.investigation-dialog li small{display:none}}@media(prefers-reduced-motion:reduce){.atlas-state i,.investigation-loading i{animation:none}}
</style>
