<script setup lang="ts">
import { computed, defineAsyncComponent, onBeforeUnmount, onMounted, ref, shallowRef } from "vue";
import type { CommandAuthority } from "../operations/command-port";
import {
  createOperationsPort,
  initialOperationsSnapshot,
  type OperationsPort,
  type SignalView,
} from "../operations/operations-port";

const PerspectiveExplorer = defineAsyncComponent(() => import("./PerspectiveExplorer.vue"));
const CommandPlane = defineAsyncComponent(() => import("./CommandPlane.vue"));

const snapshot = shallowRef(initialOperationsSnapshot);
const hasSnapshot = ref(false);
const lastSuccessfulAt = ref<string>();
const failureReason = ref("");
const view = ref<"overview" | "control" | "explorer">("overview");
const connection = ref<"connecting" | "streaming" | "reconnecting" | "snapshot" | "paused" | "error">("connecting");
const selectedSignalId = ref(initialOperationsSnapshot.signals[0].id);
const inspectedOrderId = ref(initialOperationsSnapshot.orders[0].clientOrderId);
const commandAuthority = shallowRef<CommandAuthority>({
  state: "unavailable",
  detail:
    typeof window !== "undefined" &&
    window.__HELIOS_OPERATIONS__?.commandUrl &&
    window.__HELIOS_OPERATIONS__.commandSessionUrl
      ? "Open Control plane to verify command authority"
      : "No authenticated command service is configured",
});
let port: OperationsPort | undefined;
let unsubscribe: (() => void) | undefined;

const selectedSignal = computed(() =>
  snapshot.value.signals.find((signal) => signal.id === selectedSignalId.value) ?? snapshot.value.signals[0],
);
const selectedOrder = computed(() =>
  snapshot.value.orders.find((order) => order.clientOrderId === inspectedOrderId.value) ?? snapshot.value.orders[0],
);
const grossUtilization = computed(() => {
  const gross = BigInt(snapshot.value.risk.grossExposureMicros);
  const limit = BigInt(snapshot.value.risk.grossLimitMicros);
  if (limit === 0n) return 0;
  return Number((gross * 1_000_000n) / limit) / 1_000_000;
});
const unrealizedTotal = computed(() =>
  snapshot.value.positions.reduce((total, position) => total + BigInt(position.unrealizedPnlMicros), 0n),
);
const modeLabel = computed(() => {
  if (!hasSnapshot.value) return "Mode pending";
  return {
    demo: "Demo",
    shadow: "Shadow",
    paper: "Paper",
    live: "Live",
  }[snapshot.value.mode];
});
const capitalLabel = computed(() => {
  if (!hasSnapshot.value) return "Capital unknown";
  return snapshot.value.risk.capitalGate === "authorized" ? "Capital authorized" : "Capital closed";
});
const dataTitle = computed(() => {
  if (!hasSnapshot.value) return "Data pending";
  return snapshot.value.dataClass === "synthetic" ? "Synthetic feed" : "Observed feed";
});
const dataDetail = computed(() => {
  if (!hasSnapshot.value) return "Waiting for a validated snapshot";
  return snapshot.value.dataClass === "synthetic" ? "Deterministic UI fixture" : "Source-backed operations data";
});
const isStale = computed(() => hasSnapshot.value && ["connecting", "reconnecting", "error"].includes(connection.value));
const boundaryTitle = computed(() => {
  if (isStale.value) return "Stale operations snapshot";
  return commandAuthority.value.state === "authenticated"
    ? "Protected command plane attached"
    : "Operations read model";
});
const boundaryDetail = computed(() => {
  if (isStale.value) {
    return `Updates are unavailable. Showing the last validated observation from ${lastObservationLabel.value}.`;
  }
  if (commandAuthority.value.state === "authenticated") {
    return `Commands are admitted by a separate authenticated service. ${snapshot.value.risk.capitalGateReason}.`;
  }
  return `${commandAuthority.value.detail}. ${snapshot.value.risk.capitalGateReason}.`;
});
const lastObservationLabel = computed(() => {
  if (!lastSuccessfulAt.value) return "no successful snapshot";
  return new Intl.DateTimeFormat("en-US", {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    timeZoneName: "short",
  }).format(new Date(lastSuccessfulAt.value));
});
const operationalAnnouncement = computed(() => {
  if (!hasSnapshot.value) {
    return connection.value === "error"
      ? `Operations source unavailable. ${failureReason.value}`
      : "Connecting to the operations source.";
  }
  const blocked = snapshot.value.signals.filter((signal) => signal.state === "blocked").length;
  const state = isStale.value ? `Operations snapshot stale as of ${lastObservationLabel.value}` : `Feed ${connection.value}`;
  return `${state}. ${capitalLabel.value}. ${blocked} blocked signals. ${snapshot.value.risk.pendingReconciliations} pending reconciliations.`;
});

function applySnapshot(next: typeof initialOperationsSnapshot): void {
  snapshot.value = next;
  hasSnapshot.value = true;
  lastSuccessfulAt.value = next.observedAt;
  failureReason.value = "";
}

function startSubscription(): void {
  if (!port) return;
  unsubscribe?.();
  if (!port.supportsStreaming) {
    connection.value = "snapshot";
    return;
  }
  connection.value = "connecting";
  unsubscribe = port.subscribe(
    (next) => {
      applySnapshot(next);
    },
    (status) => {
      connection.value = status;
      if (status === "reconnecting") failureReason.value = "The live update channel is reconnecting";
      if (status === "error") failureReason.value = "A malformed streaming snapshot was rejected";
    },
  );
}

async function connectPort(): Promise<void> {
  unsubscribe?.();
  unsubscribe = undefined;
  port?.close();
  port = undefined;
  connection.value = "connecting";
  failureReason.value = "";
  try {
    port = createOperationsPort();
    applySnapshot(await port.load());
    startSubscription();
  } catch (error) {
    console.error(error);
    failureReason.value = error instanceof Error ? error.message : "The operations source could not be loaded";
    connection.value = "error";
  }
}

function applyCommandAuthority(authority: CommandAuthority): void {
  commandAuthority.value = authority;
}

function handleStreamControl(): void {
  if (connection.value === "error") {
    void connectPort();
    return;
  }
  if (connection.value === "snapshot") return;
  if (connection.value === "paused") {
    startSubscription();
  } else {
    unsubscribe?.();
    unsubscribe = undefined;
    connection.value = "paused";
  }
}

onMounted(() => void connectPort());

onBeforeUnmount(() => {
  unsubscribe?.();
  port?.close();
});

function money(micros: string | bigint, signed = false): string {
  const value = BigInt(micros);
  const negative = value < 0n;
  const absolute = negative ? -value : value;
  const roundedCents = (absolute + 5_000n) / 10_000n;
  const dollars = roundedCents / 100n;
  const cents = roundedCents % 100n;
  const prefix = negative ? "-" : signed ? "+" : "";
  const whole = new Intl.NumberFormat("en-US", { maximumFractionDigits: 0 }).format(dollars);
  const fraction = dollars < 100n || cents !== 0n ? `.${cents.toString().padStart(2, "0")}` : "";
  return `${prefix}$${whole}${fraction}`;
}

function quantity(micros: string): string {
  const value = BigInt(micros);
  const negative = value < 0n;
  const absolute = negative ? -value : value;
  const whole = new Intl.NumberFormat("en-US", { maximumFractionDigits: 0 }).format(absolute / 1_000_000n);
  const fraction = (absolute % 1_000_000n).toString().padStart(6, "0").replace(/0+$/, "");
  return `${negative ? "-" : ""}${whole}${fraction ? `.${fraction}` : ""}`;
}

function percent(bps: number): string {
  return `${(bps / 100).toFixed(1)}%`;
}

function shortId(id: string): string {
  return `${id.slice(0, 6)}…${id.slice(-4)}`;
}

function streamLabel(): string {
  switch (connection.value) {
    case "paused": return "Resume feed";
    case "error": return "Retry feed";
    case "reconnecting": return "Reconnecting";
    case "snapshot": return "Snapshot only";
    case "connecting": return "Connecting";
    default: return "Pause feed";
  }
}

function eventTime(offsetMs = 0): string {
  const observed = new Date(snapshot.value.observedAt).getTime();
  return new Date(observed - offsetMs).toISOString().slice(11, 23);
}

function plotPoints(signal: SignalView): string {
  const values = signal.trace.map((point) => point.valueBps);
  const min = Math.min(...values);
  const max = Math.max(...values);
  const range = Math.max(1, max - min);
  return signal.trace
    .map((point, index) => `${(index / (signal.trace.length - 1)) * 100},${44 - ((point.valueBps - min) / range) * 36}`)
    .join(" ");
}

const queueDepth = computed(() => 12_842 + (snapshot.value.sequence % 7) * 13);
const tapeRows = computed(() => [
  { sequence: snapshot.value.sequence - 5, time: eventTime(517), type: "FLUX", source: "GOES-R", stage: "R3A", reduce: "P1", output: "PROTON_PULSE", tone: "blue" },
  { sequence: snapshot.value.sequence - 4, time: eventTime(414), type: "PLASMA", source: "DSCOVR", stage: "R2C", reduce: "P3", output: "PACKET_GAP", tone: "oxide" },
  { sequence: snapshot.value.sequence - 3, time: eventTime(313), type: "MARK", source: "BTC-USD", stage: "R1B", reduce: "P2", output: "LIQUIDITY", tone: "green" },
  { sequence: snapshot.value.sequence - 2, time: eventTime(238), type: "NOWCAST", source: "SWPC", stage: "R3A", reduce: "P1", output: "KP_UPDATE", tone: "blue" },
  { sequence: snapshot.value.sequence - 1, time: eventTime(184), type: "SIGNAL", source: "CME-LIQ", stage: "R4D", reduce: "P4", output: "REDUCE_LONG", tone: "green" },
  {
    sequence: snapshot.value.sequence,
    time: eventTime(),
    type: "ORDER",
    source: "RISK",
    stage: "R1B",
    reduce: "P2",
    output: snapshot.value.mode === "live" ? "EXECUTION_STATE" : `${snapshot.value.mode.toUpperCase()}_FILL`,
    tone: "green",
  },
]);
</script>

<template>
  <div class="operator-console">
    <header class="operator-commandbar">
      <div class="operator-identity">
        <span class="operator-mark" aria-hidden="true"><i></i><i></i><i></i><i></i></span>
        <div>
          <strong>Helios Control</strong>
          <span>{{ hasSnapshot ? snapshot.accountLabel : "Operations source pending" }}</span>
        </div>
      </div>
      <nav aria-label="Console views">
        <button :aria-current="view === 'overview' ? 'page' : undefined" @click="view = 'overview'">Overview</button>
        <button
          :aria-current="view === 'control' ? 'page' : undefined"
          :disabled="!hasSnapshot"
          @click="view = 'control'"
        >
          Control plane
          <span>Protected</span>
        </button>
        <button
          :aria-current="view === 'explorer' ? 'page' : undefined"
          :disabled="!hasSnapshot"
          @click="view = 'explorer'"
        >
          Data explorer
          <span>WASM</span>
        </button>
      </nav>
      <div class="operator-session">
        <span class="mode-chip" :data-mode="hasSnapshot ? snapshot.mode : 'pending'">{{ modeLabel }}</span>
        <span class="capital-chip" :data-gate="hasSnapshot ? snapshot.risk.capitalGate : 'unknown'">{{ capitalLabel }}</span>
        <span class="data-chip" :data-class="hasSnapshot ? snapshot.dataClass : 'pending'">{{ dataTitle }}</span>
        <span class="command-chip" :data-authority="commandAuthority.state">
          {{ commandAuthority.state === "authenticated" ? "Commands secured" : "Commands unbound" }}
        </span>
        <button
          class="stream-control"
          :disabled="connection === 'connecting' || connection === 'snapshot'"
          @click="handleStreamControl"
        >
          <span :data-state="connection" aria-hidden="true"></span>
          {{ streamLabel() }}
        </button>
      </div>
    </header>
    <p class="sr-only" aria-live="polite" aria-atomic="true">{{ operationalAnnouncement }}</p>

    <section v-if="!hasSnapshot" class="operator-unavailable" :data-state="connection" aria-labelledby="unavailable-heading">
      <span class="unavailable-mark" aria-hidden="true"></span>
      <p>{{ connection === "error" ? "Operations source unavailable" : "Establishing operations channel" }}</p>
      <h1 id="unavailable-heading">{{ connection === "error" ? "No validated snapshot is available" : "Waiting for the first validated snapshot" }}</h1>
      <span>{{ failureReason || "Establishing the read-only operations connection." }}</span>
      <small>Last successful observation: {{ lastObservationLabel }}</small>
      <button v-if="connection === 'error'" @click="connectPort">Retry operations feed</button>
    </section>

    <template v-else-if="view === 'overview'">
      <div class="operator-boundary" :data-stale="isStale ? 'true' : undefined">
        <strong>{{ boundaryTitle }}</strong>
        <span>{{ boundaryDetail }}</span>
        <span class="operator-sequence">SEQ {{ snapshot.sequence.toLocaleString() }}</span>
      </div>

      <div class="operator-layout">
        <aside class="operator-rail" aria-label="Operations index">
          <a href="#control-plane" @click.prevent="view = 'control'">
            <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M2.5 4h11M2.5 8h11M2.5 12h11M5 2.5v3M10.5 6.5v3M7.5 10.5v3"/></svg>
            Control plane
            <span>{{ snapshot.strategies.length }}</span>
          </a>
          <a href="#signal-tape" class="active">
            <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M2 11 5.2 7.8l2.3 1.8L12.8 4 14 5.2"/><circle cx="5.2" cy="7.8" r="1"/><circle cx="12.8" cy="4" r="1"/></svg>
            Signal tape
            <span>{{ snapshot.signals.length }}</span>
          </a>
          <a href="#positions-ledger">
            <svg viewBox="0 0 16 16" aria-hidden="true"><rect x="2.5" y="3" width="11" height="10"/><path d="M2.5 6.5h11M6 6.5V13"/></svg>
            Positions
            <span>{{ snapshot.positions.length }}</span>
          </a>
          <a href="#orders-ledger">
            <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M3 2.5h8l2 2V14H3zM10.5 2.5V5H13M5.5 8h5M5.5 10.5h3.5"/></svg>
            Active orders
            <span>{{ snapshot.orders.length }}</span>
          </a>
          <a href="#execution-ledger">
            <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M2.5 8h11M8 2.5v11M4.5 4.5l7 7M11.5 4.5l-7 7"/><circle cx="8" cy="8" r="5.5"/></svg>
            Executions
            <span>{{ snapshot.fills.length }}</span>
          </a>
          <a href="#source-health">
            <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M3 12.5V9m3 3.5V6m3 6.5V3.5m3 9V7.5"/></svg>
            Source health
            <span>{{ snapshot.sources.filter((source) => source.health !== 'healthy').length }}</span>
          </a>

          <div class="rail-ledger">
            <p>Runtime</p>
            <dl>
              <div><dt>Provider</dt><dd>{{ snapshot.provider }}</dd></div>
              <div><dt>Observed</dt><dd>{{ connection === "connecting" ? "waiting" : connection }}</dd></div>
              <div><dt>Checkpoint</dt><dd>{{ snapshot.risk.checkpointAgeMs }}ms</dd></div>
              <div><dt>Clock</dt><dd>{{ snapshot.risk.clockOffsetMs }}ms</dd></div>
            </dl>
          </div>
          <div class="data-stamp" :data-class="snapshot.dataClass">
            <svg viewBox="0 0 18 18" aria-hidden="true"><path d="M9 1.8 15.3 5.4v7.2L9 16.2l-6.3-3.6V5.4zM2.7 5.4 9 9l6.3-3.6M9 9v7.2"/></svg>
            <div><strong>{{ dataTitle }}</strong><span>{{ dataDetail }}</span></div>
          </div>
        </aside>

        <main class="operator-workspace">
          <section class="operator-summary" aria-label="Portfolio and system summary">
            <div class="summary-scroll" tabindex="0" aria-label="Portfolio and system summary. Scroll horizontally for all metrics.">
              <dl>
                <div><dt>Gross exposure</dt><dd>{{ money(snapshot.risk.grossExposureMicros) }}</dd><small>{{ (grossUtilization * 100).toFixed(1) }}% of limit</small></div>
                <div><dt>Reserved</dt><dd>{{ money(snapshot.risk.reservedGrossMicros) }}</dd><small>Open order capacity</small></div>
                <div><dt>Unrealized P&amp;L</dt><dd :class="{ negative: unrealizedTotal < 0n }">{{ money(unrealizedTotal, true) }}</dd><small>Across {{ snapshot.positions.length }} positions</small></div>
                <div><dt>Orders today</dt><dd>{{ snapshot.risk.dailyOrderCount }} / {{ snapshot.risk.dailyOrderLimit }}</dd><small>{{ snapshot.risk.pendingReconciliations }} unreconciled</small></div>
                <div><dt>Source lag</dt><dd :class="{ caution: snapshot.risk.sourceLagMs > 2_000 }">{{ snapshot.risk.sourceLagMs.toLocaleString() }}ms</dd><small>Worst active source</small></div>
              </dl>
            </div>
            <span class="summary-scroll-cue" aria-hidden="true">Scroll for all portfolio facts →</span>
          </section>

          <section id="signal-tape" class="signal-tape" aria-labelledby="signal-tape-heading">
            <header>
              <div>
                <h1 id="signal-tape-heading">Live event path</h1>
                <p>Every event keeps its source time, reducer owner, signal output, and execution boundary.</p>
              </div>
              <dl class="tape-metrics">
                <div><dt>Queue depth</dt><dd>{{ queueDepth.toLocaleString() }}</dd></div>
                <div><dt>Event time</dt><dd>{{ eventTime() }}</dd></div>
                <div><dt>Lag</dt><dd class="caution">{{ (snapshot.risk.sourceLagMs / 1_000).toFixed(2) }}s</dd></div>
              </dl>
            </header>

            <span class="tape-scroll-cue" aria-hidden="true">Scroll for reorder, reduce, and effect →</span>
            <div
              class="tape-scroll"
              tabindex="0"
              role="region"
              :aria-label="`${dataTitle} event table. Scroll horizontally for reorder, reduce, and effect columns.`"
            >
              <div class="tape-table" role="table" aria-colcount="6" :aria-rowcount="tapeRows.length + 1">
                <div role="rowgroup">
                  <div class="tape-columns" role="row">
                    <span role="columnheader">Sequence</span><span role="columnheader">Event time</span><span role="columnheader">Input</span><span role="columnheader">Reorder</span><span role="columnheader">Reduce</span><span role="columnheader">Signal / effect</span>
                  </div>
                </div>
                <div class="tape-body" role="rowgroup">
                  <div v-for="(row, index) in tapeRows" :key="row.sequence" class="tape-row" :data-tone="row.tone" role="row">
                    <span class="tape-play" :class="{ moving: index === tapeRows.length - 1 && connection === 'streaming' }" aria-hidden="true"></span>
                    <code role="cell">{{ String(row.sequence).padStart(12, "0") }}</code>
                    <code role="cell">{{ row.time }}</code>
                    <strong role="cell">{{ row.type }}</strong>
                    <div class="tape-run" role="cell"><span></span><b>{{ row.stage }}</b></div>
                    <div class="tape-run" role="cell"><span></span><b>{{ row.reduce }}</b></div>
                    <div class="tape-run output" role="cell"><span></span><b>{{ row.output }}</b></div>
                  </div>
                </div>
              </div>
            </div>

            <div class="signal-decision">
              <div class="signal-list" role="list" aria-label="Signal candidates">
                <button
                  v-for="signal in snapshot.signals"
                  :key="signal.id"
                  :class="{ selected: signal.id === selectedSignal.id }"
                  :aria-pressed="signal.id === selectedSignal.id"
                  @click="selectedSignalId = signal.id"
                >
                  <span class="signal-state" :data-state="signal.state">{{ signal.state }}</span>
                  <strong>{{ signal.instrument }}</strong>
                  <span>{{ signal.hypothesis }}</span>
                  <b>{{ percent(signal.posteriorBps) }}</b>
                </button>
              </div>
              <article v-if="selectedSignal" class="signal-inspector">
                <header>
                  <div><span>{{ selectedSignal.instrument }} · {{ selectedSignal.horizon }}</span><h2>{{ selectedSignal.hypothesis }}</h2></div>
                  <strong>{{ percent(selectedSignal.posteriorBps) }}</strong>
                </header>
                <svg class="signal-plot" viewBox="0 0 100 48" role="img" :aria-label="`${selectedSignal.hypothesis} posterior trace`" preserveAspectRatio="none">
                  <path d="M0 38H100M0 24H100M0 10H100M50 4V44" class="plot-grid" />
                  <polyline :points="plotPoints(selectedSignal)" class="plot-line" vector-effect="non-scaling-stroke" />
                  <circle cx="50" cy="8" r="1.6" class="plot-event" />
                </svg>
                <dl>
                  <div><dt>Trigger</dt><dd>{{ selectedSignal.trigger }}</dd></div>
                  <div><dt>Available</dt><dd>{{ selectedSignal.availableAt }}</dd></div>
                  <div><dt>Decision cut</dt><dd>{{ selectedSignal.decisionCut }}</dd></div>
                  <div><dt>Proposed effect</dt><dd>{{ selectedSignal.action }}</dd></div>
                  <div v-if="selectedSignal.blocker" class="blocker"><dt>Blocker</dt><dd>{{ selectedSignal.blocker }}</dd></div>
                </dl>
                <ol class="lineage" aria-label="Signal lineage">
                  <li v-for="step in selectedSignal.lineage" :key="step">{{ step }}</li>
                </ol>
              </article>
              <p v-else class="operator-empty">No signal candidates in this snapshot.</p>
            </div>
          </section>

          <div class="ledger-grid">
            <section id="positions-ledger" class="ledger-panel positions-panel" aria-labelledby="positions-heading">
              <header><div><h2 id="positions-heading">Positions held</h2><p>Account marks stay separate from research estimates.</p></div><span>{{ snapshot.positions.length }} open</span></header>
              <div class="table-scroll" tabindex="0" aria-label="Positions table, scroll horizontally on small screens">
                <table>
                  <thead><tr><th>Instrument</th><th>Strategy</th><th class="number">Quantity</th><th class="number">Average</th><th class="number">Mark</th><th class="number">Market value</th><th class="number">Unrealized</th><th class="number">Freshness</th></tr></thead>
                  <tbody>
                    <tr v-for="position in snapshot.positions" :key="`${position.instrument}:${position.strategy}`">
                      <th>{{ position.instrument }}</th><td>{{ position.strategy }}</td><td class="number">{{ quantity(position.quantityMicros) }}</td><td class="number">{{ money(position.averagePriceMicros) }}</td><td class="number">{{ money(position.markPriceMicros) }}</td><td class="number">{{ money(position.marketValueMicros) }}</td><td class="number" :class="Number(position.unrealizedPnlMicros) >= 0 ? 'positive' : 'negative'">{{ money(position.unrealizedPnlMicros, true) }}</td><td class="number">{{ position.freshnessMs }}ms</td>
                    </tr>
                  </tbody>
                </table>
              </div>
            </section>

            <section id="orders-ledger" class="ledger-panel orders-panel" aria-labelledby="orders-heading">
              <header><div><h2 id="orders-heading">Active orders</h2><p>Authoritative OMS state compared with venue truth through stable order and execution identities.</p></div><span>{{ snapshot.orders.length }} active</span></header>
              <div class="order-layout">
                <div class="order-list" role="list">
                  <button v-for="order in snapshot.orders" :key="order.clientOrderId" :class="{ selected: order.clientOrderId === selectedOrder.clientOrderId }" @click="inspectedOrderId = order.clientOrderId">
                    <span :data-side="order.side">{{ order.side }}</span><strong>{{ order.instrument }}</strong><b>{{ order.state.replace('_', ' ') }}</b><code>{{ shortId(order.clientOrderId) }}</code>
                  </button>
                </div>
                <dl v-if="selectedOrder" class="order-detail">
                  <div><dt>Venue</dt><dd>{{ selectedOrder.venue }}</dd></div><div><dt>Quantity</dt><dd>{{ quantity(selectedOrder.quantityMicros) }}</dd></div><div><dt>Filled</dt><dd>{{ quantity(selectedOrder.filledQuantityMicros) }}</dd></div><div><dt>Limit</dt><dd>{{ money(selectedOrder.limitPriceMicros) }}</dd></div><div><dt>Average</dt><dd>{{ selectedOrder.averagePriceMicros ? money(selectedOrder.averagePriceMicros) : "Not filled" }}</dd></div><div><dt>Reconciliation</dt><dd class="verified">{{ selectedOrder.reconciliation }}</dd></div><div v-if="selectedOrder.omsVersion !== undefined"><dt>OMS version</dt><dd>{{ selectedOrder.omsVersion }}</dd></div><div v-if="selectedOrder.brokerOrderId"><dt>Venue order</dt><dd><code>{{ shortId(selectedOrder.brokerOrderId) }}</code></dd></div><div v-if="selectedOrder.timeInForce"><dt>Time in force</dt><dd>{{ selectedOrder.timeInForce.replaceAll('_', ' ') }}</dd></div><div v-if="selectedOrder.uncertaintyReason"><dt>Uncertainty</dt><dd class="negative">{{ selectedOrder.uncertaintyReason }}</dd></div>
                </dl>
                <p v-else class="operator-empty">No active orders in this snapshot.</p>
              </div>
            </section>
          </div>

          <section id="execution-ledger" class="ledger-panel execution-panel" aria-labelledby="execution-heading">
            <header>
              <div><h2 id="execution-heading">Recent executions</h2><p>Broker-confirmed fills stay linked to order intent and strategy ownership.</p></div>
              <span>{{ snapshot.fills.length }} confirmed</span>
            </header>
            <div class="table-scroll" tabindex="0" aria-label="Recent executions table, scroll horizontally on small screens">
              <table>
                <thead><tr><th>Executed</th><th>Instrument</th><th>Side</th><th>Strategy</th><th>Venue</th><th>Liquidity</th><th class="number">Quantity</th><th class="number">Price</th><th>Execution ID</th><th>Order ID</th></tr></thead>
                <tbody>
                  <tr v-for="fill in snapshot.fills" :key="fill.executionId">
                    <td>{{ fill.executedAt }}</td><th>{{ fill.instrument }}</th><td :class="fill.side === 'buy' ? 'positive' : 'negative'">{{ fill.side }}</td><td>{{ fill.strategy }}</td><td>{{ fill.venue }}</td><td>{{ fill.liquidity }}</td><td class="number">{{ quantity(fill.quantityMicros) }}</td><td class="number">{{ money(fill.priceMicros) }}</td><td><code>{{ fill.executionId }}</code></td><td><code>{{ shortId(fill.clientOrderId) }}</code></td>
                  </tr>
                </tbody>
              </table>
            </div>
          </section>

          <section id="source-health" class="source-health" aria-labelledby="source-heading">
            <header><div><h2 id="source-heading">What the system is working with</h2><p>Source watermarks and lag determine whether a strategy is allowed to update.</p></div><button @click="view = 'explorer'">Open the Perspective explorer <span>→</span></button></header>
            <ul>
              <li v-for="source in snapshot.sources" :key="`${source.name}:${source.channel}`">
                <span class="health-dot" :data-state="source.health" aria-hidden="true"></span>
                <div><strong>{{ source.name }}</strong><span>{{ source.channel }}</span></div>
                <dl><div><dt>Lag</dt><dd>{{ source.lagMs.toLocaleString() }}ms</dd></div><div><dt>Watermark</dt><dd>{{ source.watermark }}</dd></div></dl>
                <span class="source-detail">{{ source.detail }}</span>
              </li>
            </ul>
          </section>
        </main>
      </div>
    </template>

    <Suspense v-else-if="view === 'control'">
      <CommandPlane :snapshot="snapshot" :stale="isStale" @authority="applyCommandAuthority" />
      <template #fallback><div class="explorer-fallback" role="status">Preparing protected command plane…</div></template>
    </Suspense>

    <Suspense v-else>
      <PerspectiveExplorer :snapshot="snapshot" />
      <template #fallback><div class="explorer-fallback" role="status">Preparing analytical worker…</div></template>
    </Suspense>
  </div>
</template>

<style scoped>
.operator-console {
  --operator-black: #05090d;
  min-height: 100vh;
  color: var(--atlas-ink);
  background: var(--atlas-ground);
  font-family: var(--vp-font-family-base);
}

.operator-console *,
.operator-console *::before,
.operator-console *::after { box-sizing: border-box; }

.operator-console button,
.operator-console a { -webkit-tap-highlight-color: transparent; }

.operator-console button:focus-visible,
.operator-console a:focus-visible,
.table-scroll:focus-visible,
.summary-scroll:focus-visible,
.tape-scroll:focus-visible {
  outline: 2px solid var(--atlas-oxide);
  outline-offset: -2px;
}

.sr-only {
  position: absolute;
  width: 1px;
  height: 1px;
  padding: 0;
  overflow: hidden;
  white-space: nowrap;
  border: 0;
  clip: rect(0, 0, 0, 0);
  clip-path: inset(50%);
}

.operator-commandbar {
  position: sticky;
  top: 0;
  z-index: 20;
  display: grid;
  grid-template-columns: minmax(280px, 1fr) auto minmax(560px, 1fr);
  min-height: 70px;
  border-bottom: 1px solid var(--atlas-rule);
  background: color-mix(in srgb, var(--atlas-ground) 97%, transparent);
  backdrop-filter: blur(18px);
}

.operator-identity,
.operator-session,
.operator-commandbar nav {
  display: flex;
  align-items: center;
}

.operator-identity { gap: 13px; padding: 0 22px; }
.operator-identity > div { display: grid; gap: 2px; }
.operator-identity strong { font-size: 16px; font-weight: 660; letter-spacing: -0.015em; }
.operator-identity > div > span,
.operator-session,
.operator-commandbar nav {
  font-family: var(--vp-font-family-mono);
  font-size: 10px;
  font-variation-settings: "MONO" 1, "CASL" 0;
  letter-spacing: 0.05em;
  text-transform: uppercase;
}
.operator-identity > div > span { color: var(--atlas-muted); }

.operator-mark { display: grid; grid-template-columns: repeat(2, 5px); gap: 3px; width: 21px; height: 21px; padding: 3px; border: 1px solid var(--atlas-green); }
.operator-mark i { background: var(--atlas-green); }

.operator-commandbar nav { align-self: stretch; }
.operator-commandbar nav button {
  position: relative;
  height: 100%;
  padding: 0 20px;
  color: var(--atlas-muted);
  border: 0;
  border-right: 1px solid var(--atlas-rule-soft);
  border-left: 1px solid transparent;
  background: transparent;
  cursor: pointer;
}
.operator-commandbar nav button:hover { color: var(--atlas-ink); background: var(--atlas-surface-alt); }
.operator-commandbar nav button[aria-current="page"] { color: var(--atlas-blue); background: var(--atlas-surface-strong); }
.operator-commandbar nav button[aria-current="page"]::after { position: absolute; right: 0; bottom: 0; left: 0; height: 2px; content: ""; background: var(--atlas-blue); }
.operator-commandbar nav button span { margin-left: 6px; color: var(--atlas-green-ink); font-size: 8px; }
.operator-commandbar nav button:disabled { color: var(--atlas-axis); background: transparent; cursor: not-allowed; }

.operator-session { justify-content: flex-end; gap: 6px; padding: 0 18px; }
.mode-chip,
.capital-chip,
.data-chip,
.command-chip { display: inline-flex; min-height: 30px; align-items: center; padding: 5px 8px; white-space: nowrap; border: 1px solid var(--atlas-rule); }
.mode-chip { color: var(--atlas-blue); }
.mode-chip[data-mode="live"] { color: var(--atlas-green-ink); border-color: var(--atlas-green); }
.mode-chip[data-mode="pending"] { color: var(--atlas-axis); }
.capital-chip { color: var(--atlas-oxide); }
.capital-chip[data-gate="authorized"] { color: var(--atlas-green-ink); border-color: var(--atlas-green); }
.capital-chip[data-gate="unknown"] { color: var(--atlas-axis); }
.data-chip { color: var(--atlas-blue); }
.data-chip[data-class="observed"] { color: var(--atlas-green-ink); border-color: var(--atlas-green); }
.data-chip[data-class="pending"] { color: var(--atlas-axis); }
.command-chip { color: var(--atlas-axis); }
.command-chip[data-authority="authenticated"] { color: var(--atlas-green-ink); border-color: var(--atlas-green); }
.command-chip[data-authority="expired"] { color: var(--atlas-oxide); }
.stream-control {
  display: flex;
  align-items: center;
  gap: 7px;
  min-height: 30px;
  padding: 6px 10px;
  color: var(--atlas-ink);
  font: inherit;
  text-transform: inherit;
  border: 1px solid var(--atlas-rule);
  border-radius: 1px;
  background: transparent;
  cursor: pointer;
}
.stream-control:hover { color: var(--atlas-blue); border-color: var(--atlas-blue); }
.stream-control:disabled { opacity: 0.45; cursor: wait; }
.stream-control > span { width: 7px; height: 7px; border-radius: 50%; background: var(--atlas-green); }
.stream-control > span[data-state="connecting"],
.stream-control > span[data-state="reconnecting"] { background: var(--atlas-blue); animation: status-pulse 900ms ease-out infinite alternate; }
.stream-control > span[data-state="paused"],
.stream-control > span[data-state="error"] { background: var(--atlas-oxide); animation: none; }
.stream-control > span[data-state="snapshot"] { background: var(--atlas-axis); animation: none; }

.operator-boundary {
  display: grid;
  grid-template-columns: auto 1fr auto;
  gap: 18px;
  align-items: center;
  min-height: 44px;
  padding: 0 22px;
  color: var(--atlas-muted);
  font-size: 13px;
  border-bottom: 1px solid var(--atlas-rule);
  background: var(--atlas-surface-alt);
}
.operator-boundary strong { color: var(--atlas-oxide); font-family: var(--vp-font-family-mono); font-size: 10px; letter-spacing: 0.05em; text-transform: uppercase; }
.operator-sequence { color: var(--atlas-axis); font-family: var(--vp-font-family-mono); font-size: 10px; font-variant-numeric: tabular-nums; }
.operator-boundary[data-stale="true"] { color: var(--atlas-ink); background: color-mix(in srgb, var(--atlas-oxide) 9%, var(--atlas-ground)); }

.operator-unavailable {
  display: grid;
  min-height: calc(100vh - 122px);
  place-content: center;
  justify-items: start;
  padding: 32px;
  background: var(--operator-black);
}
.unavailable-mark { width: 24px; height: 24px; margin-bottom: 30px; border: 1px solid var(--atlas-oxide); background: linear-gradient(135deg, transparent 46%, var(--atlas-oxide) 47%, var(--atlas-oxide) 53%, transparent 54%); }
.operator-unavailable p { margin: 0 0 8px; color: var(--atlas-oxide); font-family: var(--vp-font-family-mono); font-size: 10px; letter-spacing: .08em; text-transform: uppercase; }
.operator-unavailable h1 { max-width: 580px; margin: 0; font-size: clamp(28px, 5vw, 58px); line-height: .98; letter-spacing: -.045em; }
.operator-unavailable > span { max-width: 600px; margin-top: 16px; color: var(--atlas-muted); font-size: 13px; }
.operator-unavailable small { margin-top: 8px; color: var(--atlas-axis); font-family: var(--vp-font-family-mono); font-size: 10px; text-transform: uppercase; }
.operator-unavailable button { margin-top: 24px; padding: 9px 12px; color: var(--atlas-ground); font-family: var(--vp-font-family-mono); font-size: 10px; text-transform: uppercase; border: 1px solid var(--atlas-oxide); background: var(--atlas-oxide); cursor: pointer; }
.operator-unavailable button:hover { color: var(--atlas-oxide); background: transparent; }

.operator-layout { display: grid; grid-template-columns: 220px minmax(0, 1fr); max-width: 1920px; min-height: calc(100vh - 114px); margin: 0 auto; border-right: 1px solid var(--atlas-rule); border-left: 1px solid var(--atlas-rule); }
.operator-rail { position: relative; min-width: 0; padding-top: 8px; border-right: 1px solid var(--atlas-rule); background: var(--atlas-surface-alt); }
.operator-rail > a {
  display: grid;
  grid-template-columns: 16px 1fr auto;
  gap: 9px;
  align-items: center;
  min-height: 44px;
  padding: 0 16px;
  color: var(--atlas-muted);
  font-family: var(--vp-font-family-mono);
  font-size: 10px;
  font-variation-settings: "MONO" 1, "CASL" 0;
  letter-spacing: 0.03em;
  text-transform: uppercase;
  border-top: 1px solid transparent;
  border-bottom: 1px solid transparent;
}
.operator-rail > a svg { width: 16px; height: 16px; fill: none; stroke: currentColor; stroke-width: 1.2; }
.operator-rail > a > span { color: var(--atlas-axis); font-variant-numeric: tabular-nums; }
.operator-rail > a:hover,
.operator-rail > a.active { color: var(--atlas-blue); background: var(--atlas-blue-soft); border-color: var(--atlas-rule); }

.rail-ledger { margin-top: 28px; padding: 16px; border-top: 1px solid var(--atlas-rule); border-bottom: 1px solid var(--atlas-rule); }
.rail-ledger > p { margin: 0 0 12px; color: var(--atlas-blue); font-family: var(--vp-font-family-mono); font-size: 10px; letter-spacing: 0.06em; text-transform: uppercase; }
.rail-ledger dl { margin: 0; }
.rail-ledger dl div { display: grid; gap: 3px; padding: 9px 0; border-bottom: 1px solid var(--atlas-rule-soft); }
.rail-ledger dt { color: var(--atlas-axis); font-family: var(--vp-font-family-mono); font-size: 8px; text-transform: uppercase; }
.rail-ledger dd { min-width: 0; margin: 0; overflow: hidden; color: var(--atlas-muted); font-family: var(--vp-font-family-mono); font-size: 10px; text-overflow: ellipsis; white-space: nowrap; }
.data-stamp { position: sticky; top: calc(100vh - 86px); display: flex; gap: 9px; align-items: center; margin: 18px 12px; padding: 10px; border: 1px solid var(--atlas-rule); }
.data-stamp svg { width: 18px; fill: none; stroke: var(--atlas-blue); }
.data-stamp[data-class="observed"] svg { stroke: var(--atlas-green); }
.data-stamp div { display: grid; gap: 2px; }
.data-stamp strong { font-family: var(--vp-font-family-mono); font-size: 10px; letter-spacing: 0.04em; text-transform: uppercase; }
.data-stamp span { color: var(--atlas-muted); font-size: 10px; }

.operator-workspace { min-width: 0; }
.operator-summary { position: relative; border-bottom: 1px solid var(--atlas-rule); }
.summary-scroll > dl { display: grid; grid-template-columns: repeat(5, minmax(150px, 1fr)); margin: 0; }
.operator-summary dl > div { min-width: 0; padding: 18px 20px; border-right: 1px solid var(--atlas-rule); }
.operator-summary dl > div:last-child { border-right: 0; }
.operator-summary dt,
.tape-metrics dt { color: var(--atlas-axis); font-family: var(--vp-font-family-mono); font-size: 8px; letter-spacing: 0.05em; text-transform: uppercase; }
.operator-summary dd { margin: 7px 0 3px; font-family: var(--vp-font-family-mono); font-size: 21px; font-variant-numeric: tabular-nums; letter-spacing: -.035em; }
.operator-summary small { color: var(--atlas-muted); font-size: 11px; }
.summary-scroll-cue,
.tape-scroll-cue { display: none; }
.positive { color: var(--atlas-green-ink) !important; }
.negative { color: var(--atlas-oxide) !important; }
.caution { color: var(--atlas-oxide) !important; }


.signal-tape { scroll-margin-top: 124px; background: var(--operator-black); }
.signal-tape > header { display: flex; justify-content: space-between; gap: 24px; align-items: flex-start; min-height: 92px; padding: 21px 22px; border-bottom: 1px solid var(--atlas-rule); }
.signal-tape h1,
.ledger-panel h2,
.source-health h2,
.signal-inspector h2 { margin: 0; color: var(--atlas-ink); font-size: 20px; line-height: 1.2; letter-spacing: -0.02em; }
.signal-tape header p,
.ledger-panel header p,
.source-health header p { margin: 5px 0 0; color: var(--atlas-muted); font-size: 13px; line-height: 1.45; }
.tape-metrics { display: flex; margin: 0; }
.tape-metrics > div { min-width: 118px; padding: 3px 14px; border-left: 1px solid var(--atlas-rule); }
.tape-metrics dd { margin: 6px 0 0; color: var(--atlas-green-ink); font-family: var(--vp-font-family-mono); font-size: 21px; font-variant-numeric: tabular-nums; }

.tape-table { min-width: 960px; }
.tape-columns,
.tape-row { display: grid; grid-template-columns: 150px 126px 92px minmax(120px, 1fr) minmax(120px, .75fr) minmax(176px, 1.25fr); }
.tape-columns { padding: 9px 18px 9px 32px; color: var(--atlas-axis); font-family: var(--vp-font-family-mono); font-size: 8px; letter-spacing: 0.06em; text-transform: uppercase; border-bottom: 1px solid var(--atlas-rule); }
.tape-scroll { overflow-x: auto; scrollbar-color: var(--atlas-blue) var(--atlas-blue-soft); }
.tape-body { position: relative; padding: 7px 18px 10px; overflow: hidden; }
.tape-body::after { position: absolute; inset: 0; pointer-events: none; content: ""; background-image: linear-gradient(var(--atlas-rule-soft) 1px, transparent 1px), linear-gradient(90deg, var(--atlas-rule-soft) 1px, transparent 1px); background-size: 100% 28px, 92px 100%; opacity: .28; }
.tape-row { position: relative; z-index: 1; min-width: 930px; min-height: 35px; align-items: center; padding-left: 14px; color: var(--atlas-blue); font-family: var(--vp-font-family-mono); font-size: 11px; font-variant-numeric: tabular-nums; }
.tape-row[data-tone="green"] { color: var(--atlas-green); }
.tape-row[data-tone="oxide"] { color: var(--atlas-oxide); }
.tape-row code,
.tape-row strong { color: inherit; font: inherit; }
.tape-play { position: absolute; left: -2px; width: 7px; height: 7px; border-radius: 50%; background: currentColor; }
.tape-play.moving { animation: event-register 1.6s cubic-bezier(.16, 1, .3, 1) infinite; }
.tape-run { display: grid; grid-template-columns: 1fr 54px; align-items: center; min-width: 0; }
.tape-run span { height: 1px; background: currentColor; opacity: .6; }
.tape-run b { padding: 3px 5px; overflow: hidden; font-weight: 550; text-align: center; text-overflow: ellipsis; white-space: nowrap; border: 1px solid currentColor; background: var(--operator-black); }
.tape-run.output { grid-template-columns: 1fr minmax(118px, 1.5fr); }
.operator-empty { display: grid; min-height: 130px; place-items: center; margin: 0; padding: 20px; color: var(--atlas-axis); font-family: var(--vp-font-family-mono); font-size: 10px; text-transform: uppercase; }

.signal-decision { display: grid; grid-template-columns: minmax(420px, .85fr) minmax(500px, 1.15fr); border-top: 1px solid var(--atlas-rule); }
.signal-list { border-right: 1px solid var(--atlas-rule); }
.signal-list button { display: grid; grid-template-columns: 76px 72px 1fr auto; gap: 12px; align-items: center; width: 100%; min-height: 54px; padding: 0 18px; color: var(--atlas-muted); text-align: left; border: 0; border-bottom: 1px solid var(--atlas-rule-soft); background: transparent; cursor: pointer; }
.signal-list button:hover { background: var(--atlas-surface-alt); }
.signal-list button.selected { color: var(--atlas-ink); background: var(--atlas-blue-soft); }
.signal-list button strong,
.signal-list button b { font-family: var(--vp-font-family-mono); font-size: 11px; font-variant-numeric: tabular-nums; }
.signal-list button > span:nth-child(3) { overflow: hidden; font-size: 13px; text-overflow: ellipsis; white-space: nowrap; }
.signal-state { font-family: var(--vp-font-family-mono); font-size: 8px; letter-spacing: .04em; text-transform: uppercase; }
.signal-state[data-state="eligible"] { color: var(--atlas-green-ink); }
.signal-state[data-state="blocked"] { color: var(--atlas-oxide); }
.signal-state[data-state="observing"] { color: var(--atlas-blue); }
.signal-inspector { min-width: 0; padding: 18px 20px; background: var(--atlas-ground); }
.signal-inspector > header { display: flex; justify-content: space-between; gap: 18px; align-items: start; }
.signal-inspector header span { color: var(--atlas-blue); font-family: var(--vp-font-family-mono); font-size: 10px; letter-spacing: .04em; text-transform: uppercase; }
.signal-inspector header > strong { color: var(--atlas-green-ink); font-family: var(--vp-font-family-mono); font-size: 21px; }
.signal-plot { width: 100%; height: 108px; margin: 14px 0; overflow: visible; border-top: 1px solid var(--atlas-rule-soft); border-bottom: 1px solid var(--atlas-rule-soft); }
.plot-grid { fill: none; stroke: var(--atlas-rule-soft); stroke-width: .5; }
.plot-line { fill: none; stroke: var(--atlas-blue); stroke-width: 2; }
.plot-event { fill: var(--atlas-oxide); stroke: var(--atlas-ground); stroke-width: .7; }
.signal-inspector > dl { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); margin: 0; border-top: 1px solid var(--atlas-rule); border-left: 1px solid var(--atlas-rule); }
.signal-inspector > dl > div { padding: 10px; border-right: 1px solid var(--atlas-rule); border-bottom: 1px solid var(--atlas-rule); }
.signal-inspector dt,
.order-detail dt { color: var(--atlas-axis); font-family: var(--vp-font-family-mono); font-size: 8px; text-transform: uppercase; }
.signal-inspector dd,
.order-detail dd { margin: 4px 0 0; overflow: hidden; font-size: 11px; text-overflow: ellipsis; white-space: nowrap; }
.signal-inspector .blocker { grid-column: 1 / -1; color: var(--atlas-oxide); }
.lineage { display: flex; align-items: center; gap: 0; margin: 11px 0 0; padding: 0; list-style: none; }
.lineage li { display: flex; align-items: center; color: var(--atlas-muted); font-family: var(--vp-font-family-mono); font-size: 8px; text-transform: uppercase; }
.lineage li:not(:last-child)::after { width: 20px; height: 1px; margin: 0 7px; content: ""; background: var(--atlas-blue); }

.ledger-grid { display: grid; grid-template-columns: minmax(0, 1.35fr) minmax(420px, .65fr); }
.ledger-panel { min-width: 0; scroll-margin-top: 124px; border-top: 1px solid var(--atlas-rule); }
.ledger-panel + .ledger-panel { border-left: 1px solid var(--atlas-rule); }
.ledger-panel > header,
.source-health > header { display: flex; justify-content: space-between; align-items: start; gap: 18px; min-height: 78px; padding: 17px 20px; border-bottom: 1px solid var(--atlas-rule); }
.ledger-panel header > span { color: var(--atlas-blue); font-family: var(--vp-font-family-mono); font-size: 10px; text-transform: uppercase; }
.table-scroll { overflow-x: auto; scrollbar-color: var(--atlas-blue) var(--atlas-blue-soft); }
table { width: 100%; min-width: 920px; border-collapse: collapse; color: var(--atlas-muted); font-size: 11px; font-variant-numeric: tabular-nums; }
th,
td { height: 42px; padding: 0 14px; text-align: left; border-right: 1px solid var(--atlas-rule-soft); border-bottom: 1px solid var(--atlas-rule-soft); white-space: nowrap; }
thead th { height: 29px; color: var(--atlas-axis); font-family: var(--vp-font-family-mono); font-size: 8px; letter-spacing: .04em; text-transform: uppercase; background: var(--atlas-surface-alt); }
tbody th { color: var(--atlas-ink); font-family: var(--vp-font-family-mono); }
.number { text-align: right; font-family: var(--vp-font-family-mono); }
.execution-panel table { min-width: 1120px; }
.execution-panel code { color: var(--atlas-axis); font-family: var(--vp-font-family-mono); font-size: 10px; }
.order-layout { display: grid; grid-template-columns: minmax(190px, .9fr) minmax(170px, 1.1fr); }
.order-list { border-right: 1px solid var(--atlas-rule); }
.order-list button { display: grid; grid-template-columns: 40px 1fr; gap: 5px 9px; width: 100%; min-height: 68px; padding: 11px 13px; color: var(--atlas-muted); text-align: left; border: 0; border-bottom: 1px solid var(--atlas-rule-soft); background: transparent; cursor: pointer; }
.order-list button:hover,
.order-list button.selected { background: var(--atlas-blue-soft); }
.order-list button span { color: var(--atlas-green-ink); font-family: var(--vp-font-family-mono); font-size: 8px; text-transform: uppercase; }
.order-list button span[data-side="sell"] { color: var(--atlas-oxide); }
.order-list button strong { font-family: var(--vp-font-family-mono); font-size: 11px; }
.order-list button b { overflow: hidden; font-size: 10px; font-weight: 450; text-overflow: ellipsis; text-transform: uppercase; white-space: nowrap; }
.order-list button code { grid-column: 1 / -1; color: var(--atlas-axis); font-size: 8px; }
.order-detail { display: grid; grid-template-columns: repeat(2, 1fr); align-content: start; margin: 0; }
.order-detail > div { min-width: 0; padding: 10px; border-right: 1px solid var(--atlas-rule-soft); border-bottom: 1px solid var(--atlas-rule-soft); }
.verified { color: var(--atlas-green-ink); }
.source-health { scroll-margin-top: 124px; border-top: 1px solid var(--atlas-rule); border-bottom: 1px solid var(--atlas-rule); }
.source-health header button { color: var(--atlas-blue); font-family: var(--vp-font-family-mono); font-size: 10px; letter-spacing: .03em; text-transform: uppercase; border: 0; border-bottom: 1px solid currentColor; background: transparent; cursor: pointer; }
.source-health header button:hover { color: var(--atlas-oxide); }
.source-health ul { display: grid; grid-template-columns: repeat(4, 1fr); margin: 0; padding: 0; list-style: none; }
.source-health li { display: grid; grid-template-columns: 8px 1fr; gap: 8px 11px; min-width: 0; padding: 15px 17px; border-right: 1px solid var(--atlas-rule); }
.source-health li:last-child { border-right: 0; }
.health-dot { width: 7px; height: 7px; margin-top: 4px; border-radius: 50%; background: var(--atlas-green); }
.health-dot[data-state="degraded"] { background: var(--atlas-oxide); }
.health-dot[data-state="stale"] { background: var(--atlas-axis); }
.source-health li > div { display: flex; gap: 7px; align-items: baseline; min-width: 0; }
.source-health li strong { color: var(--atlas-ink); font-family: var(--vp-font-family-mono); font-size: 11px; }
.source-health li div span { overflow: hidden; color: var(--atlas-muted); font-size: 10px; text-overflow: ellipsis; white-space: nowrap; }
.source-health li dl { grid-column: 2; display: flex; gap: 16px; margin: 0; }
.source-health li dl div { display: grid; gap: 2px; }
.source-health li dt { color: var(--atlas-axis); font-family: var(--vp-font-family-mono); font-size: 8px; text-transform: uppercase; }
.source-health li dd { margin: 0; color: var(--atlas-muted); font-family: var(--vp-font-family-mono); font-size: 10px; }
.source-detail { grid-column: 2; overflow: hidden; color: var(--atlas-axis); font-size: 10px; text-overflow: ellipsis; white-space: nowrap; }
.explorer-fallback { display: grid; min-height: calc(100vh - 70px); place-items: center; color: var(--atlas-muted); font-family: var(--vp-font-family-mono); font-size: 11px; background: var(--operator-black); }

@keyframes event-register {
  0% { opacity: .35; transform: translateX(0); }
  68% { opacity: 1; transform: translateX(min(68vw, 980px)); }
  69%, 100% { opacity: 0; transform: translateX(min(68vw, 980px)); }
}
@keyframes status-pulse { to { opacity: .3; } }

@media (max-width: 1180px) {
  .operator-commandbar { grid-template-columns: 1fr auto; }
  .operator-commandbar nav { order: 3; grid-column: 1 / -1; min-height: 40px; border-top: 1px solid var(--atlas-rule); }
  .operator-commandbar nav button { height: 40px; }
  .summary-scroll { overflow-x: auto; }
  .summary-scroll > dl { grid-template-columns: repeat(5, 190px); }
  .summary-scroll-cue { display: block; padding: 5px 12px; color: var(--atlas-blue); font-family: var(--vp-font-family-mono); font-size: 8px; letter-spacing: .03em; text-align: right; text-transform: uppercase; border-top: 1px solid var(--atlas-rule-soft); }
  .signal-decision,
  .ledger-grid { grid-template-columns: 1fr; }
  .signal-list,
  .ledger-panel + .ledger-panel { border-right: 0; border-left: 0; }
  .ledger-panel + .ledger-panel { border-top: 1px solid var(--atlas-rule); }
  .source-health ul { grid-template-columns: repeat(2, 1fr); }
  .source-health li:nth-child(2) { border-right: 0; }
  .source-health li:nth-child(-n+2) { border-bottom: 1px solid var(--atlas-rule); }
}

@media (max-width: 820px) {
  .operator-commandbar { position: static; display: flex; flex-wrap: wrap; }
  .operator-identity { min-height: 52px; flex: 1; }
  .operator-identity > div { display: grid; gap: 1px; }
  .operator-session { min-height: 52px; padding: 0 10px; }
  .operator-commandbar nav { order: 3; width: 100%; }
  .operator-commandbar nav button { flex: 1; }
  .operator-boundary { grid-template-columns: 1fr auto; padding: 8px 12px; }
  .operator-boundary > span:nth-child(2) { grid-column: 1 / -1; order: 3; }
  .operator-layout { display: block; border: 0; }
  .operator-rail { display: none; }
  .signal-tape { overflow: hidden; }
  .signal-tape > header { align-items: stretch; flex-direction: column; }
  .tape-metrics { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); width: 100%; }
  .tape-metrics > div { min-width: 0; padding: 3px 8px; }
  .tape-metrics dd { overflow: hidden; font-size: 16px; text-overflow: ellipsis; white-space: nowrap; }
  .tape-metrics > div:first-child { border-left: 0; }
  .tape-scroll-cue { display: block; padding: 5px 12px; color: var(--atlas-blue); font-family: var(--vp-font-family-mono); font-size: 8px; letter-spacing: .03em; text-align: right; text-transform: uppercase; border-bottom: 1px solid var(--atlas-rule-soft); }
  .signal-decision { grid-template-columns: minmax(0, 1fr); }
  .signal-list button { grid-template-columns: 60px 54px 1fr auto; }
  .signal-inspector > dl { grid-template-columns: repeat(2, minmax(0, 1fr)); }
  .lineage { overflow-x: auto; padding-bottom: 6px; }
  .lineage li { white-space: nowrap; }
  .order-layout { grid-template-columns: minmax(160px, .8fr) minmax(170px, 1fr); }
  .source-health ul { grid-template-columns: 1fr; }
  .source-health li,
  .source-health li:nth-child(2),
  .source-health li:nth-child(-n+2) { border-right: 0; border-bottom: 1px solid var(--atlas-rule); }
  .source-health li:last-child { border-bottom: 0; }
  .source-health > header { align-items: flex-start; flex-direction: column; }
}

@media (max-width: 520px) {
  .operator-session { display: grid; grid-template-columns: repeat(2, 1fr); width: 100%; padding: 8px 12px; border-top: 1px solid var(--atlas-rule); }
  .mode-chip,
  .capital-chip,
  .data-chip,
  .command-chip { justify-content: center; padding: 5px 3px; text-align: center; }
  .stream-control { grid-column: 1 / -1; width: 100%; justify-content: center; }
  .summary-scroll > dl { grid-template-columns: repeat(5, 160px); }
  .signal-tape > header { padding: 13px 12px; }
  .signal-list button { grid-template-columns: 54px 50px 1fr auto; gap: 6px; padding: 0 9px; }
  .signal-list button > span:nth-child(3) { font-size: 10px; }
  .signal-inspector { padding: 12px; }
  .signal-inspector h2 { font-size: 16px; }
  .signal-inspector header > strong { font-size: 20px; }
  .order-layout { grid-template-columns: 1fr; }
  .order-list { border-right: 0; border-bottom: 1px solid var(--atlas-rule); }
  .order-list button { grid-template-columns: 35px 1fr auto auto; min-height: 44px; align-items: center; }
  .order-list button code { grid-column: auto; }
  .operator-identity > div > span { max-width: 190px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
}

@media (prefers-reduced-motion: reduce) {
  .tape-play.moving,
  .stream-control > span[data-state="connecting"],
  .stream-control > span[data-state="reconnecting"] { animation: none; }
}
</style>
