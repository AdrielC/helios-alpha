<script setup lang="ts">
import { computed, defineAsyncComponent, onBeforeUnmount, onMounted, ref, shallowRef } from "vue";
import type { CommandAuthority } from "../operations/command-port";
import {
  createOperationsPort,
  initialOperationsSnapshot,
  type AlertSeverity,
  type AlertView,
  type MetricSeriesView,
  type OperationsPort,
} from "../operations/operations-port";
const TimeSeriesChart = defineAsyncComponent(() => import("./TimeSeriesChart.vue"));
const PerspectiveExplorer = defineAsyncComponent(() => import("./PerspectiveExplorer.vue"));
const CommandPlane = defineAsyncComponent(() => import("./CommandPlane.vue"));
type WorkspaceView = "operations" | "alerts" | "control" | "explorer";
type ConnectionState = "connecting" | "streaming" | "reconnecting" | "snapshot" | "paused" | "error";

const snapshot = shallowRef(initialOperationsSnapshot);
const hasSnapshot = ref(false);
const lastSuccessfulAt = ref<string>();
const failureReason = ref("");
const view = ref<WorkspaceView>("operations");
const connection = ref<ConnectionState>("connecting");
const selectedSignalId = ref(initialOperationsSnapshot.signals[0]?.id ?? "");
const inspectedOrderId = ref(initialOperationsSnapshot.orders[0]?.clientOrderId ?? "");
const selectedMetricId = ref(initialOperationsSnapshot.metrics[0]?.id ?? "");
const selectedAlertId = ref(initialOperationsSnapshot.alerts[0]?.id ?? "");
const alertFilter = ref<"all" | AlertSeverity>("all");
const commandAuthority = shallowRef<CommandAuthority>({ state: "unavailable", detail: "Command channel unavailable" });
let port: OperationsPort | undefined;
let unsubscribe: (() => void) | undefined;

const selectedSignal = computed(() => snapshot.value.signals.find((item) => item.id === selectedSignalId.value) ?? snapshot.value.signals[0]);
const selectedOrder = computed(() => snapshot.value.orders.find((item) => item.clientOrderId === inspectedOrderId.value) ?? snapshot.value.orders[0]);
const selectedMetric = computed(() => snapshot.value.metrics.find((item) => item.id === selectedMetricId.value) ?? snapshot.value.metrics[0]);
const selectedSignalSeries = computed<MetricSeriesView | undefined>(() => {
  const signal = selectedSignal.value;
  if (!signal) return undefined;
  const observedAt = new Date(snapshot.value.observedAt).getTime();
  return {
    id: `signal-${signal.id}`,
    label: signal.hypothesis,
    unit: "%",
    tone: signal.state === "blocked" ? "coral" : signal.state === "eligible" ? "green" : "cyan",
    points: signal.trace.map((point) => ({ timestamp: new Date(observedAt + point.offsetSeconds * 1_000).toISOString(), value: point.valueBps / 100 })),
    referenceLines: [{ label: "50%", value: 50, tone: "neutral" }],
  };
});
const isStale = computed(() => hasSnapshot.value && ["connecting", "reconnecting", "error"].includes(connection.value));
const runtimeAlerts = computed<readonly AlertView[]>(() => {
  const items: AlertView[] = [];
  const now = snapshot.value.observedAt;
  if (isStale.value) {
    items.push({
      id: "runtime-stale-view", severity: "critical", status: "open", category: "system",
      title: "Operations view is stale", detail: failureReason.value || "The live projection is not receiving updates.",
      openedAt: lastSuccessfulAt.value ?? now, updatedAt: now,
    });
  }
  if (commandAuthority.value.state !== "authenticated") {
    items.push({
      id: "runtime-command-channel", severity: "info", status: "open", category: "security",
      title: "Command channel unavailable", detail: "Control actions remain read-only until an authenticated command service is attached.",
      openedAt: now, updatedAt: now,
      relatedEntity: { kind: "control", id: "command-channel", label: "Strategy control" },
    });
  }
  return items;
});
const alerts = computed(() => [...runtimeAlerts.value, ...snapshot.value.alerts]);
const activeAlerts = computed(() => alerts.value.filter((alert) => alert.status !== "resolved"));
const filteredAlerts = computed(() => activeAlerts.value.filter((alert) => alertFilter.value === "all" || alert.severity === alertFilter.value));
const selectedAlert = computed(() => alerts.value.find((alert) => alert.id === selectedAlertId.value) ?? filteredAlerts.value[0] ?? alerts.value[0]);
const alertCounts = computed(() => ({
  critical: activeAlerts.value.filter((alert) => alert.severity === "critical").length,
  warning: activeAlerts.value.filter((alert) => alert.severity === "warning").length,
  info: activeAlerts.value.filter((alert) => alert.severity === "info").length,
}));
const grossUtilization = computed(() => {
  const gross = BigInt(snapshot.value.risk.grossExposureMicros);
  const limit = BigInt(snapshot.value.risk.grossLimitMicros);
  return limit === 0n ? 0 : Number((gross * 1_000_000n) / limit) / 1_000_000;
});
const unrealizedTotal = computed(() => snapshot.value.positions.reduce((total, position) => total + BigInt(position.unrealizedPnlMicros), 0n));
const lastObservationLabel = computed(() => !lastSuccessfulAt.value ? "Never" : new Intl.DateTimeFormat("en-US", { hour: "2-digit", minute: "2-digit", second: "2-digit", timeZoneName: "short" }).format(new Date(lastSuccessfulAt.value)));
const connectionLabel = computed(() => ({ connecting: "Connecting", streaming: "Live", reconnecting: "Reconnecting", snapshot: "Snapshot", paused: "Frozen", error: "Offline" }[connection.value]));
const commandLabel = computed(() => commandAuthority.value.state === "authenticated" ? "Command ready" : "Read only");
const contextLine = computed(() => {
  const context = snapshot.value.context;
  return `${context.organizationName} / ${context.workspaceName} / ${context.accountName}`;
});
const operationalAnnouncement = computed(() => !hasSnapshot.value
  ? connection.value === "error" ? "Operations source unavailable" : "Connecting to operations"
  : `${connectionLabel.value}. ${activeAlerts.value.length} active alerts. ${snapshot.value.orders.length} active orders.`);

function applySnapshot(next: typeof initialOperationsSnapshot): void {
  snapshot.value = next;
  hasSnapshot.value = true;
  lastSuccessfulAt.value = next.observedAt;
  failureReason.value = "";
  if (!next.signals.some((item) => item.id === selectedSignalId.value)) selectedSignalId.value = next.signals[0]?.id ?? "";
  if (!next.orders.some((item) => item.clientOrderId === inspectedOrderId.value)) inspectedOrderId.value = next.orders[0]?.clientOrderId ?? "";
  if (!next.metrics.some((item) => item.id === selectedMetricId.value)) selectedMetricId.value = next.metrics[0]?.id ?? "";
}
function startSubscription(): void {
  if (!port) return;
  unsubscribe?.();
  if (!port.supportsStreaming) { connection.value = "snapshot"; return; }
  connection.value = "connecting";
  unsubscribe = port.subscribe(applySnapshot, (status) => {
    connection.value = status;
    if (status === "reconnecting") failureReason.value = "The live projection is reconnecting.";
    if (status === "error") failureReason.value = "A malformed operations snapshot was rejected.";
  });
}
async function connectPort(): Promise<void> {
  unsubscribe?.();
  port?.close();
  unsubscribe = undefined;
  port = undefined;
  connection.value = "connecting";
  failureReason.value = "";
  try {
    port = createOperationsPort();
    applySnapshot(await port.load());
    startSubscription();
  } catch (error) {
    console.error(error);
    failureReason.value = error instanceof Error ? error.message : "The operations source could not be loaded.";
    connection.value = "error";
  }
}
function handleViewFreeze(): void {
  if (connection.value === "error") void connectPort();
  else if (connection.value === "paused") startSubscription();
  else if (!["connecting", "snapshot", "reconnecting"].includes(connection.value)) {
    unsubscribe?.();
    unsubscribe = undefined;
    connection.value = "paused";
  }
}
function applyCommandAuthority(authority: CommandAuthority): void { commandAuthority.value = authority; }
function openAlert(alert: AlertView): void { selectedAlertId.value = alert.id; view.value = "alerts"; }
function openAlertEntity(alert: AlertView): void {
  const related = alert.relatedEntity;
  if (!related) return;
  if (related.kind === "strategy" || related.kind === "control") view.value = "control";
  else {
    view.value = "operations";
    if (related.kind === "signal") selectedSignalId.value = related.id;
    requestAnimationFrame(() => document.getElementById(related.kind === "source" ? "sources" : "signals")?.scrollIntoView({ block: "start" }));
  }
}
function strategyName(id: string): string { return snapshot.value.strategies.find((item) => item.id === id)?.name ?? id; }
function money(micros: string | bigint, signed = false): string {
  const value = BigInt(micros);
  const negative = value < 0n;
  const absolute = negative ? -value : value;
  const roundedCents = (absolute + 5_000n) / 10_000n;
  const dollars = roundedCents / 100n;
  const cents = roundedCents % 100n;
  const prefix = negative ? "-" : signed ? "+" : "";
  const whole = new Intl.NumberFormat("en-US", { maximumFractionDigits: 0 }).format(dollars);
  return `${prefix}$${whole}${dollars < 100n || cents !== 0n ? `.${cents.toString().padStart(2, "0")}` : ""}`;
}
function quantity(micros: string): string {
  const value = BigInt(micros);
  const negative = value < 0n;
  const absolute = negative ? -value : value;
  const whole = new Intl.NumberFormat("en-US", { maximumFractionDigits: 0 }).format(absolute / 1_000_000n);
  const fraction = (absolute % 1_000_000n).toString().padStart(6, "0").replace(/0+$/, "");
  return `${negative ? "-" : ""}${whole}${fraction ? `.${fraction}` : ""}`;
}
function percent(bps: number): string { return `${(bps / 100).toFixed(1)}%`; }
function shortId(id: string): string { return id.length <= 14 ? id : `${id.slice(0, 6)}…${id.slice(-4)}`; }
function relativeTime(timestamp: string): string {
  const delta = Math.max(0, new Date(snapshot.value.observedAt).getTime() - new Date(timestamp).getTime());
  if (delta < 60_000) return `${Math.floor(delta / 1_000)}s ago`;
  if (delta < 3_600_000) return `${Math.floor(delta / 60_000)}m ago`;
  return `${Math.floor(delta / 3_600_000)}h ago`;
}

onMounted(() => void connectPort());
onBeforeUnmount(() => { unsubscribe?.(); port?.close(); });
</script>

<template>
  <div class="operator-console">
    <header class="app-header">
      <div class="product-identity">
        <span class="operator-mark" aria-hidden="true"><i></i><i></i><i></i><i></i></span>
        <div><strong>Helios OMS</strong><span>{{ hasSnapshot ? contextLine : "Connecting" }}</span></div>
      </div>
      <nav aria-label="Workspace views">
        <button :aria-current="view === 'operations' ? 'page' : undefined" @click="view = 'operations'">Operations</button>
        <button :aria-current="view === 'alerts' ? 'page' : undefined" @click="view = 'alerts'">Alerts <span v-if="activeAlerts.length">{{ activeAlerts.length }}</span></button>
        <button :aria-current="view === 'control' ? 'page' : undefined" :disabled="!hasSnapshot" @click="view = 'control'">Control</button>
        <button :aria-current="view === 'explorer' ? 'page' : undefined" :disabled="!hasSnapshot" @click="view = 'explorer'">Explore</button>
      </nav>
      <div class="session-state">
        <span class="status-chip" :data-mode="hasSnapshot ? snapshot.mode : 'pending'">{{ hasSnapshot ? snapshot.mode : "pending" }}</span>
        <span class="status-chip" :data-authority="commandAuthority.state">{{ commandLabel }}</span>
        <span class="live-state" :data-state="connection"><i aria-hidden="true"></i>{{ connectionLabel }}</span>
        <button class="freeze-button" :disabled="['connecting', 'snapshot', 'reconnecting'].includes(connection)" :title="connection === 'paused' ? 'Resume live updates in this browser' : connection === 'error' ? 'Retry the operations connection' : 'Freeze updates in this browser'" @click="handleViewFreeze">
          {{ connection === "paused" ? "Resume live" : connection === "error" ? "Retry" : "Freeze view" }}
        </button>
      </div>
    </header>
    <p class="sr-only" aria-live="polite" aria-atomic="true">{{ operationalAnnouncement }}</p>

    <section v-if="!hasSnapshot" class="unavailable" :data-state="connection" aria-labelledby="unavailable-heading">
      <span aria-hidden="true"></span><h1 id="unavailable-heading">{{ connection === "error" ? "Operations unavailable" : "Connecting to operations" }}</h1>
      <p>{{ failureReason || "Waiting for a validated account snapshot." }}</p><small>Last update {{ lastObservationLabel }}</small>
      <button v-if="connection === 'error'" @click="connectPort">Retry</button>
    </section>

    <template v-else-if="view === 'operations'">
      <div v-if="isStale" class="stale-strip" role="alert"><strong>Stale view</strong><span>Last update {{ lastObservationLabel }}</span><button @click="openAlert(runtimeAlerts[0])">Details</button></div>
      <div class="operations-layout">
        <aside class="entity-rail" aria-label="Operations sections">
          <a href="#telemetry" class="active">Telemetry</a><a href="#positions">Positions <span>{{ snapshot.positions.length }}</span></a><a href="#orders">Orders <span>{{ snapshot.orders.length }}</span></a><a href="#signals">Signals <span>{{ snapshot.signals.length }}</span></a><a href="#activity">Activity <span>{{ snapshot.activity.length }}</span></a><a href="#sources">Sources <span>{{ snapshot.sources.length }}</span></a>
          <div class="runtime-facts"><dl><div><dt>Provider</dt><dd>{{ snapshot.provider }}</dd></div><div><dt>Sequence</dt><dd>{{ snapshot.sequence.toLocaleString() }}</dd></div><div><dt>Checkpoint</dt><dd>{{ snapshot.risk.checkpointAgeMs }}ms</dd></div><div><dt>Clock</dt><dd>{{ snapshot.risk.clockOffsetMs }}ms</dd></div></dl></div>
        </aside>

        <main class="operations-workspace">
          <section class="metric-strip" aria-label="Account summary"><dl>
            <div><dt>Gross exposure</dt><dd>{{ money(snapshot.risk.grossExposureMicros) }}</dd><small>{{ (grossUtilization * 100).toFixed(1) }}% of limit</small></div>
            <div><dt>Unrealized P&amp;L</dt><dd :class="{ negative: unrealizedTotal < 0n }">{{ money(unrealizedTotal, true) }}</dd><small>{{ snapshot.positions.length }} positions</small></div>
            <div><dt>Active orders</dt><dd>{{ snapshot.orders.length }}</dd><small>{{ snapshot.risk.pendingReconciliations }} unreconciled</small></div>
            <div><dt>Orders today</dt><dd>{{ snapshot.risk.dailyOrderCount }} / {{ snapshot.risk.dailyOrderLimit }}</dd><small>Account limit</small></div>
            <div><dt>Source lag</dt><dd :class="{ caution: snapshot.risk.sourceLagMs > 2_000 }">{{ snapshot.risk.sourceLagMs.toLocaleString() }}ms</dd><small>Worst source</small></div>
          </dl></section>

          <section id="telemetry" class="telemetry-section">
            <div class="telemetry-main">
              <header class="section-header"><h1>Portfolio telemetry</h1><div class="metric-tabs" role="tablist" aria-label="Portfolio metric"><button v-for="metric in snapshot.metrics" :key="metric.id" role="tab" :aria-selected="selectedMetric?.id === metric.id" @click="selectedMetricId = metric.id">{{ metric.label }}</button></div></header>
              <TimeSeriesChart v-if="selectedMetric" :series="selectedMetric" /><p v-else class="empty-state">No telemetry series</p>
            </div>
            <aside class="alert-summary" aria-labelledby="active-alerts-heading">
              <header><h2 id="active-alerts-heading">Active alerts</h2><button @click="view = 'alerts'">View all</button></header>
              <button v-for="alert in activeAlerts.slice(0, 4)" :key="alert.id" :data-severity="alert.severity" @click="openAlert(alert)"><i aria-hidden="true"></i><span><strong>{{ alert.title }}</strong><small>{{ alert.category }} · {{ relativeTime(alert.updatedAt) }}</small></span></button>
              <p v-if="activeAlerts.length === 0" class="empty-state">No active alerts</p>
            </aside>
          </section>

          <section id="positions" class="data-section" aria-labelledby="positions-heading">
            <header class="section-header"><h2 id="positions-heading">Positions</h2><span>{{ snapshot.positions.length }} open</span></header>
            <div class="table-scroll" tabindex="0" aria-label="Positions. Scroll horizontally for all columns."><table><thead><tr><th>Instrument</th><th>Strategy</th><th class="number">Quantity</th><th class="number">Average</th><th class="number">Mark</th><th class="number">Market value</th><th class="number">Unrealized</th><th class="number">Age</th></tr></thead><tbody>
              <tr v-for="position in snapshot.positions" :key="`${position.instrument}:${position.strategy}`"><th>{{ position.instrument }}</th><td>{{ strategyName(position.strategy) }}</td><td class="number">{{ quantity(position.quantityMicros) }}</td><td class="number">{{ money(position.averagePriceMicros) }}</td><td class="number">{{ money(position.markPriceMicros) }}</td><td class="number">{{ money(position.marketValueMicros) }}</td><td class="number" :class="Number(position.unrealizedPnlMicros) >= 0 ? 'positive' : 'negative'">{{ money(position.unrealizedPnlMicros, true) }}</td><td class="number">{{ position.freshnessMs }}ms</td></tr>
            </tbody></table></div>
          </section>

          <div class="split-grid">
            <section id="orders" class="data-section" aria-labelledby="orders-heading">
              <header class="section-header"><h2 id="orders-heading">Active orders</h2><span>{{ snapshot.orders.length }}</span></header>
              <div class="order-register"><button v-for="order in snapshot.orders" :key="order.clientOrderId" :class="{ selected: selectedOrder?.clientOrderId === order.clientOrderId }" @click="inspectedOrderId = order.clientOrderId"><span :data-side="order.side">{{ order.side }}</span><strong>{{ order.instrument }}</strong><b>{{ order.state.replaceAll('_', ' ') }}</b><small>{{ strategyName(order.strategy) }}</small><code>{{ shortId(order.clientOrderId) }}</code></button></div>
              <dl v-if="selectedOrder" class="order-detail"><div><dt>Venue</dt><dd>{{ selectedOrder.venue }}</dd></div><div><dt>Quantity</dt><dd>{{ quantity(selectedOrder.quantityMicros) }}</dd></div><div><dt>Filled</dt><dd>{{ quantity(selectedOrder.filledQuantityMicros) }}</dd></div><div><dt>Limit</dt><dd>{{ money(selectedOrder.limitPriceMicros) }}</dd></div><div><dt>Reconciliation</dt><dd>{{ selectedOrder.reconciliation }}</dd></div><div><dt>OMS version</dt><dd>{{ selectedOrder.omsVersion ?? "n/a" }}</dd></div></dl><p v-else class="empty-state">No active orders</p>
            </section>
            <section id="activity" class="data-section" aria-labelledby="activity-heading">
              <header class="section-header"><h2 id="activity-heading">Activity</h2><span>Live</span></header>
              <div class="activity-table" role="table" aria-label="Recent OMS activity"><div v-for="item in snapshot.activity" :key="item.id" role="row" :data-severity="item.severity"><code role="cell">{{ item.occurredAt }}</code><strong role="cell">{{ item.category }}</strong><span role="cell">{{ item.entity }}</span><small role="cell">{{ item.stage }}</small><b role="cell">{{ item.outcome }}</b></div></div>
            </section>
          </div>

          <section id="signals" class="data-section" aria-labelledby="signals-heading">
            <header class="section-header"><h2 id="signals-heading">Signals</h2><span>{{ snapshot.signals.length }} candidates</span></header>
            <div class="signal-layout">
              <div class="signal-register" role="list"><button v-for="signal in snapshot.signals" :key="signal.id" :class="{ selected: selectedSignal?.id === signal.id }" @click="selectedSignalId = signal.id"><i :data-state="signal.state" aria-hidden="true"></i><span><strong>{{ signal.hypothesis }}</strong><small>{{ strategyName(signal.strategyId) }} · {{ signal.instrument }} · {{ signal.horizon }}</small></span><b>{{ percent(signal.posteriorBps) }}</b></button></div>
              <article v-if="selectedSignal" class="signal-inspector"><header><div><h3>{{ selectedSignal.hypothesis }}</h3><span>{{ selectedSignal.instrument }} · {{ selectedSignal.state }}</span></div><strong>{{ percent(selectedSignal.posteriorBps) }}</strong></header><TimeSeriesChart v-if="selectedSignalSeries" :series="selectedSignalSeries" /><dl><div><dt>Trigger</dt><dd>{{ selectedSignal.trigger }}</dd></div><div><dt>Decision cut</dt><dd>{{ selectedSignal.decisionCut }}</dd></div><div><dt>Action</dt><dd>{{ selectedSignal.action }}</dd></div><div v-if="selectedSignal.blocker"><dt>Blocker</dt><dd class="negative">{{ selectedSignal.blocker }}</dd></div></dl><ol aria-label="Signal lineage"><li v-for="step in selectedSignal.lineage" :key="step">{{ step }}</li></ol></article><p v-else class="empty-state">No signal candidates</p>
            </div>
          </section>

          <section id="sources" class="data-section sources-section" aria-labelledby="sources-heading">
            <header class="section-header"><h2 id="sources-heading">Sources</h2><button @click="view = 'explorer'">Explore data</button></header>
            <ul><li v-for="source in snapshot.sources" :key="`${source.name}:${source.channel}`" :data-health="source.health"><i aria-hidden="true"></i><div><strong>{{ source.name }}</strong><span>{{ source.channel }}</span></div><dl><div><dt>Lag</dt><dd>{{ source.lagMs.toLocaleString() }}ms</dd></div><div><dt>Watermark</dt><dd>{{ source.watermark }}</dd></div></dl><small>{{ source.detail }}</small></li></ul>
          </section>
        </main>
      </div>
    </template>

    <main v-else-if="view === 'alerts'" class="alerts-workspace">
      <header class="alerts-header"><div><h1>Alerts</h1><span>{{ activeAlerts.length }} active</span></div><div class="alert-filters" role="group" aria-label="Filter alerts"><button :aria-pressed="alertFilter === 'all'" @click="alertFilter = 'all'">All {{ activeAlerts.length }}</button><button :aria-pressed="alertFilter === 'critical'" @click="alertFilter = 'critical'">Critical {{ alertCounts.critical }}</button><button :aria-pressed="alertFilter === 'warning'" @click="alertFilter = 'warning'">Warning {{ alertCounts.warning }}</button><button :aria-pressed="alertFilter === 'info'" @click="alertFilter = 'info'">Info {{ alertCounts.info }}</button></div></header>
      <div class="alerts-layout"><div class="alert-register" role="list"><button v-for="alert in filteredAlerts" :key="alert.id" :class="{ selected: selectedAlert?.id === alert.id }" :data-severity="alert.severity" @click="selectedAlertId = alert.id"><i aria-hidden="true"></i><span><strong>{{ alert.title }}</strong><small>{{ alert.category }} · {{ relativeTime(alert.updatedAt) }}</small></span><b>{{ alert.status }}</b></button><p v-if="filteredAlerts.length === 0" class="empty-state">No alerts in this filter</p></div>
        <article v-if="selectedAlert" class="alert-inspector" :data-severity="selectedAlert.severity"><header><span>{{ selectedAlert.severity }} · {{ selectedAlert.category }}</span><h2>{{ selectedAlert.title }}</h2></header><p>{{ selectedAlert.detail }}</p><dl><div><dt>Status</dt><dd>{{ selectedAlert.status }}</dd></div><div><dt>Opened</dt><dd>{{ new Date(selectedAlert.openedAt).toLocaleString() }}</dd></div><div><dt>Updated</dt><dd>{{ new Date(selectedAlert.updatedAt).toLocaleString() }}</dd></div><div v-if="selectedAlert.relatedEntity"><dt>Related</dt><dd>{{ selectedAlert.relatedEntity.label }}</dd></div></dl><button v-if="selectedAlert.relatedEntity" @click="openAlertEntity(selectedAlert)">Open {{ selectedAlert.relatedEntity.kind }}</button></article>
      </div>
    </main>

    <Suspense v-else-if="view === 'control'"><CommandPlane :snapshot="snapshot" :stale="isStale" @authority="applyCommandAuthority" /><template #fallback><div class="view-loading">Loading control</div></template></Suspense>
    <Suspense v-else><PerspectiveExplorer :snapshot="snapshot" /><template #fallback><div class="view-loading">Loading explorer</div></template></Suspense>
  </div>
</template>

<style scoped>
.operator-console{--operator-black:#05090d;min-height:100vh;color:var(--atlas-ink);background:var(--atlas-ground)}
.operator-console *,.operator-console *::before,.operator-console *::after{box-sizing:border-box}.operator-console button,.operator-console a{-webkit-tap-highlight-color:transparent}.operator-console button:focus-visible,.operator-console a:focus-visible,.table-scroll:focus-visible{outline:2px solid var(--atlas-blue);outline-offset:-2px}.sr-only{position:absolute;width:1px;height:1px;padding:0;overflow:hidden;clip:rect(0,0,0,0);white-space:nowrap;border:0}
.app-header{position:sticky;z-index:20;top:0;display:grid;grid-template-columns:minmax(270px,1fr) auto minmax(390px,1fr);min-height:64px;border-bottom:1px solid var(--atlas-rule);background:color-mix(in srgb,var(--atlas-ground) 96%,transparent);backdrop-filter:blur(12px)}.product-identity,.session-state,.app-header nav{display:flex;align-items:center}.product-identity{gap:12px;min-width:0;padding:0 20px}.product-identity>div{display:grid;min-width:0;gap:2px}.product-identity strong{font-size:15px;font-weight:670;letter-spacing:-.015em}.product-identity span{overflow:hidden;color:var(--atlas-muted);font-size:10px;text-overflow:ellipsis;white-space:nowrap}.operator-mark{display:grid;grid-template-columns:repeat(2,5px);gap:3px;width:21px;height:21px;flex:0 0 auto;padding:3px;border:1px solid var(--atlas-green)}.operator-mark i{background:var(--atlas-green)}
.app-header nav{align-self:stretch}.app-header nav button{position:relative;height:100%;min-width:78px;padding:0 13px;color:var(--atlas-muted);font:600 10px var(--vp-font-family-mono);letter-spacing:.02em;text-transform:uppercase;border:0;background:transparent;cursor:pointer}.app-header nav button:hover{color:var(--atlas-ink);background:var(--atlas-surface-alt)}.app-header nav button[aria-current=page]{color:var(--atlas-blue);background:var(--atlas-surface-strong)}.app-header nav button[aria-current=page]::after{position:absolute;right:0;bottom:0;left:0;height:2px;content:"";background:var(--atlas-blue)}.app-header nav button span{display:inline-grid;min-width:17px;height:17px;margin-left:4px;place-items:center;color:var(--operator-black);font-size:8px;border-radius:50%;background:var(--atlas-oxide)}.app-header nav button:disabled{opacity:.42;cursor:not-allowed}
.session-state{justify-content:flex-end;gap:7px;padding:0 18px}.status-chip,.live-state{color:var(--atlas-muted);font:9px var(--vp-font-family-mono);letter-spacing:.035em;text-transform:uppercase;white-space:nowrap}.status-chip{padding:5px 7px;border:1px solid var(--atlas-rule)}.status-chip[data-mode=live],.status-chip[data-authority=authenticated]{color:var(--atlas-green-ink);border-color:color-mix(in srgb,var(--atlas-green) 42%,var(--atlas-rule))}.live-state{display:flex;gap:6px;align-items:center}.live-state i{width:7px;height:7px;border-radius:50%;background:var(--atlas-axis)}.live-state[data-state=streaming] i{background:var(--atlas-green);animation:live-pulse 2.4s cubic-bezier(.16,1,.3,1) infinite}.live-state[data-state=reconnecting] i,.live-state[data-state=error] i{background:var(--atlas-oxide)}.freeze-button{min-height:29px;padding:0 9px;color:var(--atlas-blue);font:9px var(--vp-font-family-mono);text-transform:uppercase;border:1px solid var(--atlas-rule);background:transparent;cursor:pointer}.freeze-button:hover{color:var(--atlas-ink);border-color:var(--atlas-blue);background:var(--atlas-blue-soft)}.freeze-button:disabled{color:var(--atlas-axis);cursor:not-allowed}
.stale-strip{display:grid;grid-template-columns:auto 1fr auto;gap:12px;align-items:center;min-height:38px;padding:0 18px;color:var(--atlas-ink);font-size:11px;border-bottom:1px solid var(--atlas-oxide);background:color-mix(in srgb,var(--atlas-oxide) 9%,var(--atlas-ground))}.stale-strip strong{color:var(--atlas-oxide);font:10px var(--vp-font-family-mono);text-transform:uppercase}.stale-strip button{color:var(--atlas-ink);font:9px var(--vp-font-family-mono);text-transform:uppercase;border:0;background:transparent;cursor:pointer}.unavailable{display:grid;min-height:calc(100vh - 64px);place-content:center;justify-items:start;padding:8vw;background:var(--operator-black)}.unavailable>span{width:10px;height:10px;margin-bottom:18px;background:var(--atlas-blue);animation:live-pulse 1.8s cubic-bezier(.16,1,.3,1) infinite}.unavailable[data-state=error]>span{background:var(--atlas-oxide)}.unavailable h1{margin:0;font-size:clamp(28px,5vw,44px);letter-spacing:-.035em}.unavailable p{margin:10px 0 0;color:var(--atlas-muted)}.unavailable small{margin-top:8px;color:var(--atlas-axis);font:9px var(--vp-font-family-mono);text-transform:uppercase}.unavailable button{margin-top:22px;padding:8px 12px;color:var(--operator-black);border:0;background:var(--atlas-oxide);cursor:pointer}
.operations-layout{display:grid;grid-template-columns:188px minmax(0,1fr);max-width:1920px;min-height:calc(100vh - 64px);margin:0 auto;border-right:1px solid var(--atlas-rule);border-left:1px solid var(--atlas-rule)}.entity-rail{position:sticky;top:64px;align-self:start;min-height:calc(100vh - 64px);padding-top:10px;border-right:1px solid var(--atlas-rule);background:var(--atlas-surface-alt)}.entity-rail>a{display:flex;justify-content:space-between;align-items:center;min-height:37px;padding:0 15px;color:var(--atlas-muted);font:10px var(--vp-font-family-mono);text-decoration:none;border-top:1px solid transparent;border-bottom:1px solid transparent}.entity-rail>a span{color:var(--atlas-axis);font-variant-numeric:tabular-nums}.entity-rail>a:hover,.entity-rail>a.active{color:var(--atlas-blue);border-color:var(--atlas-rule-soft);background:var(--atlas-blue-soft)}.runtime-facts{position:absolute;right:0;bottom:0;left:0;padding:13px 15px;border-top:1px solid var(--atlas-rule)}.runtime-facts dl{display:grid;gap:7px;margin:0}.runtime-facts dl div{display:grid;grid-template-columns:1fr auto;gap:8px}.runtime-facts dt,.runtime-facts dd{margin:0;color:var(--atlas-axis);font:8px var(--vp-font-family-mono);text-transform:uppercase}.runtime-facts dd{overflow:hidden;max-width:92px;color:var(--atlas-muted);text-overflow:ellipsis;white-space:nowrap}.operations-workspace{min-width:0}
.metric-strip{overflow-x:auto;border-bottom:1px solid var(--atlas-rule)}.metric-strip dl{display:grid;grid-template-columns:repeat(5,minmax(170px,1fr));min-width:850px;margin:0}.metric-strip dl>div{padding:16px 18px;border-right:1px solid var(--atlas-rule)}.metric-strip dl>div:last-child{border-right:0}.metric-strip dt,.section-header>span{color:var(--atlas-axis);font:9px var(--vp-font-family-mono);letter-spacing:.035em;text-transform:uppercase}.metric-strip dd{margin:7px 0 3px;font:620 20px var(--vp-font-family-mono);font-variant-numeric:tabular-nums;letter-spacing:-.025em}.metric-strip small{color:var(--atlas-muted);font-size:10px}.positive{color:var(--atlas-green-ink)!important}.negative,.caution{color:var(--atlas-oxide)!important}
.telemetry-section{display:grid;grid-template-columns:minmax(0,2fr) minmax(260px,.72fr);border-bottom:1px solid var(--atlas-rule);background:var(--operator-black);scroll-margin-top:70px}.telemetry-main{min-width:0;border-right:1px solid var(--atlas-rule)}.section-header{display:flex;min-height:48px;justify-content:space-between;gap:16px;align-items:center;padding:0 16px;border-bottom:1px solid var(--atlas-rule)}.section-header h1,.section-header h2{margin:0;font-size:18px;font-weight:650;letter-spacing:-.01em}.section-header button{color:var(--atlas-blue);font:9px var(--vp-font-family-mono);text-transform:uppercase;border:0;background:transparent;cursor:pointer}.metric-tabs{display:flex;align-self:stretch;overflow-x:auto}.metric-tabs button{position:relative;min-width:max-content;padding:0 11px;color:var(--atlas-axis);font:9px var(--vp-font-family-mono);border:0;background:transparent;cursor:pointer}.metric-tabs button[aria-selected=true]{color:var(--atlas-blue)}.metric-tabs button[aria-selected=true]::after{position:absolute;right:8px;bottom:0;left:8px;height:2px;content:"";background:var(--atlas-blue)}
.alert-summary>header{display:flex;min-height:48px;justify-content:space-between;align-items:center;padding:0 13px;border-bottom:1px solid var(--atlas-rule)}.alert-summary h2{margin:0;font-size:13px}.alert-summary header button{color:var(--atlas-blue);font:8px var(--vp-font-family-mono);text-transform:uppercase;border:0;background:transparent;cursor:pointer}.alert-summary>button{display:grid;grid-template-columns:8px minmax(0,1fr);gap:9px;align-items:start;width:100%;min-height:59px;padding:11px 13px;text-align:left;border:0;border-bottom:1px solid var(--atlas-rule-soft);background:transparent;cursor:pointer}.alert-summary>button:hover{background:var(--atlas-surface-strong)}.alert-summary>button i,.alert-register button i{width:7px;height:7px;margin-top:3px;border-radius:50%;background:var(--atlas-blue)}.alert-summary>button[data-severity=warning] i,.alert-register button[data-severity=warning] i{background:var(--atlas-oxide)}.alert-summary>button[data-severity=critical] i,.alert-register button[data-severity=critical] i{background:var(--atlas-oxide)}.alert-summary>button span{display:grid;gap:5px;min-width:0}.alert-summary strong{overflow:hidden;color:var(--atlas-ink);font-size:11px;text-overflow:ellipsis;white-space:nowrap}.alert-summary small{color:var(--atlas-axis);font:8px var(--vp-font-family-mono);text-transform:uppercase}
.data-section{scroll-margin-top:70px;border-bottom:1px solid var(--atlas-rule)}.table-scroll{overflow-x:auto}table{width:100%;min-width:980px;border-collapse:collapse}th,td{height:42px;padding:0 13px;color:var(--atlas-muted);font-size:10px;text-align:left;border-right:1px solid var(--atlas-rule-soft);border-bottom:1px solid var(--atlas-rule-soft)}thead th{height:31px;color:var(--atlas-axis);font:8px var(--vp-font-family-mono);letter-spacing:.035em;text-transform:uppercase;background:var(--atlas-surface-alt)}tbody th{color:var(--atlas-ink);font:610 11px var(--vp-font-family-mono)}.number{font-family:var(--vp-font-family-mono);font-variant-numeric:tabular-nums;text-align:right}.split-grid{display:grid;grid-template-columns:1fr 1fr}.split-grid>section:first-child{border-right:1px solid var(--atlas-rule)}
.order-register{display:grid}.order-register>button{display:grid;grid-template-columns:34px 72px auto;gap:4px 9px;align-items:center;min-height:56px;padding:8px 12px;text-align:left;border:0;border-bottom:1px solid var(--atlas-rule-soft);background:transparent;cursor:pointer}.order-register>button:hover,.order-register>button.selected{background:var(--atlas-blue-soft)}.order-register span{color:var(--atlas-oxide);font:8px var(--vp-font-family-mono);text-transform:uppercase}.order-register span[data-side=buy]{color:var(--atlas-green-ink)}.order-register strong{color:var(--atlas-ink);font:10px var(--vp-font-family-mono)}.order-register b{justify-self:end;color:var(--atlas-blue);font:500 8px var(--vp-font-family-mono);text-transform:uppercase}.order-register small{grid-column:2/4;overflow:hidden;color:var(--atlas-muted);font-size:9px;text-overflow:ellipsis;white-space:nowrap}.order-register code{grid-column:1;grid-row:2;color:var(--atlas-axis);font-size:7px}.order-detail{display:grid;grid-template-columns:repeat(3,1fr);margin:0;border-top:1px solid var(--atlas-rule)}.order-detail>div{padding:10px 12px;border-right:1px solid var(--atlas-rule-soft);border-bottom:1px solid var(--atlas-rule-soft)}.order-detail dt{color:var(--atlas-axis);font:8px var(--vp-font-family-mono);text-transform:uppercase}.order-detail dd{margin:5px 0 0;color:var(--atlas-ink);font:9px var(--vp-font-family-mono)}
.activity-table>div{display:grid;grid-template-columns:92px 70px minmax(90px,1fr) minmax(100px,.8fr) auto;gap:9px;align-items:center;min-height:49px;padding:0 12px;border-bottom:1px solid var(--atlas-rule-soft)}.activity-table code,.activity-table small,.activity-table b{color:var(--atlas-axis);font:500 8px var(--vp-font-family-mono)}.activity-table strong{color:var(--atlas-blue);font:8px var(--vp-font-family-mono);text-transform:uppercase}.activity-table span{color:var(--atlas-ink);font-size:10px}.activity-table b{justify-self:end;color:var(--atlas-green-ink);text-transform:uppercase}.activity-table>div[data-severity=warning] b{color:var(--atlas-oxide)}
.signal-layout{display:grid;grid-template-columns:minmax(260px,.62fr) minmax(0,1.38fr);min-height:430px;background:var(--operator-black)}.signal-register{border-right:1px solid var(--atlas-rule)}.signal-register button{display:grid;grid-template-columns:8px minmax(0,1fr) auto;gap:10px;align-items:start;width:100%;min-height:72px;padding:13px 14px;text-align:left;border:0;border-bottom:1px solid var(--atlas-rule-soft);background:transparent;cursor:pointer}.signal-register button:hover,.signal-register button.selected{background:var(--atlas-blue-soft)}.signal-register i{width:7px;height:7px;margin-top:3px;border-radius:50%;background:var(--atlas-blue)}.signal-register i[data-state=eligible]{background:var(--atlas-green)}.signal-register i[data-state=blocked]{background:var(--atlas-oxide)}.signal-register span{display:grid;gap:5px;min-width:0}.signal-register strong{overflow:hidden;color:var(--atlas-ink);font-size:11px;text-overflow:ellipsis;white-space:nowrap}.signal-register small{overflow:hidden;color:var(--atlas-axis);font:8px var(--vp-font-family-mono);text-overflow:ellipsis;white-space:nowrap}.signal-register b{color:var(--atlas-green-ink);font:11px var(--vp-font-family-mono)}.signal-inspector{min-width:0;padding:15px 18px 18px}.signal-inspector>header{display:flex;justify-content:space-between;align-items:flex-start;gap:20px}.signal-inspector h3{margin:0;font-size:16px}.signal-inspector header span{color:var(--atlas-muted);font:8px var(--vp-font-family-mono);text-transform:uppercase}.signal-inspector header>strong{color:var(--atlas-green-ink);font:24px var(--vp-font-family-mono)}.signal-inspector dl{display:grid;grid-template-columns:repeat(4,minmax(0,1fr));margin:4px 0 0;border:1px solid var(--atlas-rule)}.signal-inspector dl>div{min-width:0;padding:9px;border-right:1px solid var(--atlas-rule-soft)}.signal-inspector dt{color:var(--atlas-axis);font:8px var(--vp-font-family-mono);text-transform:uppercase}.signal-inspector dd{overflow:hidden;margin:5px 0 0;color:var(--atlas-muted);font-size:9px;text-overflow:ellipsis;white-space:nowrap}.signal-inspector ol{display:flex;flex-wrap:wrap;margin:12px 0 0;padding:0;list-style:none}.signal-inspector li{color:var(--atlas-blue);font:8px var(--vp-font-family-mono)}.signal-inspector li:not(:last-child)::after{margin:0 7px;color:var(--atlas-axis);content:"→"}
.sources-section ul{display:grid;grid-template-columns:repeat(auto-fit,minmax(250px,1fr));margin:0;padding:0;list-style:none}.sources-section li{display:grid;grid-template-columns:8px minmax(0,1fr) auto;gap:8px 10px;align-items:center;min-height:86px;padding:12px 14px;border-right:1px solid var(--atlas-rule-soft);border-bottom:1px solid var(--atlas-rule-soft)}.sources-section li>i{width:7px;height:7px;border-radius:50%;background:var(--atlas-green)}.sources-section li[data-health=degraded]>i{background:var(--atlas-oxide)}.sources-section li[data-health=stale]>i{background:var(--atlas-oxide)}.sources-section li>div{display:grid;gap:3px}.sources-section strong{font-size:11px}.sources-section span,.sources-section small{color:var(--atlas-muted);font:8px var(--vp-font-family-mono)}.sources-section li>dl{display:grid;gap:4px;margin:0}.sources-section li>dl div{display:flex;justify-content:space-between;gap:8px}.sources-section dt,.sources-section dd{margin:0;color:var(--atlas-axis);font:7px var(--vp-font-family-mono);text-transform:uppercase}.sources-section dd{color:var(--atlas-muted)}.sources-section li>small{grid-column:2/-1}.empty-state{display:grid;min-height:80px;place-items:center;margin:0;color:var(--atlas-axis);font:9px var(--vp-font-family-mono);text-transform:uppercase}
.alerts-workspace{max-width:1500px;min-height:calc(100vh - 64px);margin:0 auto;border-right:1px solid var(--atlas-rule);border-left:1px solid var(--atlas-rule)}.alerts-header{display:flex;justify-content:space-between;align-items:center;min-height:76px;padding:0 20px;border-bottom:1px solid var(--atlas-rule)}.alerts-header>div:first-child{display:flex;gap:10px;align-items:baseline}.alerts-header h1{margin:0;font-size:28px}.alerts-header span{color:var(--atlas-axis);font:9px var(--vp-font-family-mono);text-transform:uppercase}.alert-filters{display:flex}.alert-filters button{min-height:32px;padding:0 10px;color:var(--atlas-muted);font:9px var(--vp-font-family-mono);border:1px solid var(--atlas-rule);border-right:0;background:transparent;cursor:pointer}.alert-filters button:last-child{border-right:1px solid var(--atlas-rule)}.alert-filters button[aria-pressed=true]{color:var(--atlas-blue);background:var(--atlas-blue-soft)}.alerts-layout{display:grid;grid-template-columns:minmax(310px,.8fr) minmax(0,1.2fr)}.alert-register{min-height:calc(100vh - 140px);border-right:1px solid var(--atlas-rule)}.alert-register button{display:grid;grid-template-columns:8px minmax(0,1fr) auto;gap:10px;align-items:start;width:100%;min-height:70px;padding:14px;text-align:left;border:0;border-bottom:1px solid var(--atlas-rule-soft);background:transparent;cursor:pointer}.alert-register button:hover,.alert-register button.selected{background:var(--atlas-surface-strong)}.alert-register button span{display:grid;gap:5px;min-width:0}.alert-register strong{color:var(--atlas-ink);font-size:11px}.alert-register small,.alert-register b{color:var(--atlas-axis);font:8px var(--vp-font-family-mono);text-transform:uppercase}.alert-register b{font-weight:500}.alert-inspector{align-self:start;max-width:820px;padding:36px 42px}.alert-inspector header span{color:var(--atlas-blue);font:9px var(--vp-font-family-mono);text-transform:uppercase}.alert-inspector[data-severity=warning] header span{color:var(--atlas-oxide)}.alert-inspector[data-severity=critical] header span{color:var(--atlas-oxide)}.alert-inspector h2{max-width:24ch;margin:8px 0 0;font-size:clamp(28px,3vw,44px);line-height:1.02;letter-spacing:-.03em}.alert-inspector>p{max-width:65ch;margin:20px 0 0;color:var(--atlas-muted);font-size:13px;line-height:1.55}.alert-inspector dl{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));margin:28px 0 0;border-top:1px solid var(--atlas-rule);border-left:1px solid var(--atlas-rule)}.alert-inspector dl>div{padding:12px;border-right:1px solid var(--atlas-rule);border-bottom:1px solid var(--atlas-rule)}.alert-inspector dt{color:var(--atlas-axis);font:8px var(--vp-font-family-mono);text-transform:uppercase}.alert-inspector dd{margin:6px 0 0;color:var(--atlas-ink);font-size:11px}.alert-inspector>button{margin-top:18px;padding:8px 10px;color:var(--atlas-blue);font:9px var(--vp-font-family-mono);text-transform:uppercase;border:1px solid var(--atlas-blue);background:transparent;cursor:pointer}.view-loading{display:grid;min-height:calc(100vh - 64px);place-items:center;color:var(--atlas-muted);font:10px var(--vp-font-family-mono);text-transform:uppercase;background:var(--operator-black)}
@keyframes live-pulse{0%,100%{box-shadow:0 0 0 0 color-mix(in srgb,var(--atlas-green) 0%,transparent)}40%{box-shadow:0 0 0 5px color-mix(in srgb,var(--atlas-green) 13%,transparent)}}
@media(max-width:1180px){.app-header{grid-template-columns:1fr auto}.app-header nav{order:3;grid-column:1/-1;height:40px;border-top:1px solid var(--atlas-rule)}.app-header nav button{flex:1}.session-state{min-height:64px}.entity-rail{top:105px}.telemetry-section{grid-template-columns:1fr}.telemetry-main{border-right:0;border-bottom:1px solid var(--atlas-rule)}.alert-summary{display:grid;grid-template-columns:repeat(2,1fr)}.alert-summary>header{grid-column:1/-1}}
@media(max-width:820px){.operations-layout{display:block;border:0}.entity-rail{display:none}.split-grid,.signal-layout,.alerts-layout{grid-template-columns:1fr}.split-grid>section:first-child,.signal-register,.alert-register{border-right:0;border-bottom:1px solid var(--atlas-rule)}.alert-register{min-height:0}.signal-inspector dl{grid-template-columns:repeat(2,1fr)}.alerts-header{align-items:flex-start;flex-direction:column;gap:14px;padding:16px}.alert-filters{width:100%;overflow-x:auto}.alert-filters button{flex:1;min-width:max-content}}
@media(max-width:620px){.app-header{position:static;display:flex;flex-wrap:wrap}.product-identity{width:100%;min-height:54px;padding:0 12px;border-bottom:1px solid var(--atlas-rule)}.product-identity>div{max-width:calc(100vw - 58px)}.session-state{order:2;display:grid;grid-template-columns:repeat(2,1fr);width:100%;min-height:auto;padding:8px 12px}.status-chip{display:none}.live-state{min-height:30px}.freeze-button{width:100%}.app-header nav{order:3;width:100%;overflow-x:auto}.app-header nav button{min-width:92px}.metric-strip dl{min-width:760px}.telemetry-section{scroll-margin-top:0}.section-header{align-items:flex-start;flex-direction:column;height:auto;min-height:60px;padding:12px}.metric-tabs{width:100%}.metric-tabs button{padding:7px 10px}.alert-summary{grid-template-columns:1fr}.alert-summary>header{grid-column:1}.signal-inspector{padding:14px 12px}.signal-inspector>header{align-items:flex-end}.signal-inspector dl{grid-template-columns:1fr}.signal-inspector dl>div{border-right:0;border-bottom:1px solid var(--atlas-rule-soft)}.order-detail{grid-template-columns:repeat(2,1fr)}.activity-table{overflow-x:auto}.activity-table>div{min-width:650px}.alert-inspector{padding:26px 18px}.alert-inspector dl{grid-template-columns:1fr}}
@media(prefers-reduced-motion:reduce){.live-state[data-state=streaming] i,.unavailable>span{animation:none}}
</style>
