export type FeedMode = "demo" | "shadow" | "paper" | "live";
export type HealthState = "healthy" | "degraded" | "stale";
export type SignalState = "observing" | "eligible" | "blocked";
export type OrderState =
  | "pending_submit"
  | "working"
  | "partially_filled"
  | "pending_cancel"
  | "pending_replace"
  | "filled"
  | "canceled"
  | "rejected"
  | "expired"
  | "unknown";
export type StrategyState = "running" | "paused" | "blocked";
export type StageState = "running" | "paused" | "blocked" | "replaying";
export type AlertSeverity = "critical" | "warning" | "info";
export type AlertStatus = "open" | "acknowledged" | "resolved";

export interface OperationsContext {
  readonly organizationId: string;
  readonly organizationName: string;
  readonly workspaceId: string;
  readonly workspaceName: string;
  readonly accountId: string;
  readonly accountName: string;
}

export interface SignalPoint {
  readonly offsetSeconds: number;
  readonly valueBps: number;
}

export interface SignalView {
  readonly id: string;
  readonly strategyId: string;
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
  readonly dayPnlMicros?: string;
  readonly dayChangeBps?: number;
  readonly currency: "USD";
  readonly freshnessMs: number;
}

export interface OrderView {
  readonly clientOrderId: string;
  readonly brokerOrderId?: string;
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
  readonly omsVersion?: number;
  readonly timeInForce?: "day" | "good_till_canceled" | "immediate_or_cancel" | "fill_or_kill";
  readonly uncertaintyReason?: string;
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

export interface StrategyView {
  readonly id: string;
  readonly name: string;
  readonly state: StrategyState;
  readonly generation: number;
  readonly activeSignalId?: string;
  readonly detail: string;
}

export interface StageView {
  readonly id: string;
  readonly name: string;
  readonly kind: string;
  readonly state: StageState;
  readonly lagMs: number;
  readonly checkpoint: string;
  readonly detail: string;
  readonly canPauseBefore: boolean;
}

export interface AlertView {
  readonly id: string;
  readonly severity: AlertSeverity;
  readonly status: AlertStatus;
  readonly category: string;
  readonly title: string;
  readonly detail: string;
  readonly openedAt: string;
  readonly updatedAt: string;
  readonly relatedEntity?: {
    readonly kind: string;
    readonly id: string;
    readonly label: string;
  };
}

export interface MetricPoint {
  readonly timestamp: string;
  readonly value: number;
}

export interface ReferenceLine {
  readonly label: string;
  readonly value: number;
  readonly tone: "neutral" | "warning" | "critical";
}

export interface MetricSeriesView {
  readonly id: string;
  readonly label: string;
  readonly unit: "USD" | "%" | "ms" | "count";
  readonly tone: "cyan" | "green" | "coral";
  readonly points: readonly MetricPoint[];
  readonly referenceLines: readonly ReferenceLine[];
}

export interface ActivityView {
  readonly id: string;
  readonly sequence: number;
  readonly occurredAt: string;
  readonly category: string;
  readonly source: string;
  readonly stage: string;
  readonly entity: string;
  readonly outcome: string;
  readonly severity: "normal" | "warning" | "critical";
}

export interface OperationsSnapshot {
  readonly schemaVersion: 2;
  readonly sequence: number;
  readonly mode: FeedMode;
  readonly provider: string;
  readonly observedAt: string;
  readonly dataClass: "synthetic" | "observed";
  readonly context: OperationsContext;
  readonly strategies: readonly StrategyView[];
  readonly stages: readonly StageView[];
  readonly signals: readonly SignalView[];
  readonly positions: readonly PositionView[];
  readonly orders: readonly OrderView[];
  readonly fills: readonly FillView[];
  readonly sources: readonly SourceView[];
  readonly alerts: readonly AlertView[];
  readonly metrics: readonly MetricSeriesView[];
  readonly activity: readonly ActivityView[];
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
  schemaVersion: 2,
  sequence: 184_512,
  mode: "shadow",
  provider: "DemoOperationsPort",
  observedAt: initialObservedAt,
  dataClass: "synthetic",
  context: {
    organizationId: "northstar-research",
    organizationName: "Northstar Research",
    workspaceId: "event-strategies",
    workspaceName: "Event Strategies",
    accountId: "shadow-01",
    accountName: "Shadow 01",
  },
  strategies: [
    {
      id: "cme-liquidity-v3",
      name: "CME liquidity response",
      state: "running",
      generation: 31,
      activeSignalId: "cme-arrival-btc-01",
      detail: "Shadow decisions only",
    },
    {
      id: "geomagnetic-semis-v2",
      name: "Geomagnetic semiconductor stress",
      state: "running",
      generation: 12,
      activeSignalId: "kp-grid-semis-02",
      detail: "Waiting for source agreement",
    },
    {
      id: "solar-wind-vol-v1",
      name: "Solar-wind volatility response",
      state: "blocked",
      generation: 8,
      activeSignalId: "solar-wind-energy-03",
      detail: "Capacity evidence expired",
    },
  ],
  stages: [
    {
      id: "source-fence",
      name: "Source fence",
      kind: "source",
      state: "running",
      lagMs: 184,
      checkpoint: "src:184512",
      detail: "Backfill and live tail joined",
      canPauseBefore: false,
    },
    {
      id: "event-order",
      name: "Event-time order",
      kind: "ordering",
      state: "running",
      lagMs: 311,
      checkpoint: "ord:184508",
      detail: "2s watermark, one declared gap",
      canPauseBefore: true,
    },
    {
      id: "feature-state",
      name: "Feature state",
      kind: "feature",
      state: "running",
      lagMs: 422,
      checkpoint: "feat:184505",
      detail: "10m buckets and stable moments",
      canPauseBefore: true,
    },
    {
      id: "hypothesis-update",
      name: "Hypothesis update",
      kind: "hypothesis",
      state: "running",
      lagMs: 588,
      checkpoint: "hyp:184501",
      detail: "Point-in-time posterior update",
      canPauseBefore: true,
    },
    {
      id: "risk-admission",
      name: "Risk admission",
      kind: "risk",
      state: "blocked",
      lagMs: 642,
      checkpoint: "risk:184498",
      detail: "Capital gate closed",
      canPauseBefore: true,
    },
    {
      id: "execution-router",
      name: "Execution router",
      kind: "execution",
      state: "paused",
      lagMs: 642,
      checkpoint: "exec:184498",
      detail: "No live order authority",
      canPauseBefore: true,
    },
  ],
  signals: [
    {
      id: "cme-arrival-btc-01",
      strategyId: "cme-liquidity-v3",
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
      strategyId: "geomagnetic-semis-v2",
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
      strategyId: "solar-wind-vol-v1",
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
      dayPnlMicros: "73441600",
      dayChangeBps: 36,
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
      dayPnlMicros: "-42120000",
      dayChangeBps: -86,
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
      dayPnlMicros: "12420000",
      dayChangeBps: 211,
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
      state: "working",
      quantityMicros: "6000000",
      filledQuantityMicros: "0",
      limitPriceMicros: "268750000",
      venue: "SIM-XNYS",
      strategy: "geomagnetic-semis-v2",
      submittedAt: "15:40:12.499Z",
      reconciliation: "matched",
      brokerOrderId: "sim-xnys-88412",
      omsVersion: 2,
      timeInForce: "day",
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
  alerts: [
    {
      id: "alert-source-gap",
      severity: "warning",
      status: "open",
      category: "market data",
      title: "Packet gap on DSCOVR",
      detail: "The plasma channel is 4.82 seconds behind its active watermark.",
      openedAt: "2026-08-30T15:41:58.600Z",
      updatedAt: "2026-08-30T15:42:13.600Z",
      relatedEntity: { kind: "source", id: "DSCOVR:solar-wind-plasma", label: "DSCOVR / solar-wind-plasma" },
    },
    {
      id: "alert-capital-gate",
      severity: "info",
      status: "open",
      category: "risk",
      title: "Capital admission closed",
      detail: "The account remains in shadow mode until its evidence window completes.",
      openedAt: "2026-08-30T14:00:00.000Z",
      updatedAt: "2026-08-30T15:42:18.420Z",
      relatedEntity: { kind: "account", id: "shadow-01", label: "Shadow 01" },
    },
    {
      id: "alert-capacity-model",
      severity: "warning",
      status: "acknowledged",
      category: "strategy",
      title: "Capacity evidence expired",
      detail: "The strategy is blocked pending a refreshed market-impact estimate.",
      openedAt: "2026-08-30T15:37:20.000Z",
      updatedAt: "2026-08-30T15:40:02.000Z",
      relatedEntity: { kind: "strategy", id: "solar-wind-vol-v1", label: "Solar-wind volatility response" },
    },
  ],
  metrics: [
    {
      id: "gross-exposure",
      label: "Gross exposure",
      unit: "USD",
      tone: "cyan",
      points: [
        { timestamp: "2026-08-30T15:36:48.420Z", value: 22_420 },
        { timestamp: "2026-08-30T15:37:18.420Z", value: 22_910 },
        { timestamp: "2026-08-30T15:37:48.420Z", value: 23_080 },
        { timestamp: "2026-08-30T15:38:18.420Z", value: 23_840 },
        { timestamp: "2026-08-30T15:38:48.420Z", value: 24_110 },
        { timestamp: "2026-08-30T15:39:18.420Z", value: 24_820 },
        { timestamp: "2026-08-30T15:39:48.420Z", value: 25_180 },
        { timestamp: "2026-08-30T15:40:18.420Z", value: 25_240 },
        { timestamp: "2026-08-30T15:40:48.420Z", value: 25_610 },
        { timestamp: "2026-08-30T15:41:18.420Z", value: 25_820 },
        { timestamp: "2026-08-30T15:41:48.420Z", value: 25_930 },
        { timestamp: "2026-08-30T15:42:18.420Z", value: 25_977.24 },
      ],
      referenceLines: [{ label: "Gross limit", value: 50_000, tone: "warning" }],
    },
    {
      id: "unrealized-pnl",
      label: "Unrealized P&L",
      unit: "USD",
      tone: "green",
      points: [
        { timestamp: "2026-08-30T15:36:48.420Z", value: 42.6 },
        { timestamp: "2026-08-30T15:37:18.420Z", value: 51.4 },
        { timestamp: "2026-08-30T15:37:48.420Z", value: 48.2 },
        { timestamp: "2026-08-30T15:38:18.420Z", value: 62.1 },
        { timestamp: "2026-08-30T15:38:48.420Z", value: 74.6 },
        { timestamp: "2026-08-30T15:39:18.420Z", value: 68.9 },
        { timestamp: "2026-08-30T15:39:48.420Z", value: 79.2 },
        { timestamp: "2026-08-30T15:40:18.420Z", value: 71.8 },
        { timestamp: "2026-08-30T15:40:48.420Z", value: 83.1 },
        { timestamp: "2026-08-30T15:41:18.420Z", value: 92.4 },
        { timestamp: "2026-08-30T15:41:48.420Z", value: 98.3 },
        { timestamp: "2026-08-30T15:42:18.420Z", value: 101.2816 },
      ],
      referenceLines: [{ label: "Flat", value: 0, tone: "neutral" }],
    },
    {
      id: "source-lag",
      label: "Worst source lag",
      unit: "ms",
      tone: "coral",
      points: [
        { timestamp: "2026-08-30T15:36:48.420Z", value: 880 },
        { timestamp: "2026-08-30T15:37:18.420Z", value: 1_120 },
        { timestamp: "2026-08-30T15:37:48.420Z", value: 940 },
        { timestamp: "2026-08-30T15:38:18.420Z", value: 1_380 },
        { timestamp: "2026-08-30T15:38:48.420Z", value: 1_640 },
        { timestamp: "2026-08-30T15:39:18.420Z", value: 1_920 },
        { timestamp: "2026-08-30T15:39:48.420Z", value: 2_460 },
        { timestamp: "2026-08-30T15:40:18.420Z", value: 3_180 },
        { timestamp: "2026-08-30T15:40:48.420Z", value: 3_620 },
        { timestamp: "2026-08-30T15:41:18.420Z", value: 4_110 },
        { timestamp: "2026-08-30T15:41:48.420Z", value: 4_540 },
        { timestamp: "2026-08-30T15:42:18.420Z", value: 4_820 },
      ],
      referenceLines: [{ label: "SLO", value: 2_000, tone: "warning" }],
    },
  ],
  activity: [
    { id: "act-184512", sequence: 184_512, occurredAt: "15:42:18.420Z", category: "order", source: "risk", stage: "execution-router", entity: "BTC-USD", outcome: "partial fill", severity: "normal" },
    { id: "act-184511", sequence: 184_511, occurredAt: "15:42:17.980Z", category: "signal", source: "cme-arrival-btc-01", stage: "risk-admission", entity: "BTC-USD", outcome: "eligible", severity: "normal" },
    { id: "act-184510", sequence: 184_510, occurredAt: "15:42:15.600Z", category: "source", source: "DSCOVR", stage: "event-order", entity: "solar-wind-plasma", outcome: "packet gap", severity: "warning" },
    { id: "act-184509", sequence: 184_509, occurredAt: "15:42:13.902Z", category: "checkpoint", source: "feature-state", stage: "feature-state", entity: "generation 31", outcome: "committed", severity: "normal" },
    { id: "act-184508", sequence: 184_508, occurredAt: "15:42:11.102Z", category: "mark", source: "MARKET", stage: "source-fence", entity: "SMH", outcome: "updated", severity: "normal" },
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
    const unrealized = positions.reduce(
      (total, position) => total + Number(BigInt(position.unrealizedPnlMicros)) / 1_000_000,
      0,
    );
    const metricValues: Record<string, number> = {
      "gross-exposure": Number(BigInt(snapshot.risk.grossExposureMicros)) / 1_000_000 + delta / 1_000_000,
      "unrealized-pnl": unrealized,
      "source-lag": snapshot.risk.sourceLagMs + (this.tick % 3) * 36,
    };
    const metrics = snapshot.metrics.map((metric) => ({
      ...metric,
      points: metric.points.map((point, index, points) => ({
        timestamp: new Date(observedAt - (points.length - 1 - index) * 30_000).toISOString(),
        value: index === points.length - 1 ? metricValues[metric.id] ?? point.value : point.value,
      })),
    }));
    return {
      ...snapshot,
      sequence: this.sequence,
      observedAt: new Date(observedAt).toISOString(),
      positions,
      metrics,
      alerts: snapshot.alerts.map((alert) => {
        const ageMs = alert.id === "alert-source-gap" ? 15_000 : alert.id === "alert-capacity-model" ? 120_000 : 2_000;
        const openAgeMs = alert.id === "alert-source-gap" ? 35_000 : alert.id === "alert-capacity-model" ? 300_000 : 7_200_000;
        return {
          ...alert,
          openedAt: new Date(observedAt - openAgeMs).toISOString(),
          updatedAt: new Date(observedAt - ageMs).toISOString(),
        };
      }),
      activity: snapshot.activity.map((activity, index) =>
        index === 0
          ? {
              ...activity,
              id: `act-${this.sequence}`,
              sequence: this.sequence,
              occurredAt: new Date(observedAt).toISOString().slice(11, 23),
            }
          : activity,
      ),
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

export interface HeliosRuntimeConfig extends HttpPortOptions {
  readonly commandUrl?: string;
  readonly commandSessionUrl?: string;
  readonly timeSeriesCatalogUrl?: string;
  readonly forecastBundlesUrl?: string;
  readonly timeSeriesQueryUrl?: string;
  readonly investigationUrl?: string;
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
  if (snapshot.schemaVersion !== 2) fail("schemaVersion");
  integer(snapshot.sequence, "sequence");
  oneOf(snapshot.mode, ["demo", "shadow", "paper", "live"], "mode");
  text(snapshot.provider, "provider");
  const observedAt = text(snapshot.observedAt, "observedAt");
  if (!Number.isFinite(Date.parse(observedAt))) fail("observedAt");
  oneOf(snapshot.dataClass, ["synthetic", "observed"], "dataClass");
  const context = record(snapshot.context, "context");
  for (const field of ["organizationId", "organizationName", "workspaceId", "workspaceName", "accountId", "accountName"] as const) {
    text(context[field], `context.${field}`);
  }

  for (const [index, candidate] of list(snapshot.strategies, "strategies").entries()) {
    const strategy = record(candidate, `strategies[${index}]`);
    for (const field of ["id", "name", "detail"] as const) {
      text(strategy[field], `strategies[${index}].${field}`);
    }
    oneOf(strategy.state, ["running", "paused", "blocked"], `strategies[${index}].state`);
    integer(strategy.generation, `strategies[${index}].generation`);
    if (strategy.activeSignalId !== undefined) {
      text(strategy.activeSignalId, `strategies[${index}].activeSignalId`);
    }
  }

  for (const [index, candidate] of list(snapshot.stages, "stages").entries()) {
    const stage = record(candidate, `stages[${index}]`);
    for (const field of ["id", "name", "checkpoint", "detail"] as const) {
      text(stage[field], `stages[${index}].${field}`);
    }
    text(stage.kind, `stages[${index}].kind`);
    oneOf(
      stage.state,
      ["running", "paused", "blocked", "replaying"],
      `stages[${index}].state`,
    );
    integer(stage.lagMs, `stages[${index}].lagMs`);
    if (typeof stage.canPauseBefore !== "boolean") fail(`stages[${index}].canPauseBefore`);
  }

  for (const [index, candidate] of list(snapshot.signals, "signals").entries()) {
    const signal = record(candidate, `signals[${index}]`);
    for (const field of ["id", "strategyId", "hypothesis", "instrument", "trigger", "horizon", "observedAt", "availableAt", "decisionCut", "action"] as const) {
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
    if (position.dayPnlMicros !== undefined) micros(position.dayPnlMicros, `positions[${index}].dayPnlMicros`);
    if (position.dayChangeBps !== undefined) finite(position.dayChangeBps, `positions[${index}].dayChangeBps`);
    if (position.currency !== "USD") fail(`positions[${index}].currency`);
    integer(position.freshnessMs, `positions[${index}].freshnessMs`);
  }

  for (const [index, candidate] of list(snapshot.orders, "orders").entries()) {
    const order = record(candidate, `orders[${index}]`);
    for (const field of ["clientOrderId", "instrument", "venue", "strategy", "submittedAt"] as const) {
      text(order[field], `orders[${index}].${field}`);
    }
    oneOf(order.side, ["buy", "sell"], `orders[${index}].side`);
    oneOf(
      order.state,
      [
        "pending_submit",
        "working",
        "partially_filled",
        "pending_cancel",
        "pending_replace",
        "filled",
        "canceled",
        "rejected",
        "expired",
        "unknown",
      ],
      `orders[${index}].state`,
    );
    oneOf(order.reconciliation, ["matched", "pending"], `orders[${index}].reconciliation`);
    for (const field of ["quantityMicros", "filledQuantityMicros", "limitPriceMicros"] as const) {
      micros(order[field], `orders[${index}].${field}`);
    }
    if (order.averagePriceMicros !== undefined) micros(order.averagePriceMicros, `orders[${index}].averagePriceMicros`);
    if (order.brokerOrderId !== undefined) text(order.brokerOrderId, `orders[${index}].brokerOrderId`);
    if (order.omsVersion !== undefined) integer(order.omsVersion, `orders[${index}].omsVersion`);
    if (order.uncertaintyReason !== undefined) text(order.uncertaintyReason, `orders[${index}].uncertaintyReason`);
    if (order.timeInForce !== undefined) {
      oneOf(
        order.timeInForce,
        ["day", "good_till_canceled", "immediate_or_cancel", "fill_or_kill"],
        `orders[${index}].timeInForce`,
      );
    }
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

  for (const [index, candidate] of list(snapshot.alerts, "alerts").entries()) {
    const alert = record(candidate, `alerts[${index}]`);
    for (const field of ["id", "category", "title", "detail", "openedAt", "updatedAt"] as const) {
      text(alert[field], `alerts[${index}].${field}`);
    }
    oneOf(alert.severity, ["critical", "warning", "info"], `alerts[${index}].severity`);
    oneOf(alert.status, ["open", "acknowledged", "resolved"], `alerts[${index}].status`);
    if (!Number.isFinite(Date.parse(String(alert.openedAt)))) fail(`alerts[${index}].openedAt`);
    if (!Number.isFinite(Date.parse(String(alert.updatedAt)))) fail(`alerts[${index}].updatedAt`);
    if (alert.relatedEntity !== undefined) {
      const related = record(alert.relatedEntity, `alerts[${index}].relatedEntity`);
      for (const field of ["kind", "id", "label"] as const) {
        text(related[field], `alerts[${index}].relatedEntity.${field}`);
      }
    }
  }

  for (const [index, candidate] of list(snapshot.metrics, "metrics").entries()) {
    const metric = record(candidate, `metrics[${index}]`);
    for (const field of ["id", "label"] as const) text(metric[field], `metrics[${index}].${field}`);
    oneOf(metric.unit, ["USD", "%", "ms", "count"], `metrics[${index}].unit`);
    oneOf(metric.tone, ["cyan", "green", "coral"], `metrics[${index}].tone`);
    list(metric.points, `metrics[${index}].points`).forEach((candidatePoint, pointIndex) => {
      const point = record(candidatePoint, `metrics[${index}].points[${pointIndex}]`);
      const timestamp = text(point.timestamp, `metrics[${index}].points[${pointIndex}].timestamp`);
      if (!Number.isFinite(Date.parse(timestamp))) fail(`metrics[${index}].points[${pointIndex}].timestamp`);
      finite(point.value, `metrics[${index}].points[${pointIndex}].value`);
    });
    list(metric.referenceLines, `metrics[${index}].referenceLines`).forEach((candidateLine, lineIndex) => {
      const line = record(candidateLine, `metrics[${index}].referenceLines[${lineIndex}]`);
      text(line.label, `metrics[${index}].referenceLines[${lineIndex}].label`);
      finite(line.value, `metrics[${index}].referenceLines[${lineIndex}].value`);
      oneOf(line.tone, ["neutral", "warning", "critical"], `metrics[${index}].referenceLines[${lineIndex}].tone`);
    });
  }

  for (const [index, candidate] of list(snapshot.activity, "activity").entries()) {
    const activity = record(candidate, `activity[${index}]`);
    for (const field of ["id", "occurredAt", "category", "source", "stage", "entity", "outcome"] as const) {
      text(activity[field], `activity[${index}].${field}`);
    }
    integer(activity.sequence, `activity[${index}].sequence`);
    oneOf(activity.severity, ["normal", "warning", "critical"], `activity[${index}].severity`);
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
    __HELIOS_OPERATIONS__?: HeliosRuntimeConfig;
  }
}

export function createOperationsPort(): OperationsPort {
  if (typeof window !== "undefined" && window.__HELIOS_OPERATIONS__?.snapshotUrl) {
    return new HttpOperationsPort(window.__HELIOS_OPERATIONS__);
  }
  return new DemoOperationsPort();
}
