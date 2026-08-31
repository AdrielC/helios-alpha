import type { OperationsContext } from "./operations-port";
import executionEvidenceManifest from "../../../../config/forecasts/execution-evidence-v1.json";
import spaceWeatherManifest from "../../../../config/forecasts/space-weather-impact-v1.json";

export type SeriesDomain = "market" | "signal" | "source" | "risk" | "portfolio" | "execution";
export type SeriesRender = "candlestick" | "bar" | "histogram" | "line" | "area" | "baseline";
export type SeriesTransform = "raw" | "indexed" | "percent_change" | "z_score";
export type MarkerKind = "order" | "ack" | "fill" | "cancel" | "replace" | "alert" | "model" | "risk";

export interface TimeSeriesDescriptor {
  readonly id: string;
  readonly label: string;
  readonly shortLabel: string;
  readonly domain: SeriesDomain;
  readonly unit: string;
  readonly precision: number;
  readonly color: string;
  readonly render: SeriesRender;
  readonly provenance: string;
  readonly sourceNames?: readonly string[];
  readonly freshness: string;
  readonly defaultVisible: boolean;
  readonly paneWeight?: number;
}

export interface TimeSeriesScalarPoint {
  readonly kind: "scalar";
  readonly timestamp: string;
  readonly availableAt: string;
  readonly value: number;
  readonly color?: string;
}

export interface TimeSeriesOhlcPoint {
  readonly kind: "ohlc";
  readonly timestamp: string;
  readonly availableAt: string;
  readonly open: number;
  readonly high: number;
  readonly low: number;
  readonly close: number;
}

export type TimeSeriesPoint = TimeSeriesScalarPoint | TimeSeriesOhlcPoint;

export interface TimeSeriesData {
  readonly descriptor: TimeSeriesDescriptor;
  readonly points: readonly TimeSeriesPoint[];
}

export interface TimelineMarker {
  readonly id: string;
  readonly timestamp: string;
  readonly availableAt: string;
  readonly kind: MarkerKind;
  readonly label: string;
  readonly entityId: string;
  readonly detail: string;
  readonly attributes?: Readonly<Record<string, string | number | boolean>>;
}

export interface TimeSeriesWindow {
  readonly schemaVersion: 2;
  readonly sequence: number;
  readonly from: string;
  readonly to: string;
  readonly series: readonly TimeSeriesData[];
  readonly markers: readonly TimelineMarker[];
}

export interface TimeSeriesRequest {
  readonly context: OperationsContext;
  readonly seriesIds: readonly string[];
  readonly from: string;
  readonly to: string;
  readonly maxPoints: number;
}

export interface TimeSeriesPort {
  readonly name: string;
  catalog(context: OperationsContext): Promise<readonly TimeSeriesDescriptor[]>;
  forecastBundles(context: OperationsContext): Promise<readonly ForecastBundle[]>;
  query(request: TimeSeriesRequest): Promise<TimeSeriesWindow>;
}

export interface TimelineLane {
  readonly id: string;
  readonly seriesIds: readonly string[];
  readonly weight?: number;
}

export interface TimelineWorkspace {
  readonly schemaVersion: 2;
  readonly transform: SeriesTransform;
  readonly lanes: readonly TimelineLane[];
  readonly windowMinutes: number;
  readonly forecastBundleIds?: readonly string[];
}

export interface ForecastBundle {
  readonly schemaVersion: 1;
  readonly bundleVersion: number;
  readonly definitionSha256: string;
  readonly id: string;
  readonly label: string;
  readonly thesis: string;
  readonly horizon: string;
  readonly state: "monitoring" | "eligible" | "blocked";
  readonly strategyIds: readonly string[];
  readonly seriesIds: readonly string[];
  readonly sharedSeriesIds: readonly string[];
  readonly inputContract: readonly ForecastInputRequirement[];
}

export interface ForecastInputRequirement {
  readonly seriesId: string;
  readonly role: string;
  readonly required: boolean;
  readonly maxAgeSeconds: number;
  readonly sourceIds: readonly string[];
}

const descriptors: readonly TimeSeriesDescriptor[] = [
  { id: "market-ohlc", label: "Price", shortLabel: "Price", domain: "market", unit: "USD", precision: 2, color: "#78a9ef", render: "candlestick", provenance: "ESZ4 consolidated mark", sourceNames: ["MARKET"], freshness: "42ms", defaultVisible: true, paneWeight: 1.65 },
  { id: "bid-ask-spread", label: "Bid / ask spread", shortLabel: "Spread", domain: "market", unit: "bps", precision: 2, color: "#d9a83e", render: "line", provenance: "ESZ4 top of book", sourceNames: ["MARKET"], freshness: "42ms", defaultVisible: true },
  { id: "market-volume", label: "Volume", shortLabel: "Volume", domain: "market", unit: "volume", precision: 0, color: "#4f94ee", render: "histogram", provenance: "ESZ4 trade tape", sourceNames: ["MARKET"], freshness: "42ms", defaultVisible: true, paneWeight: 0.82 },
  { id: "goes-xray-flux", label: "GOES X-ray flux", shortLabel: "X-ray", domain: "source", unit: "W/m2", precision: 9, color: "#f5bf42", render: "line", provenance: "NOAA SWPC GOES primary X-ray flux", sourceNames: ["noaa-swpc-goes-xray-primary-v1"], freshness: "shadow", defaultVisible: false },
  { id: "goes-proton-flux-ge10", label: "GOES proton flux", shortLabel: "Protons", domain: "source", unit: "pfu", precision: 3, color: "#ed7655", render: "line", provenance: "NOAA SWPC GOES primary integral proton flux", sourceNames: ["noaa-swpc-goes-protons-primary-v1"], freshness: "shadow", defaultVisible: false },
  { id: "donki-flare-events", label: "DONKI solar flares", shortLabel: "Flares", domain: "source", unit: "event", precision: 0, color: "#ff9e64", render: "histogram", provenance: "NASA CCMC DONKI flare revisions", sourceNames: ["nasa-ccmc-donki-flare-v1"], freshness: "shadow", defaultVisible: false },
  { id: "donki-cme-analysis", label: "DONKI CME analysis", shortLabel: "CME", domain: "source", unit: "km/s", precision: 0, color: "#db6d9b", render: "histogram", provenance: "NASA CCMC DONKI CME and WSA-ENLIL revisions", sourceNames: ["nasa-ccmc-donki-cme-v1"], freshness: "shadow", defaultVisible: false },
  { id: "l1-solar-wind-speed", label: "Solar wind speed", shortLabel: "Wind", domain: "source", unit: "km/s", precision: 2, color: "#46c7d7", render: "line", provenance: "NOAA SWPC active L1 solar-wind source", sourceNames: ["noaa-swpc-l1-wind-1m-v1"], freshness: "shadow", defaultVisible: false },
  { id: "l1-imf-bz-gsm", label: "Interplanetary magnetic field Bz", shortLabel: "IMF Bz", domain: "source", unit: "nT", precision: 2, color: "#cf73ff", render: "baseline", provenance: "NOAA SWPC active L1 magnetometer source", sourceNames: ["noaa-swpc-l1-mag-1m-v1"], freshness: "shadow", defaultVisible: false },
  { id: "planetary-kp", label: "Planetary Kp", shortLabel: "Kp", domain: "source", unit: "index", precision: 2, color: "#d6c05c", render: "line", provenance: "NOAA SWPC one-minute planetary K index", sourceNames: ["noaa-swpc-planetary-kp-1m-v1"], freshness: "shadow", defaultVisible: false },
  { id: "signal-strength", label: "Signal strength", shortLabel: "Signal", domain: "signal", unit: "%", precision: 2, color: "#59b77c", render: "line", provenance: "Atlas model", freshness: "decision cut", defaultVisible: true },
  { id: "signal-posterior", label: "Posterior probability", shortLabel: "Posterior", domain: "signal", unit: "%", precision: 2, color: "#a16de0", render: "line", provenance: "Atlas model", freshness: "decision cut", defaultVisible: true },
  { id: "source-latency", label: "Source latency p95", shortLabel: "Latency", domain: "source", unit: "ms", precision: 0, color: "#e17455", render: "line", provenance: "All source adapters", sourceNames: ["GOES-R", "DSCOVR", "SWPC", "MARKET"], freshness: "1s", defaultVisible: true },
  { id: "net-exposure", label: "Net exposure", shortLabel: "Exposure", domain: "portfolio", unit: "USD", precision: 0, color: "#47c2cf", render: "area", provenance: "Fill-derived positions", freshness: "157ms", defaultVisible: true },
  { id: "participation", label: "Capacity / participation", shortLabel: "Capacity", domain: "execution", unit: "%", precision: 1, color: "#d6c05c", render: "line", provenance: "Execution measurement", freshness: "fill event", defaultVisible: true },
  { id: "realized-pnl", label: "P&L realized (cumulative)", shortLabel: "Realized", domain: "portfolio", unit: "USD", precision: 0, color: "#59b77c", render: "area", provenance: "OMS ledger", freshness: "fill event", defaultVisible: true },
  { id: "unrealized-pnl", label: "P&L unrealized", shortLabel: "Unrealized", domain: "portfolio", unit: "USD", precision: 0, color: "#b0bac4", render: "line", provenance: "Position marks", freshness: "157ms", defaultVisible: true },
  { id: "source-quality", label: "Source quality", shortLabel: "Quality", domain: "source", unit: "%", precision: 1, color: "#63c9d4", render: "line", provenance: "Source fence composite", sourceNames: ["GOES-R", "DSCOVR", "SWPC", "MARKET"], freshness: "184ms", defaultVisible: false },
  { id: "risk-utilization", label: "Gross risk utilization", shortLabel: "Risk", domain: "risk", unit: "%", precision: 1, color: "#e5a12c", render: "line", provenance: "Independent risk authority", freshness: "642ms", defaultVisible: false },
];

const descriptorById = new Map(descriptors.map((descriptor) => [descriptor.id, descriptor]));

const forecastBundles = validateForecastBundles([
  {
    ...spaceWeatherManifest,
    definitionSha256: "5caa1582b16ccd00bd3e3cfe85ec1f88574f4584ab476bd3f9c5446c69cead83",
  },
  {
    ...executionEvidenceManifest,
    definitionSha256: "88fbca3fbe175f8b998e76b7d99b588943926a7b6146ef728f17b17fa0467c7d",
  },
]);

function sameOrigin(candidate: string, field: string): string {
  const url = new URL(candidate, window.location.href);
  if (url.origin !== window.location.origin) throw new Error(`${field} must be same-origin`);
  return url.href;
}

function finiteNumber(value: unknown, path: string): number {
  if (typeof value !== "number" || !Number.isFinite(value)) throw new Error(`Invalid time-series response at ${path}`);
  return value;
}

function text(value: unknown, path: string): string {
  if (typeof value !== "string" || value.length === 0) throw new Error(`Invalid time-series response at ${path}`);
  return value;
}

function textArray(value: unknown, path: string, allowEmpty = true): readonly string[] {
  if (!Array.isArray(value) || (!allowEmpty && value.length === 0)) throw new Error(`Invalid forecast bundle at ${path}`);
  const result = value.map((item, index) => text(item, `${path}[${index}]`));
  if (new Set(result).size !== result.length) throw new Error(`Invalid forecast bundle at ${path}: duplicate values`);
  return result;
}

export function validateForecastBundles(value: unknown): readonly ForecastBundle[] {
  if (!Array.isArray(value)) throw new Error("Invalid forecast bundle registry");
  return value.map((candidate, bundleIndex) => {
    const path = `bundles[${bundleIndex}]`;
    if (!candidate || typeof candidate !== "object" || Array.isArray(candidate)) throw new Error(`Invalid forecast bundle at ${path}`);
    const bundle = candidate as Record<string, unknown>;
    if (bundle.schemaVersion !== 1 || !Number.isSafeInteger(bundle.bundleVersion) || (bundle.bundleVersion as number) <= 0) throw new Error(`Invalid forecast bundle version at ${path}`);
    if (typeof bundle.definitionSha256 !== "string" || !/^[0-9a-f]{64}$/.test(bundle.definitionSha256)) throw new Error(`Invalid forecast bundle fingerprint at ${path}`);
    text(bundle.id, `${path}.id`);
    text(bundle.label, `${path}.label`);
    text(bundle.thesis, `${path}.thesis`);
    text(bundle.horizon, `${path}.horizon`);
    if (!(["monitoring", "eligible", "blocked"] as const).includes(bundle.state as "monitoring")) throw new Error(`Invalid forecast bundle state at ${path}`);
    textArray(bundle.strategyIds, `${path}.strategyIds`);
    const seriesIds = textArray(bundle.seriesIds, `${path}.seriesIds`, false);
    const sharedSeriesIds = textArray(bundle.sharedSeriesIds, `${path}.sharedSeriesIds`);
    if (sharedSeriesIds.some((id) => !seriesIds.includes(id))) throw new Error(`Invalid shared series at ${path}`);
    if (!Array.isArray(bundle.inputContract) || bundle.inputContract.length === 0) throw new Error(`Invalid input contract at ${path}`);
    const inputIds = bundle.inputContract.map((inputCandidate, inputIndex) => {
      const inputPath = `${path}.inputContract[${inputIndex}]`;
      if (!inputCandidate || typeof inputCandidate !== "object" || Array.isArray(inputCandidate)) throw new Error(`Invalid forecast input at ${inputPath}`);
      const input = inputCandidate as Record<string, unknown>;
      const seriesId = text(input.seriesId, `${inputPath}.seriesId`);
      text(input.role, `${inputPath}.role`);
      if (typeof input.required !== "boolean") throw new Error(`Invalid required flag at ${inputPath}`);
      if (!Number.isSafeInteger(input.maxAgeSeconds) || (input.maxAgeSeconds as number) <= 0) throw new Error(`Invalid freshness limit at ${inputPath}`);
      textArray(input.sourceIds, `${inputPath}.sourceIds`, false);
      return seriesId;
    });
    if (!bundle.inputContract.some((input) => (input as Record<string, unknown>).required === true)) throw new Error(`Forecast bundle has no required input at ${path}`);
    if (inputIds.length !== seriesIds.length || inputIds.some((id, index) => id !== seriesIds[index])) throw new Error(`Forecast input order differs from seriesIds at ${path}`);
    return bundle as unknown as ForecastBundle;
  });
}

function validateWindow(value: unknown): TimeSeriesWindow {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("Invalid time-series response at root");
  const window = value as Record<string, unknown>;
  if (window.schemaVersion !== 2 || !Array.isArray(window.series) || !Array.isArray(window.markers)) throw new Error("Invalid time-series response envelope");
  text(window.from, "from");
  text(window.to, "to");
  finiteNumber(window.sequence, "sequence");
  for (const [seriesIndex, candidate] of window.series.entries()) {
    if (!candidate || typeof candidate !== "object" || Array.isArray(candidate)) throw new Error(`Invalid time-series response at series[${seriesIndex}]`);
    const series = candidate as Record<string, unknown>;
    if (!series.descriptor || typeof series.descriptor !== "object" || !Array.isArray(series.points)) throw new Error(`Invalid time-series response at series[${seriesIndex}]`);
    const descriptor = series.descriptor as Record<string, unknown>;
    text(descriptor.id, `series[${seriesIndex}].descriptor.id`);
    for (const [pointIndex, pointCandidate] of series.points.entries()) {
      if (!pointCandidate || typeof pointCandidate !== "object" || Array.isArray(pointCandidate)) throw new Error(`Invalid time-series response at series[${seriesIndex}].points[${pointIndex}]`);
      const point = pointCandidate as Record<string, unknown>;
      text(point.timestamp, `series[${seriesIndex}].points[${pointIndex}].timestamp`);
      text(point.availableAt, `series[${seriesIndex}].points[${pointIndex}].availableAt`);
      if (point.kind === "ohlc") {
        finiteNumber(point.open, `series[${seriesIndex}].points[${pointIndex}].open`);
        finiteNumber(point.high, `series[${seriesIndex}].points[${pointIndex}].high`);
        finiteNumber(point.low, `series[${seriesIndex}].points[${pointIndex}].low`);
        finiteNumber(point.close, `series[${seriesIndex}].points[${pointIndex}].close`);
      } else if (point.kind === "scalar") finiteNumber(point.value, `series[${seriesIndex}].points[${pointIndex}].value`);
      else throw new Error(`Invalid time-series response at series[${seriesIndex}].points[${pointIndex}].kind`);
    }
  }
  return window as unknown as TimeSeriesWindow;
}

function pulse(t: number, center: number, width: number): number {
  return Math.exp(-Math.pow((t - center) * width, 2));
}

function scalarWave(id: string, index: number, count: number): number {
  const t = index / Math.max(1, count - 1);
  const shock = pulse(t, 0.47, 19);
  const response = t > 0.47 ? 1 - Math.exp(-(t - 0.47) * 7) : 0;
  const micro = Math.sin(index * 1.71) * 0.38 + Math.sin(index * 0.29) * 0.62;
  if (id === "bid-ask-spread") return 0.19 + Math.abs(Math.sin(index * 0.21)) * 0.08 + shock * 0.55;
  if (id === "market-volume") return 4_100 + Math.abs(Math.sin(index * 0.31)) * 5_600 + Math.abs(micro) * 1_100 + shock * 18_000;
  if (id === "goes-xray-flux") return 2e-7 + shock * 4.8e-5;
  if (id === "goes-proton-flux-ge10") return 0.4 + response * 18 + shock * 4;
  if (id === "donki-flare-events") return shock > 0.65 ? 1 : 0;
  if (id === "donki-cme-analysis") return shock > 0.35 ? 1_420 : 0;
  if (id === "l1-solar-wind-speed") return 380 + response * 310 + Math.sin(t * 12) * 18;
  if (id === "l1-imf-bz-gsm") return 1.5 + Math.sin(t * 16) * 2.8 - response * 9;
  if (id === "planetary-kp") return 2 + response * 4.8 + Math.sin(t * 9) * 0.3;
  if (id === "signal-strength") return 0.42 + Math.sin(t * 17) * 0.07 + response * 0.28 + shock * 0.17;
  if (id === "signal-posterior") return 0.44 + Math.sin(t * 11) * 0.05 + response * 0.21 + shock * 0.09;
  if (id === "source-latency") return 18 + Math.abs(Math.sin(index * 0.18)) * 23 + shock * 105 + pulse(t, 0.76, 36) * 72;
  if (id === "net-exposure") return 820 + Math.sin(t * 12) * 260 + response * 540;
  if (id === "participation") return 7.4 + Math.sin(t * 9) * 1.1 + response * 6.8;
  if (id === "realized-pnl") return 1_900 + t * 9_600 + response * 2_100 + Math.sin(t * 6) * 420;
  if (id === "unrealized-pnl") return -900 + Math.sin(t * 13) * 1_550 + response * 3_200 - shock * 1_100;
  if (id === "source-quality") return 0.96 - shock * 0.21 - pulse(t, 0.76, 36) * 0.12;
  if (id === "risk-utilization") return 0.28 + Math.sin(t * 8) * 0.025 + response * 0.14;
  return 0;
}

function marketClose(index: number, count: number): number {
  const t = index / Math.max(1, count - 1);
  const shock = pulse(t, 0.47, 19);
  const response = t > 0.47 ? 1 - Math.exp(-(t - 0.47) * 8) : 0;
  return 4_971.1 + t * 8.7 + Math.sin(t * 25) * 1.65 + Math.sin(index * 0.47) * 0.34 + shock * 4.7 + response * 2.4;
}

function lagFor(descriptor: TimeSeriesDescriptor, index: number, count: number): number {
  const t = index / Math.max(1, count - 1);
  if (descriptor.domain === "source") return 320 + Math.round(pulse(t, 0.76, 36) * 2_900);
  if (descriptor.domain === "signal") return 210;
  return 42;
}

export class DemoTimeSeriesPort implements TimeSeriesPort {
  readonly name = "DemoTimeSeriesPort";
  private sequence = 44_100;

  async catalog(): Promise<readonly TimeSeriesDescriptor[]> {
    return descriptors;
  }

  async forecastBundles(): Promise<readonly ForecastBundle[]> {
    return forecastBundles;
  }

  async query(request: TimeSeriesRequest): Promise<TimeSeriesWindow> {
    const fromMs = Date.parse(request.from);
    const toMs = Date.parse(request.to);
    if (!Number.isFinite(fromMs) || !Number.isFinite(toMs) || toMs <= fromMs) throw new Error("Invalid time-series query window");
    const count = Math.min(Math.max(120, request.maxPoints), 720);
    const closeByIndex = Array.from({ length: count }, (_, index) => marketClose(index, count));
    const series = request.seriesIds.flatMap((id) => {
      const descriptor = descriptorById.get(id);
      if (!descriptor) return [];
      const points: TimeSeriesPoint[] = Array.from({ length: count }, (_, index) => {
        const timestampMs = fromMs + ((toMs - fromMs) * index) / Math.max(1, count - 1);
        const availableAt = new Date(timestampMs + lagFor(descriptor, index, count)).toISOString();
        const timestamp = new Date(timestampMs).toISOString();
        if (descriptor.render === "candlestick" || descriptor.render === "bar") {
          const close = closeByIndex[index];
          const open = index ? closeByIndex[index - 1] : close - 0.18;
          const wick = 0.24 + Math.abs(Math.sin(index * 1.17)) * 0.48;
          return { kind: "ohlc", timestamp, availableAt, open, high: Math.max(open, close) + wick, low: Math.min(open, close) - wick * 0.82, close };
        }
        const value = scalarWave(id, index, count);
        const color = id === "market-volume" ? (closeByIndex[index] >= (closeByIndex[index - 1] ?? closeByIndex[index]) ? "rgba(79,148,238,.82)" : "rgba(225,116,85,.72)") : undefined;
        return { kind: "scalar", timestamp, availableAt, value, color };
      });
      return [{ descriptor, points }];
    });
    const marker = (id: string, ratio: number, kind: MarkerKind, label: string, entityId: string, detail: string, attributes?: TimelineMarker["attributes"]): TimelineMarker => {
      const timestampMs = fromMs + (toMs - fromMs) * ratio;
      return { id, timestamp: new Date(timestampMs).toISOString(), availableAt: new Date(timestampMs + 140).toISOString(), kind, label, entityId, detail, attributes };
    };
    this.sequence += 1;
    return {
      schemaVersion: 2,
      sequence: this.sequence,
      from: request.from,
      to: request.to,
      series,
      markers: [
        marker("evt-order", 0.35, "order", "ORDER", "f8bcdd0f-650f-4a1e-b9de-a8137d5872ac", "Order admitted by risk", { orderIntent: "SELL 0.058 BTC-USD LMT" }),
        marker("evt-ack", 0.37, "ack", "ACK", "f8bcdd0f-650f-4a1e-b9de-a8137d5872ac", "Venue acknowledgement received", { brokerAcknowledgement: "Accepted" }),
        marker("evt-model", 0.45, "model", "MODEL", "atlas-v2.7.4", "Posterior crossed the decision threshold", { modelVersion: "Atlas v2.7.4" }),
        marker("evt-risk", 0.46, "risk", "RISK", "risk-admission", "Independent risk authority held capital admission", { riskDecision: "HOLD" }),
        marker("evt-fill-1", 0.49, "fill", "FILL", "exec-9f47f6f1", "Partial fill received", { fillResult: "Partial fill · $64,104.08" }),
        marker("evt-alert", 0.76, "alert", "ALERT", "alert-source-gap", "Source latency crossed the warning budget"),
        marker("evt-replace", 0.81, "replace", "REPLACE", "55d30390-9052-4ab6-9f49-071961e80a13", "Working price replaced after spread widened"),
        marker("evt-fill-2", 0.91, "fill", "FILL", "exec-9f47f6f1", "Final fill received", { fillResult: "Final fill · $64,098.00" }),
      ],
    };
  }
}

interface HttpTimeSeriesOptions {
  readonly catalogUrl: string;
  readonly forecastBundlesUrl: string;
  readonly queryUrl: string;
}

export class HttpTimeSeriesPort implements TimeSeriesPort {
  readonly name = "HttpTimeSeriesPort";
  private readonly catalogUrl: string;
  private readonly forecastBundlesUrl: string;
  private readonly queryUrl: string;

  constructor(options: HttpTimeSeriesOptions) {
    this.catalogUrl = sameOrigin(options.catalogUrl, "timeSeriesCatalogUrl");
    this.forecastBundlesUrl = sameOrigin(options.forecastBundlesUrl, "forecastBundlesUrl");
    this.queryUrl = sameOrigin(options.queryUrl, "timeSeriesQueryUrl");
  }

  async catalog(): Promise<readonly TimeSeriesDescriptor[]> {
    const response = await fetch(this.catalogUrl, { credentials: "same-origin", headers: { Accept: "application/json" } });
    if (!response.ok) throw new Error(`Time-series catalog failed with HTTP ${response.status}`);
    const body = await response.json();
    if (!Array.isArray(body)) throw new Error("Invalid time-series catalog");
    return body as readonly TimeSeriesDescriptor[];
  }

  async forecastBundles(): Promise<readonly ForecastBundle[]> {
    const response = await fetch(this.forecastBundlesUrl, { credentials: "same-origin", headers: { Accept: "application/json" } });
    if (!response.ok) throw new Error(`Forecast bundle registry failed with HTTP ${response.status}`);
    const body = await response.json();
    return validateForecastBundles(body);
  }

  async query(request: TimeSeriesRequest): Promise<TimeSeriesWindow> {
    const response = await fetch(this.queryUrl, {
      method: "POST",
      credentials: "same-origin",
      headers: { Accept: "application/json", "Content-Type": "application/json" },
      body: JSON.stringify(request),
    });
    if (!response.ok) throw new Error(`Time-series query failed with HTTP ${response.status}`);
    return validateWindow(await response.json());
  }
}

export function defaultTimelineWorkspace(catalog: readonly TimeSeriesDescriptor[]): TimelineWorkspace {
  return {
    schemaVersion: 2,
    transform: "raw",
    lanes: catalog.filter((series) => series.defaultVisible).map((series) => ({ id: `lane-${series.id}`, seriesIds: [series.id] })),
    windowMinutes: 60,
    forecastBundleIds: [],
  };
}

export function createTimeSeriesPort(): TimeSeriesPort {
  const config = typeof window !== "undefined" ? window.__HELIOS_OPERATIONS__ : undefined;
  if (config?.timeSeriesCatalogUrl && config.forecastBundlesUrl && config.timeSeriesQueryUrl) {
    return new HttpTimeSeriesPort({ catalogUrl: config.timeSeriesCatalogUrl, forecastBundlesUrl: config.forecastBundlesUrl, queryUrl: config.timeSeriesQueryUrl });
  }
  return new DemoTimeSeriesPort();
}
