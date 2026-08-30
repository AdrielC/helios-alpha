export type FeedMode = "demo" | "shadow" | "paper" | "live";
export type HealthState = "healthy" | "degraded" | "stale";
export type SignalState = "observing" | "eligible" | "blocked";
export type OrderState = "open" | "partially_filled" | "pending_reconciliation";

export interface SignalPoint {
  readonly offsetSeconds: number;
  readonly valueBps: number;
}

export interface SignalView {
  readonly id: string;
  readonly hypothesis: string;
  readonly instrument: string;
  readonly state: SignalState;
  readonly posteriorBps: number;
  readonly trigger: string;
  readonly horizon: string;
  readonly observedAt: string;
  readonly availableAt: string;
  readonly decisionCut: string;
  readonly action: string;
  readonly blocker?: string;
  readonly lineage: readonly string[];
  readonly trace: readonly SignalPoint[];
}

export interface PositionView {
  readonly instrument: string;
  readonly strategy: string;
  readonly quantityMicros: string;
  readonly averagePriceMicros: string;
  readonly markPriceMicros: string;
  readonly marketValueMicros: string;
  readonly unrealizedPnlMicros: string;
  readonly currency: "USD";
  readonly freshnessMs: number;
}

export interface OrderView {
  readonly clientOrderId: string;
  readonly instrument: string;
  readonly side: "buy" | "sell";
  readonly state: OrderState;
  readonly quantityMicros: string;
  readonly filledQuantityMicros: string;
  readonly limitPriceMicros: string;
  readonly averagePriceMicros?: string;
  readonly venue: string;
  readonly strategy: string;
  readonly submittedAt: string;
  readonly reconciliation: "matched" | "pending";
}

export interface FillView {
  readonly executionId: string;
  readonly clientOrderId: string;
  readonly instrument: string;
  readonly side: "buy" | "sell";
  readonly quantityMicros: string;
  readonly priceMicros: string;
  readonly venue: string;
  readonly strategy: string;
  readonly executedAt: string;
  readonly liquidity: "maker" | "taker" | "unknown";
}

export interface SourceView {
  readonly name: string;
  readonly channel: string;
  readonly health: HealthState;
  readonly lagMs: number;
  readonly watermark: string;
  readonly detail: string;
}

export interface RiskView {
  readonly grossExposureMicros: string;
  readonly grossLimitMicros: string;
  readonly reservedGrossMicros: string;
  readonly dailyOrderCount: number;
  readonly dailyOrderLimit: number;
  readonly pendingReconciliations: number;
  readonly openIncidents: number;
  readonly killSwitchActive: boolean;
  readonly capitalGate: "closed" | "authorized";
  readonly capitalGateReason: string;
  readonly checkpointAgeMs: number;
  readonly sourceLagMs: number;
  readonly clockOffsetMs: number;
}

export interface OperationsSnapshot {
  readonly schemaVersion: 1;
  readonly sequence: number;
  readonly mode: FeedMode;
  readonly provider: string;
  readonly observedAt: string;
  readonly dataClass: "synthetic" | "observed";
  readonly accountLabel: string;
  readonly signals: readonly SignalView[];
  readonly positions: readonly PositionView[];
  readonly orders: readonly OrderView[];
  readonly fills: readonly FillView[];
  readonly sources: readonly SourceView[];
  readonly risk: RiskView;
}

export type SnapshotListener = (snapshot: OperationsSnapshot) => void;
export type PortStatus = "streaming" | "reconnecting" | "error";
export type StatusListener = (status: PortStatus) => void;

/**
 * Read-only operator data boundary. Mutation authority remains outside this port.
 * A separate, authenticated command service should own cancel, flatten, and kill-switch actions.
 */
export interface OperationsPort {
  readonly name: string;
  readonly supportsStreaming: boolean;
  load(): Promise<OperationsSnapshot>;
  subscribe(listener: SnapshotListener, onStatus?: StatusListener): () => void;
  close(): void;
}

const initialObservedAt = "2026-08-30T15:42:18.420Z";

export const initialOperationsSnapshot: OperationsSnapshot = {
  schemaVersion: 1,
  sequence: 184_512,
  mode: "shadow",
  provider: "DemoOperationsPort",
  observedAt: initialObservedAt,
  dataClass: "synthetic",
  accountLabel: "SPACE-WEATHER / SHADOW",
  signals: [
    {
      id: "cme-arrival-btc-01",
      hypothesis: "CME shock to crypto liquidity",
      instrument: "BTC-USD",
      state: "eligible",
      posteriorBps: 8_720,
      trigger: "GOES proton flux > P99.5",
      horizon: "45m",
      observedAt: "15:41:55.006Z",
      availableAt: "15:41:55.311Z",
      decisionCut: "15:42:00.000Z",
      action: "Reduce long exposure 18%",
      lineage: ["GOES-R", "event-time reorder", "10m bucket", "Hawkes + posterior", "risk"],
      trace: [
        { offsetSeconds: -60, valueBps: 2_420 },
        { offsetSeconds: -45, valueBps: 2_520 },
        { offsetSeconds: -30, valueBps: 2_680 },
        { offsetSeconds: -15, valueBps: 3_120 },
        { offsetSeconds: 0, valueBps: 8_720 },
        { offsetSeconds: 15, valueBps: 7_940 },
        { offsetSeconds: 30, valueBps: 7_120 },
        { offsetSeconds: 45, valueBps: 6_840 },
        { offsetSeconds: 60, valueBps: 6_510 },
      ],
    },
    {
      id: "kp-grid-semis-02",
      hypothesis: "Geomagnetic stress to semiconductor basket",
      instrument: "SMH",
      state: "observing",
      posteriorBps: 6_340,
      trigger: "Kp nowcast >= 7",
      horizon: "1d",
      observedAt: "15:40:03.881Z",
      availableAt: "15:40:05.044Z",
      decisionCut: "15:45:00.000Z",
      action: "Await second source",
      blocker: "DSCOVR plasma confirmation missing",
      lineage: ["SWPC Kp", "calendar", "event join", "posterior", "evidence gate"],
      trace: [
        { offsetSeconds: -60, valueBps: 3_100 },
        { offsetSeconds: -45, valueBps: 3_220 },
        { offsetSeconds: -30, valueBps: 3_420 },
        { offsetSeconds: -15, valueBps: 3_880 },
        { offsetSeconds: 0, valueBps: 6_340 },
        { offsetSeconds: 15, valueBps: 6_120 },
        { offsetSeconds: 30, valueBps: 5_920 },
        { offsetSeconds: 45, valueBps: 5_610 },
        { offsetSeconds: 60, valueBps: 5_380 },
      ],
    },
    {
      id: "solar-wind-energy-03",
      hypothesis: "Solar-wind impulse to power volatility",
      instrument: "VIX",
      state: "blocked",
      posteriorBps: 7_810,
      trigger: "Bz southward < -12 nT",
      horizon: "4h",
      observedAt: "15:37:17.201Z",
      availableAt: "15:37:19.925Z",
      decisionCut: "15:40:00.000Z",
      action: "No order authority",
      blocker: "Capacity model expired",
      lineage: ["DSCOVR", "L1 propagation", "weather join", "posterior", "capital gate"],
      trace: [
        { offsetSeconds: -60, valueBps: 4_220 },
        { offsetSeconds: -45, valueBps: 4_310 },
        { offsetSeconds: -30, valueBps: 4_480 },
        { offsetSeconds: -15, valueBps: 5_260 },
        { offsetSeconds: 0, valueBps: 7_810 },
        { offsetSeconds: 15, valueBps: 7_420 },
        { offsetSeconds: 30, valueBps: 6_880 },
        { offsetSeconds: 45, valueBps: 6_220 },
        { offsetSeconds: 60, valueBps: 5_920 },
      ],
    },
  ],
  positions: [
    {
      instrument: "BTC-USD",
      strategy: "cme-liquidity-v3",
      quantityMicros: "320000",
      averagePriceMicros: "63742120000",
      markPriceMicros: "64118250000",
      marketValueMicros: "20517840000",
      unrealizedPnlMicros: "120361600",
      currency: "USD",
      freshnessMs: 184,
    },
    {
      instrument: "SMH",
      strategy: "geomagnetic-semis-v2",
      quantityMicros: "18000000",
      averagePriceMicros: "271420000",
      markPriceMicros: "269880000",
      marketValueMicros: "4857840000",
      unrealizedPnlMicros: "-27720000",
      currency: "USD",
      freshnessMs: 238,
    },
    {
      instrument: "VIXY",
      strategy: "solar-wind-vol-v1",
      quantityMicros: "54000000",
      averagePriceMicros: "10980000",
      markPriceMicros: "11140000",
      marketValueMicros: "601560000",
      unrealizedPnlMicros: "8640000",
      currency: "USD",
      freshnessMs: 311,
    },
  ],
  orders: [
    {
      clientOrderId: "f8bcdd0f-650f-4a1e-b9de-a8137d5872ac",
      instrument: "BTC-USD",
      side: "sell",
      state: "partially_filled",
      quantityMicros: "58000",
      filledQuantityMicros: "34000",
      limitPriceMicros: "64095000000",
      averagePriceMicros: "64104080000",
      venue: "SIM-RH-CRYPTO",
      strategy: "cme-liquidity-v3",
      submittedAt: "15:42:02.118Z",
      reconciliation: "matched",
    },
    {
      clientOrderId: "55d30390-9052-4ab6-9f49-071961e80a13",
      instrument: "SMH",
      side: "buy",
      state: "open",
      quantityMicros: "6000000",
      filledQuantityMicros: "0",
      limitPriceMicros: "268750000",
      venue: "SIM-XNYS",
      strategy: "geomagnetic-semis-v2",
      submittedAt: "15:40:12.499Z",
      reconciliation: "matched",
    },
  ],
  fills: [
    {
      executionId: "exec-9f47f6f1",
      clientOrderId: "f8bcdd0f-650f-4a1e-b9de-a8137d5872ac",
      instrument: "BTC-USD",
      side: "sell",
      quantityMicros: "34000",
      priceMicros: "64104080000",
      venue: "SIM-RH-CRYPTO",
      strategy: "cme-liquidity-v3",
      executedAt: "15:42:04.881Z",
      liquidity: "taker",
    },
    {
      executionId: "exec-203e37bc",
      clientOrderId: "3c5a2b4d-14cd-4e3b-b315-45642a580c90",
      instrument: "VIXY",
      side: "buy",
      quantityMicros: "12000000",
      priceMicros: "11120000",
      venue: "SIM-XNYS",
      strategy: "solar-wind-vol-v1",
      executedAt: "15:37:31.118Z",
      liquidity: "maker",
    },
  ],
  sources: [
    {
      name: "GOES-R",
      channel: "proton-flux",
      health: "healthy",
      lagMs: 184,
      watermark: "15:42:18.236Z",
      detail: "event time ordered",
    },
    {
      name: "DSCOVR",
      channel: "solar-wind-plasma",
      health: "degraded",
      lagMs: 4_820,
      watermark: "15:42:13.600Z",
      detail: "one packet gap",
    },
    {
      name: "SWPC",
      channel: "kp-nowcast",
      health: "healthy",
      lagMs: 1_163,
      watermark: "15:42:17.257Z",
      detail: "forecast revision 31",
    },
    {
      name: "MARKET",
      channel: "consolidated-marks",
      health: "healthy",
      lagMs: 238,
      watermark: "15:42:18.182Z",
      detail: "3 instruments current",
    },
  ],
  risk: {
    grossExposureMicros: "25977240000",
    grossLimitMicros: "50000000000",
    reservedGrossMicros: "3496690000",
    dailyOrderCount: 7,
    dailyOrderLimit: 20,
    pendingReconciliations: 0,
    openIncidents: 0,
    killSwitchActive: false,
    capitalGate: "closed",
    capitalGateReason: "Shadow evidence window incomplete",
    checkpointAgeMs: 642,
    sourceLagMs: 4_820,
    clockOffsetMs: 3,
  },
};

function cloneSnapshot(snapshot: OperationsSnapshot): OperationsSnapshot {
  return JSON.parse(JSON.stringify(snapshot)) as OperationsSnapshot;
}

export class DemoOperationsPort implements OperationsPort {
  readonly name = "DemoOperationsPort";
  readonly supportsStreaming = true;
  private sequence = initialOperationsSnapshot.sequence;
  private listeners = new Set<SnapshotListener>();
  private timer: ReturnType<typeof setInterval> | undefined;
  private tick = 0;

  async load(): Promise<OperationsSnapshot> {
    return this.nextSnapshot();
  }

  subscribe(listener: SnapshotListener, onStatus?: StatusListener): () => void {
    this.listeners.add(listener);
    onStatus?.("streaming");
    if (!this.timer) {
      this.timer = setInterval(() => {
        const snapshot = this.nextSnapshot();
        for (const current of this.listeners) current(snapshot);
      }, 1_600);
    }
    return () => {
      this.listeners.delete(listener);
      if (this.listeners.size === 0) this.close();
    };
  }

  close(): void {
    if (this.timer) clearInterval(this.timer);
    this.timer = undefined;
  }

  private nextSnapshot(): OperationsSnapshot {
    const snapshot = cloneSnapshot(initialOperationsSnapshot);
    const observedAt = Date.now();
    const cycle = [0, 11_000_000, -7_000_000, 18_000_000, 4_000_000];
    const delta = cycle[this.tick % cycle.length];
    this.tick += 1;
    this.sequence += 1;
    const positions = snapshot.positions.map((position, index) => {
      if (index !== 0) return position;
      const mark = BigInt(position.markPriceMicros) + BigInt(delta);
      const quantity = BigInt(position.quantityMicros);
      const average = BigInt(position.averagePriceMicros);
      return {
        ...position,
        markPriceMicros: mark.toString(),
        marketValueMicros: ((mark * quantity) / 1_000_000n).toString(),
        unrealizedPnlMicros: (((mark - average) * quantity) / 1_000_000n).toString(),
        freshnessMs: 120 + (this.tick % 4) * 37,
      };
    });
    return {
      ...snapshot,
      sequence: this.sequence,
      observedAt: new Date(observedAt).toISOString(),
      positions,
      sources: snapshot.sources.map((source) => ({
        ...source,
        watermark: new Date(observedAt - source.lagMs).toISOString().slice(11, 23),
      })),
    };
  }
}

interface HttpPortOptions {
  readonly snapshotUrl: string;
  readonly streamUrl?: string;
}

export class HttpOperationsPort implements OperationsPort {
  readonly name = "HttpOperationsPort";
  private source: EventSource | undefined;
  private readonly options: HttpPortOptions;

  constructor(options: HttpPortOptions) {
    const sameOrigin = (candidate: string, field: string): string => {
      const url = new URL(candidate, window.location.href);
      if (url.origin !== window.location.origin) throw new Error(`${field} must be same-origin`);
      return url.href;
    };
    this.options = {
      snapshotUrl: sameOrigin(options.snapshotUrl, "snapshotUrl"),
      streamUrl: options.streamUrl ? sameOrigin(options.streamUrl, "streamUrl") : undefined,
    };
  }

  get supportsStreaming(): boolean {
    return Boolean(this.options.streamUrl && typeof EventSource !== "undefined");
  }

  async load(): Promise<OperationsSnapshot> {
    const response = await fetch(this.options.snapshotUrl, {
      credentials: "same-origin",
      headers: { Accept: "application/json" },
    });
    if (!response.ok) throw new Error(`Operations snapshot failed with HTTP ${response.status}`);
    return validateSnapshot(await response.json());
  }

  subscribe(listener: SnapshotListener, onStatus?: StatusListener): () => void {
    if (!this.options.streamUrl || typeof EventSource === "undefined") return () => undefined;
    this.source = new EventSource(this.options.streamUrl, { withCredentials: true });
    this.source.addEventListener("open", () => onStatus?.("streaming"));
    this.source.addEventListener("error", () => onStatus?.("reconnecting"));
    this.source.addEventListener("snapshot", (event) => {
      if (!(event instanceof MessageEvent)) return;
      try {
        listener(validateSnapshot(JSON.parse(String(event.data))));
      } catch (error) {
        console.error("Rejected malformed operations snapshot", error);
        onStatus?.("error");
      }
    });
    return () => this.close();
  }

  close(): void {
    this.source?.close();
    this.source = undefined;
  }
}

function validateSnapshot(value: unknown): OperationsSnapshot {
  const fail = (path: string): never => {
    throw new Error(`Invalid operations snapshot at ${path}`);
  };
  const record = (candidate: unknown, path: string): Record<string, unknown> => {
    if (!candidate || typeof candidate !== "object" || Array.isArray(candidate)) return fail(path);
    return candidate as Record<string, unknown>;
  };
  const text = (candidate: unknown, path: string): string =>
    typeof candidate === "string" && candidate.length > 0 ? candidate : fail(path);
  const integer = (candidate: unknown, path: string): number =>
    typeof candidate === "number" && Number.isSafeInteger(candidate) && candidate >= 0 ? candidate : fail(path);
  const finite = (candidate: unknown, path: string): number =>
    typeof candidate === "number" && Number.isFinite(candidate) ? candidate : fail(path);
  const oneOf = <T extends string>(candidate: unknown, options: readonly T[], path: string): T =>
    typeof candidate === "string" && options.includes(candidate as T) ? (candidate as T) : fail(path);
  const micros = (candidate: unknown, path: string): string =>
    typeof candidate === "string" && /^-?\d+$/.test(candidate) ? candidate : fail(path);
  const list = (candidate: unknown, path: string): unknown[] =>
    Array.isArray(candidate) ? candidate : fail(path);

  const snapshot = record(value, "root");
  if (snapshot.schemaVersion !== 1) fail("schemaVersion");
  integer(snapshot.sequence, "sequence");
  oneOf(snapshot.mode, ["demo", "shadow", "paper", "live"], "mode");
  text(snapshot.provider, "provider");
  const observedAt = text(snapshot.observedAt, "observedAt");
  if (!Number.isFinite(Date.parse(observedAt))) fail("observedAt");
  oneOf(snapshot.dataClass, ["synthetic", "observed"], "dataClass");
  text(snapshot.accountLabel, "accountLabel");

  for (const [index, candidate] of list(snapshot.signals, "signals").entries()) {
    const signal = record(candidate, `signals[${index}]`);
    for (const field of ["id", "hypothesis", "instrument", "trigger", "horizon", "observedAt", "availableAt", "decisionCut", "action"] as const) {
      text(signal[field], `signals[${index}].${field}`);
    }
    oneOf(signal.state, ["observing", "eligible", "blocked"], `signals[${index}].state`);
    const posterior = finite(signal.posteriorBps, `signals[${index}].posteriorBps`);
    if (posterior < 0 || posterior > 10_000) fail(`signals[${index}].posteriorBps`);
    if (signal.blocker !== undefined) text(signal.blocker, `signals[${index}].blocker`);
    list(signal.lineage, `signals[${index}].lineage`).forEach((step, stepIndex) =>
      text(step, `signals[${index}].lineage[${stepIndex}]`),
    );
    list(signal.trace, `signals[${index}].trace`).forEach((point, pointIndex) => {
      const tracePoint = record(point, `signals[${index}].trace[${pointIndex}]`);
      finite(tracePoint.offsetSeconds, `signals[${index}].trace[${pointIndex}].offsetSeconds`);
      finite(tracePoint.valueBps, `signals[${index}].trace[${pointIndex}].valueBps`);
    });
  }

  for (const [index, candidate] of list(snapshot.positions, "positions").entries()) {
    const position = record(candidate, `positions[${index}]`);
    text(position.instrument, `positions[${index}].instrument`);
    text(position.strategy, `positions[${index}].strategy`);
    for (const field of ["quantityMicros", "averagePriceMicros", "markPriceMicros", "marketValueMicros", "unrealizedPnlMicros"] as const) {
      micros(position[field], `positions[${index}].${field}`);
    }
    if (position.currency !== "USD") fail(`positions[${index}].currency`);
    integer(position.freshnessMs, `positions[${index}].freshnessMs`);
  }

  for (const [index, candidate] of list(snapshot.orders, "orders").entries()) {
    const order = record(candidate, `orders[${index}]`);
    for (const field of ["clientOrderId", "instrument", "venue", "strategy", "submittedAt"] as const) {
      text(order[field], `orders[${index}].${field}`);
    }
    oneOf(order.side, ["buy", "sell"], `orders[${index}].side`);
    oneOf(order.state, ["open", "partially_filled", "pending_reconciliation"], `orders[${index}].state`);
    oneOf(order.reconciliation, ["matched", "pending"], `orders[${index}].reconciliation`);
    for (const field of ["quantityMicros", "filledQuantityMicros", "limitPriceMicros"] as const) {
      micros(order[field], `orders[${index}].${field}`);
    }
    if (order.averagePriceMicros !== undefined) micros(order.averagePriceMicros, `orders[${index}].averagePriceMicros`);
  }

  for (const [index, candidate] of list(snapshot.fills, "fills").entries()) {
    const fill = record(candidate, `fills[${index}]`);
    for (const field of ["executionId", "clientOrderId", "instrument", "venue", "strategy", "executedAt"] as const) {
      text(fill[field], `fills[${index}].${field}`);
    }
    oneOf(fill.side, ["buy", "sell"], `fills[${index}].side`);
    oneOf(fill.liquidity, ["maker", "taker", "unknown"], `fills[${index}].liquidity`);
    micros(fill.quantityMicros, `fills[${index}].quantityMicros`);
    micros(fill.priceMicros, `fills[${index}].priceMicros`);
  }

  for (const [index, candidate] of list(snapshot.sources, "sources").entries()) {
    const source = record(candidate, `sources[${index}]`);
    for (const field of ["name", "channel", "watermark", "detail"] as const) {
      text(source[field], `sources[${index}].${field}`);
    }
    oneOf(source.health, ["healthy", "degraded", "stale"], `sources[${index}].health`);
    integer(source.lagMs, `sources[${index}].lagMs`);
  }

  const risk = record(snapshot.risk, "risk");
  for (const field of ["grossExposureMicros", "grossLimitMicros", "reservedGrossMicros"] as const) {
    micros(risk[field], `risk.${field}`);
  }
  for (const field of ["dailyOrderCount", "dailyOrderLimit", "pendingReconciliations", "openIncidents", "checkpointAgeMs", "sourceLagMs"] as const) {
    integer(risk[field], `risk.${field}`);
  }
  finite(risk.clockOffsetMs, "risk.clockOffsetMs");
  if (typeof risk.killSwitchActive !== "boolean") fail("risk.killSwitchActive");
  oneOf(risk.capitalGate, ["closed", "authorized"], "risk.capitalGate");
  text(risk.capitalGateReason, "risk.capitalGateReason");

  return snapshot as unknown as OperationsSnapshot;
}

declare global {
  interface Window {
    __HELIOS_OPERATIONS__?: HttpPortOptions;
  }
}

export function createOperationsPort(): OperationsPort {
  if (typeof window !== "undefined" && window.__HELIOS_OPERATIONS__?.snapshotUrl) {
    return new HttpOperationsPort(window.__HELIOS_OPERATIONS__);
  }
  return new DemoOperationsPort();
}
