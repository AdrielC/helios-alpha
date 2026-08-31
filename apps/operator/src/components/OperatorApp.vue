<script setup lang="ts">
import { computed, defineAsyncComponent, markRaw, onBeforeUnmount, onMounted, ref, shallowRef, watch } from "vue";
import { createCommandPort, type CommandAuthority } from "../operations/command-port";
import {
  createOperationsPort,
  initialOperationsSnapshot,
  type AlertSeverity,
  type AlertView,
  type OperationsPort,
} from "../operations/operations-port";

const CommandPlane = defineAsyncComponent(() => import("./CommandPlane.vue"));
const OperationsWorkspace = defineAsyncComponent(() => import("./OperationsWorkspace.vue"));
const OperatorHeader = defineAsyncComponent(() => import("./OperatorHeader.vue"));
const PerspectiveExplorer = defineAsyncComponent(() => import("./PerspectiveExplorer.vue"));
type WorkspaceView = "operations" | "alerts" | "control" | "explorer";
type OperationsPane = "overview" | "positions" | "orders" | "signals" | "activity" | "sources";
type ConnectionState = "connecting" | "streaming" | "reconnecting" | "snapshot" | "paused" | "error";
const panes: readonly { id: OperationsPane; label: string; short: string }[] = [
  { id: "overview", label: "Overview", short: "OV" },
  { id: "positions", label: "Positions", short: "PS" },
  { id: "orders", label: "Orders", short: "OR" },
  { id: "signals", label: "Signals", short: "SG" },
  { id: "activity", label: "Activity", short: "AC" },
  { id: "sources", label: "Sources", short: "SR" },
];

const snapshot = shallowRef(initialOperationsSnapshot);
const hasSnapshot = ref(false);
const lastSuccessfulAt = ref<string>();
const failureReason = ref("");
const view = ref<WorkspaceView>("operations");
const activePane = ref<OperationsPane>("overview");
const connection = ref<ConnectionState>("connecting");
const selectedSignalId = ref(initialOperationsSnapshot.signals[0]?.id ?? "");
const selectedOrderId = ref(initialOperationsSnapshot.orders[0]?.clientOrderId ?? "");
const selectedMetricId = ref(initialOperationsSnapshot.metrics[0]?.id ?? "");
const selectedAlertId = ref(initialOperationsSnapshot.alerts[0]?.id ?? "");
const alertFilter = ref<"all" | AlertSeverity>("all");
const railCollapsed = ref(false);
const railWidth = ref(188);
const compactNavigation = ref(false);
const commandAuthority = shallowRef<CommandAuthority>({ state: "unavailable", detail: "Command channel unavailable" });
const commandPort = markRaw(createCommandPort());
let operationsPort: OperationsPort | undefined;
let unsubscribe: (() => void) | undefined;
let compactMedia: MediaQueryList | undefined;
let updateCompactNavigation: (() => void) | undefined;

const stale = computed(() => hasSnapshot.value && ["connecting", "reconnecting", "error"].includes(connection.value));
const runtimeAlerts = computed<readonly AlertView[]>(() => {
  const items: AlertView[] = [];
  const now = snapshot.value.observedAt;
  if (stale.value) items.push({ id: "runtime-stale-view", severity: "critical", status: "open", category: "system", title: "Operations view is stale", detail: failureReason.value || "The live projection is not receiving updates.", openedAt: lastSuccessfulAt.value ?? now, updatedAt: now });
  if (commandAuthority.value.state !== "authenticated") items.push({ id: "runtime-command-channel", severity: "info", status: "open", category: "security", title: "Command channel unavailable", detail: "Control actions remain read-only until an authenticated command service is attached.", openedAt: now, updatedAt: now, relatedEntity: { kind: "control", id: "command-channel", label: "Strategy control" } });
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
const lastObservationLabel = computed(() => !lastSuccessfulAt.value ? "Never" : new Intl.DateTimeFormat("en-US", { hour: "2-digit", minute: "2-digit", second: "2-digit", timeZoneName: "short" }).format(new Date(lastSuccessfulAt.value)));
const connectionLabel = computed(() => ({ connecting: "Connecting", streaming: "Live", reconnecting: "Reconnecting", snapshot: "Snapshot", paused: "Frozen", error: "Offline" }[connection.value]));
const commandLabel = computed(() => commandAuthority.value.state === "authenticated" ? "Command ready" : "Read only");
const announcement = computed(() => !hasSnapshot.value ? connection.value === "error" ? "Operations source unavailable" : "Connecting to operations" : `${connectionLabel.value}. ${activeAlerts.value.length} active alerts. ${snapshot.value.orders.length} active orders.`);
const railStyle = computed(() => ({ "--rail-width": `${railWidth.value}px` }));

function applySnapshot(next: typeof initialOperationsSnapshot): void {
  snapshot.value = next;
  hasSnapshot.value = true;
  lastSuccessfulAt.value = next.observedAt;
  failureReason.value = "";
  if (!next.signals.some((item) => item.id === selectedSignalId.value)) selectedSignalId.value = next.signals[0]?.id ?? "";
  if (!next.orders.some((item) => item.clientOrderId === selectedOrderId.value)) selectedOrderId.value = next.orders[0]?.clientOrderId ?? "";
  if (!next.metrics.some((item) => item.id === selectedMetricId.value)) selectedMetricId.value = next.metrics[0]?.id ?? "";
}
function startSubscription(): void {
  if (!operationsPort) return;
  unsubscribe?.();
  if (!operationsPort.supportsStreaming) { connection.value = "snapshot"; return; }
  connection.value = "connecting";
  unsubscribe = operationsPort.subscribe(applySnapshot, (status) => {
    connection.value = status;
    if (status === "reconnecting") failureReason.value = "The live projection is reconnecting.";
    if (status === "error") failureReason.value = "A malformed operations snapshot was rejected.";
  });
}
async function connectOperations(): Promise<void> {
  unsubscribe?.();
  operationsPort?.close();
  operationsPort = undefined;
  connection.value = "connecting";
  failureReason.value = "";
  try {
    operationsPort = createOperationsPort();
    applySnapshot(await operationsPort.load());
    startSubscription();
  } catch (error) {
    console.error(error);
    failureReason.value = error instanceof Error ? error.message : "The operations source could not be loaded.";
    connection.value = "error";
  }
}
function freeze(): void {
  if (connection.value === "error") void connectOperations();
  else if (connection.value === "paused") startSubscription();
  else if (!["connecting", "snapshot", "reconnecting"].includes(connection.value)) { unsubscribe?.(); unsubscribe = undefined; connection.value = "paused"; }
}
function updateLocation(hash: string, replace = false): void {
  const next = `${window.location.pathname}${window.location.search}#${hash}`;
  if (replace) window.history.replaceState(null, "", next);
  else window.history.pushState(null, "", next);
}
function selectWorkspace(next: WorkspaceView): void { view.value = next; updateLocation(next === "operations" ? activePane.value : next === "explorer" ? "explore" : next); }
function selectPane(next: OperationsPane, updateHistory = true): void { activePane.value = next; view.value = "operations"; if (updateHistory) updateLocation(next); }
function syncLocation(): void {
  const hash = window.location.hash.slice(1).toLowerCase();
  if (panes.some((pane) => pane.id === hash)) selectPane(hash as OperationsPane, false);
  else if (hash === "alerts" || hash === "control") view.value = hash;
  else if (hash === "explore" || hash === "explorer") view.value = "explorer";
  else { view.value = "operations"; activePane.value = "overview"; }
}
function openAlert(alert: AlertView): void { selectedAlertId.value = alert.id; selectWorkspace("alerts"); }
function actOnAlert(alert: AlertView): void {
  if (alert.id === "runtime-stale-view") { void connectOperations(); return; }
  const related = alert.relatedEntity;
  if (!related) { openAlert(alert); return; }
  if (["strategy", "stage", "control", "account"].includes(related.kind)) { selectWorkspace("control"); return; }
  if (related.kind === "signal") { selectedSignalId.value = related.id; selectPane("signals"); }
  else if (related.kind === "source") selectPane("sources");
  else if (related.kind === "order") { selectedOrderId.value = related.id; selectPane("orders"); }
  else if (related.kind === "position") selectPane("positions");
  else selectPane("overview");
}
function alertActionLabel(alert: AlertView): string {
  if (alert.id === "runtime-stale-view") return "Reconnect";
  const kind = alert.relatedEntity?.kind;
  if (kind === "source") return "Open source";
  if (["strategy", "stage", "control", "account"].includes(kind ?? "")) return "Open control";
  if (kind === "signal") return "Open signal";
  if (kind === "order") return "Open order";
  if (kind === "position") return "Open position";
  return "Inspect";
}
function paneCount(pane: OperationsPane): number | undefined {
  if (pane === "positions") return snapshot.value.positions.length;
  if (pane === "orders") return snapshot.value.orders.length;
  if (pane === "signals") return snapshot.value.signals.length;
  if (pane === "activity") return snapshot.value.activity.length;
  if (pane === "sources") return snapshot.value.sources.length;
  return undefined;
}
function relativeTime(timestamp: string): string {
  const delta = Math.max(0, new Date(snapshot.value.observedAt).getTime() - new Date(timestamp).getTime());
  if (delta < 60_000) return `${Math.floor(delta / 1_000)}s ago`;
  if (delta < 3_600_000) return `${Math.floor(delta / 60_000)}m ago`;
  return `${Math.floor(delta / 3_600_000)}h ago`;
}
function focusPane(index: number): void {
  const pane = panes[(index + panes.length) % panes.length];
  selectPane(pane.id);
  requestAnimationFrame(() => document.getElementById(`operations-tab-${pane.id}`)?.focus());
}
function paneKey(event: KeyboardEvent, index: number): void {
  const previous = compactNavigation.value ? "ArrowLeft" : "ArrowUp";
  const next = compactNavigation.value ? "ArrowRight" : "ArrowDown";
  if (event.key === previous) { event.preventDefault(); focusPane(index - 1); }
  else if (event.key === next) { event.preventDefault(); focusPane(index + 1); }
  else if (event.key === "Home") { event.preventDefault(); focusPane(0); }
  else if (event.key === "End") { event.preventDefault(); focusPane(panes.length - 1); }
}
function railKey(event: KeyboardEvent): void {
  if (event.key === "ArrowLeft") railWidth.value = Math.max(156, railWidth.value - 8);
  else if (event.key === "ArrowRight") railWidth.value = Math.min(264, railWidth.value + 8);
  else if (event.key === "Home") railWidth.value = 156;
  else if (event.key === "End") railWidth.value = 264;
  else return;
  event.preventDefault();
}
function resizeRail(event: PointerEvent): void {
  if (event.button !== 0) return;
  const startX = event.clientX;
  const startWidth = railWidth.value;
  const move = (next: PointerEvent) => { railWidth.value = Math.min(264, Math.max(156, startWidth + next.clientX - startX)); };
  const finish = () => { window.removeEventListener("pointermove", move); window.removeEventListener("pointerup", finish); document.body.style.userSelect = ""; };
  document.body.style.userSelect = "none";
  window.addEventListener("pointermove", move);
  window.addEventListener("pointerup", finish, { once: true });
  event.preventDefault();
}

onMounted(() => {
  railCollapsed.value = window.localStorage.getItem("helios:operator:rail") === "collapsed";
  railWidth.value = Math.min(264, Math.max(156, Number(window.localStorage.getItem("helios:operator:rail-width")) || 188));
  compactMedia = window.matchMedia("(max-width: 820px)");
  updateCompactNavigation = () => { compactNavigation.value = Boolean(compactMedia?.matches); };
  compactMedia.addEventListener("change", updateCompactNavigation);
  updateCompactNavigation();
  syncLocation();
  window.addEventListener("popstate", syncLocation);
  window.addEventListener("hashchange", syncLocation);
  void connectOperations();
  void commandPort.describe().then((authority) => { commandAuthority.value = authority; });
});
watch(railCollapsed, (collapsed) => window.localStorage.setItem("helios:operator:rail", collapsed ? "collapsed" : "expanded"));
watch(railWidth, (width) => window.localStorage.setItem("helios:operator:rail-width", String(width)));
onBeforeUnmount(() => {
  unsubscribe?.();
  operationsPort?.close();
  if (compactMedia && updateCompactNavigation) compactMedia.removeEventListener("change", updateCompactNavigation);
  window.removeEventListener("popstate", syncLocation);
  window.removeEventListener("hashchange", syncLocation);
});
</script>

<template>
  <div class="operator-app">
    <Suspense><OperatorHeader :active-alerts="activeAlerts" :command-authority="commandAuthority" :command-label="commandLabel" :connection="connection" :connection-label="connectionLabel" :context="snapshot.context" :data-class="snapshot.dataClass" :has-snapshot="hasSnapshot" :mode="snapshot.mode" :observed-at="snapshot.observedAt" :view="view" @act-alert="actOnAlert" @freeze="freeze" @open-alert="openAlert" @select-view="selectWorkspace($event)"/><template #fallback><div class="header-fallback"><span></span><strong>Helios</strong><small>Connecting</small></div></template></Suspense>
    <p class="sr-only" aria-live="polite" aria-atomic="true">{{ announcement }}</p>
    <section v-if="!hasSnapshot" class="unavailable" :data-state="connection" aria-labelledby="unavailable-heading"><span aria-hidden="true"></span><h1 id="unavailable-heading">{{ connection === "error" ? "Operations unavailable" : "Connecting to operations" }}</h1><p>{{ failureReason || "Waiting for a validated account snapshot." }}</p><small>Last update {{ lastObservationLabel }}</small><button v-if="connection === 'error'" @click="connectOperations">Retry</button></section>

    <template v-else-if="view === 'operations'">
      <div v-if="stale" class="stale-strip" role="alert"><strong>Stale view</strong><span>Last update {{ lastObservationLabel }}</span><button @click="openAlert(runtimeAlerts[0])">Details</button></div>
      <div class="operations-layout" :data-rail="railCollapsed ? 'collapsed' : 'expanded'" :style="railStyle">
        <aside class="entity-rail" role="tablist" :aria-orientation="compactNavigation ? 'horizontal' : 'vertical'" aria-label="Operations panes">
          <button class="rail-toggle" :aria-label="railCollapsed ? 'Expand operations navigation' : 'Collapse operations navigation'" :aria-pressed="railCollapsed" @click="railCollapsed = !railCollapsed"><svg class="rail-mark" viewBox="0 0 28 28" aria-hidden="true"><path class="rail-orbit" d="M5.5 18.5C1.8 12.2 6.3 4.2 13.6 4.2c5 0 9.1 4 9.1 9.1 0 7.6-8.7 12-15 7.4"/><path d="M14 8.5v2M14 17.5v2M8.5 14h2M17.5 14h2"/><rect x="12" y="12" width="4" height="4"/></svg><span>Operations</span><i aria-hidden="true">{{ railCollapsed ? "+" : "−" }}</i></button>
          <button v-for="(pane, index) in panes" :id="`operations-tab-${pane.id}`" :key="pane.id" role="tab" :aria-controls="`operations-panel-${pane.id}`" :aria-selected="activePane === pane.id" :tabindex="activePane === pane.id ? 0 : -1" :title="pane.label" @click="selectPane(pane.id)" @keydown="paneKey($event, index)"><b aria-hidden="true">{{ pane.short }}</b><em>{{ pane.label }}</em><span v-if="paneCount(pane.id) !== undefined">{{ paneCount(pane.id) }}</span></button>
          <div class="runtime-facts"><dl><div><dt>Provider</dt><dd>{{ snapshot.provider }}</dd></div><div><dt>Sequence</dt><dd>{{ snapshot.sequence.toLocaleString() }}</dd></div><div><dt>Checkpoint</dt><dd>{{ snapshot.risk.checkpointAgeMs }}ms</dd></div><div><dt>Clock</dt><dd>{{ snapshot.risk.clockOffsetMs }}ms</dd></div></dl></div>
          <div v-if="!railCollapsed" class="rail-resizer" role="separator" aria-label="Resize operations navigation" aria-orientation="vertical" aria-valuemin="156" aria-valuemax="264" :aria-valuenow="railWidth" tabindex="0" @keydown="railKey" @pointerdown="resizeRail"></div>
        </aside>
        <main :id="`operations-panel-${activePane}`" class="operations-workspace" role="tabpanel" :aria-labelledby="`operations-tab-${activePane}`" tabindex="0"><Suspense><OperationsWorkspace :active-pane="activePane" :authority="commandAuthority" :connection-label="connectionLabel" :port="commandPort" :selected-metric-id="selectedMetricId" :selected-order-id="selectedOrderId" :selected-signal-id="selectedSignalId" :snapshot="snapshot" :stale="stale" @authority="commandAuthority = $event" @explore="selectWorkspace('explorer')" @select-metric="selectedMetricId = $event" @select-order="selectedOrderId = $event" @select-signal="selectedSignalId = $event"/><template #fallback><div class="view-loading">Loading operations</div></template></Suspense></main>
      </div>
    </template>

    <main v-else-if="view === 'alerts'" class="alerts-workspace"><header class="alerts-header"><div><h1>Alerts</h1><span>{{ activeAlerts.length }} active</span></div><div class="alert-filters" role="group" aria-label="Filter alerts"><button :aria-pressed="alertFilter === 'all'" @click="alertFilter = 'all'">All {{ activeAlerts.length }}</button><button :aria-pressed="alertFilter === 'critical'" @click="alertFilter = 'critical'">Critical {{ alertCounts.critical }}</button><button :aria-pressed="alertFilter === 'warning'" @click="alertFilter = 'warning'">Warning {{ alertCounts.warning }}</button><button :aria-pressed="alertFilter === 'info'" @click="alertFilter = 'info'">Info {{ alertCounts.info }}</button></div></header><div class="alerts-layout"><div class="alert-register" role="list"><button v-for="alert in filteredAlerts" :key="alert.id" :class="{ selected: selectedAlert?.id === alert.id }" :data-severity="alert.severity" @click="selectedAlertId = alert.id"><i aria-hidden="true"></i><span><strong>{{ alert.title }}</strong><small>{{ alert.category }} · {{ relativeTime(alert.updatedAt) }}</small></span><b>{{ alert.status }}</b></button><p v-if="filteredAlerts.length === 0" class="empty-state">No alerts in this filter</p></div><article v-if="selectedAlert" class="alert-inspector" :data-severity="selectedAlert.severity"><header><span>{{ selectedAlert.severity }} · {{ selectedAlert.category }}</span><h2>{{ selectedAlert.title }}</h2></header><p>{{ selectedAlert.detail }}</p><dl><div><dt>Status</dt><dd>{{ selectedAlert.status }}</dd></div><div><dt>Opened</dt><dd>{{ new Date(selectedAlert.openedAt).toLocaleString() }}</dd></div><div><dt>Updated</dt><dd>{{ new Date(selectedAlert.updatedAt).toLocaleString() }}</dd></div><div v-if="selectedAlert.relatedEntity"><dt>Related</dt><dd>{{ selectedAlert.relatedEntity.label }}</dd></div></dl><button @click="actOnAlert(selectedAlert)">{{ alertActionLabel(selectedAlert) }}</button></article></div></main>
    <Suspense v-else-if="view === 'control'"><CommandPlane :authority="commandAuthority" :port="commandPort" :snapshot="snapshot" :stale="stale" @authority="commandAuthority = $event"/><template #fallback><div class="view-loading">Loading control</div></template></Suspense>
    <Suspense v-else><PerspectiveExplorer :snapshot="snapshot"/><template #fallback><div class="view-loading">Loading explorer</div></template></Suspense>
  </div>
</template>

<style scoped>
.operator-app{--operator-black:#05090d;min-height:100vh;color:var(--atlas-ink);background:var(--atlas-ground)}.operator-app *,.operator-app *::before,.operator-app *::after{box-sizing:border-box}.operator-app button{-webkit-tap-highlight-color:transparent}.operator-app button:focus-visible{outline:2px solid var(--atlas-blue);outline-offset:-2px}.sr-only{position:absolute;width:1px;height:1px;padding:0;overflow:hidden;clip:rect(0,0,0,0);white-space:nowrap;border:0}.header-fallback{display:flex;min-height:64px;gap:10px;align-items:center;padding:0 16px;border-bottom:1px solid var(--atlas-rule);background:var(--atlas-ground)}.header-fallback>span{width:25px;height:25px;border:1px solid var(--atlas-green)}.header-fallback strong{font-size:16px}.header-fallback small{color:var(--atlas-axis);font:8px var(--vp-font-family-mono);text-transform:uppercase}.stale-strip{display:grid;grid-template-columns:auto 1fr auto;gap:12px;align-items:center;min-height:38px;padding:0 18px;border-bottom:1px solid var(--atlas-oxide);background:color-mix(in srgb,var(--atlas-oxide) 9%,var(--atlas-ground))}.stale-strip strong{color:var(--atlas-oxide);font:10px var(--vp-font-family-mono);text-transform:uppercase}.stale-strip span{font-size:11px}.stale-strip button{color:var(--atlas-ink);font:9px var(--vp-font-family-mono);text-transform:uppercase;border:0;background:transparent;cursor:pointer}.unavailable{display:grid;min-height:calc(100vh - 64px);place-content:center;justify-items:start;padding:8vw;background:var(--operator-black)}.unavailable>span{width:10px;height:10px;margin-bottom:18px;background:var(--atlas-blue);animation:live-pulse 1.8s cubic-bezier(.16,1,.3,1) infinite}.unavailable[data-state=error]>span{background:var(--atlas-oxide)}.unavailable h1{margin:0;font-size:clamp(28px,5vw,44px);letter-spacing:-.035em}.unavailable p{margin:10px 0 0;color:var(--atlas-muted)}.unavailable small{margin-top:8px;color:var(--atlas-axis);font:9px var(--vp-font-family-mono);text-transform:uppercase}.unavailable button{margin-top:22px;padding:8px 12px;color:var(--operator-black);border:0;background:var(--atlas-oxide);cursor:pointer}
.operations-layout{display:grid;grid-template-columns:var(--rail-width) minmax(0,1fr);max-width:1920px;min-height:calc(100vh - 64px);margin:0 auto;border-right:1px solid var(--atlas-rule);border-left:1px solid var(--atlas-rule);transition:grid-template-columns .24s cubic-bezier(.16,1,.3,1)}.operations-layout[data-rail=collapsed]{grid-template-columns:54px minmax(0,1fr)}.entity-rail{position:sticky;z-index:8;top:64px;align-self:start;min-height:calc(100vh - 64px);padding-top:7px;border-right:1px solid var(--atlas-rule);background:var(--atlas-surface-alt)}.rail-toggle{display:grid;width:100%;min-height:43px;grid-template-columns:28px minmax(0,1fr) auto;gap:8px;align-items:center;padding:0 12px;color:var(--atlas-axis);font:8px var(--vp-font-family-mono);text-align:left;text-transform:uppercase;border:0;border-bottom:1px solid var(--atlas-rule-soft);background:transparent;cursor:pointer}.rail-toggle:hover{color:var(--atlas-blue);background:var(--atlas-blue-soft)}.rail-toggle i{font-style:normal}.rail-mark{width:25px;fill:none;stroke:var(--atlas-green);stroke-width:1.25;stroke-linecap:square}.rail-mark rect{fill:var(--atlas-green);stroke:none}.rail-orbit{stroke:var(--atlas-blue);transform-origin:center;animation:orbit-rail 12s linear infinite}.entity-rail>button[role=tab]{display:grid;width:100%;grid-template-columns:24px minmax(0,1fr) auto;gap:8px;align-items:center;min-height:42px;padding:0 13px;color:var(--atlas-muted);font:10px var(--vp-font-family-mono);text-align:left;border:0;border-top:1px solid transparent;border-bottom:1px solid transparent;background:transparent;cursor:pointer}.entity-rail>button[role=tab] b{color:var(--atlas-axis);font:600 8px var(--vp-font-family-mono)}.entity-rail>button[role=tab] em{overflow:hidden;font-style:normal;text-overflow:ellipsis;white-space:nowrap}.entity-rail>button[role=tab] span{color:var(--atlas-axis);font-variant-numeric:tabular-nums}.entity-rail>button[role=tab]:hover,.entity-rail>button[role=tab][aria-selected=true]{color:var(--atlas-blue);border-color:var(--atlas-rule-soft);background:var(--atlas-blue-soft)}.operations-layout[data-rail=collapsed] .rail-toggle{place-items:center;padding:0}.operations-layout[data-rail=collapsed] .rail-toggle span,.operations-layout[data-rail=collapsed] .rail-toggle i,.operations-layout[data-rail=collapsed] .entity-rail>button[role=tab] em,.operations-layout[data-rail=collapsed] .entity-rail>button[role=tab] span,.operations-layout[data-rail=collapsed] .runtime-facts{display:none}.operations-layout[data-rail=collapsed] .entity-rail>button[role=tab]{grid-template-columns:1fr;padding:0;text-align:center}.runtime-facts{position:absolute;right:0;bottom:0;left:0;padding:13px 15px;border-top:1px solid var(--atlas-rule)}.runtime-facts dl{display:grid;gap:7px;margin:0}.runtime-facts dl div{display:grid;grid-template-columns:1fr auto;gap:8px}.runtime-facts dt,.runtime-facts dd{margin:0;color:var(--atlas-axis);font:8px var(--vp-font-family-mono);text-transform:uppercase}.runtime-facts dd{overflow:hidden;max-width:110px;color:var(--atlas-muted);text-overflow:ellipsis;white-space:nowrap}.rail-resizer{position:absolute;z-index:2;top:0;right:-5px;width:9px;height:100%;cursor:col-resize;touch-action:none}.rail-resizer::after{position:absolute;top:0;right:4px;width:1px;height:100%;content:"";background:transparent}.rail-resizer:hover::after,.rail-resizer:focus-visible::after{background:var(--atlas-blue)}.operations-workspace{min-width:0;outline:none}.operations-workspace:focus-visible{box-shadow:inset 0 0 0 2px var(--atlas-blue)}
.alerts-workspace{max-width:1500px;min-height:calc(100vh - 64px);margin:0 auto;border-right:1px solid var(--atlas-rule);border-left:1px solid var(--atlas-rule)}.alerts-header{display:flex;justify-content:space-between;align-items:center;min-height:76px;padding:0 20px;border-bottom:1px solid var(--atlas-rule)}.alerts-header>div:first-child{display:flex;gap:10px;align-items:baseline}.alerts-header h1{margin:0;font-size:28px}.alerts-header span{color:var(--atlas-axis);font:9px var(--vp-font-family-mono);text-transform:uppercase}.alert-filters{display:flex}.alert-filters button{min-height:32px;padding:0 10px;color:var(--atlas-muted);font:9px var(--vp-font-family-mono);border:1px solid var(--atlas-rule);border-right:0;background:transparent;cursor:pointer}.alert-filters button:last-child{border-right:1px solid var(--atlas-rule)}.alert-filters button[aria-pressed=true]{color:var(--atlas-blue);background:var(--atlas-blue-soft)}.alerts-layout{display:grid;grid-template-columns:minmax(310px,.8fr) minmax(0,1.2fr)}.alert-register{min-height:calc(100vh - 140px);border-right:1px solid var(--atlas-rule)}.alert-register button{display:grid;grid-template-columns:8px minmax(0,1fr) auto;gap:10px;align-items:start;width:100%;min-height:70px;padding:14px;text-align:left;border:0;border-bottom:1px solid var(--atlas-rule-soft);background:transparent;cursor:pointer}.alert-register button:hover,.alert-register button.selected{background:var(--atlas-surface-strong)}.alert-register button i{width:7px;height:7px;margin-top:3px;border-radius:50%;background:var(--atlas-blue)}.alert-register button[data-severity=warning] i,.alert-register button[data-severity=critical] i{background:var(--atlas-oxide)}.alert-register button span{display:grid;gap:5px;min-width:0}.alert-register strong{color:var(--atlas-ink);font-size:11px}.alert-register small,.alert-register b{color:var(--atlas-axis);font:8px var(--vp-font-family-mono);text-transform:uppercase}.alert-inspector{align-self:start;max-width:820px;padding:36px 42px}.alert-inspector header span{color:var(--atlas-blue);font:9px var(--vp-font-family-mono);text-transform:uppercase}.alert-inspector[data-severity=warning] header span,.alert-inspector[data-severity=critical] header span{color:var(--atlas-oxide)}.alert-inspector h2{max-width:24ch;margin:8px 0 0;font-size:clamp(28px,3vw,44px);line-height:1.02;letter-spacing:-.03em}.alert-inspector>p{max-width:65ch;margin:20px 0 0;color:var(--atlas-muted);font-size:13px;line-height:1.55}.alert-inspector dl{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));margin:28px 0 0;border-top:1px solid var(--atlas-rule);border-left:1px solid var(--atlas-rule)}.alert-inspector dl>div{padding:12px;border-right:1px solid var(--atlas-rule);border-bottom:1px solid var(--atlas-rule)}.alert-inspector dt{color:var(--atlas-axis);font:8px var(--vp-font-family-mono);text-transform:uppercase}.alert-inspector dd{margin:6px 0 0;color:var(--atlas-ink);font-size:11px}.alert-inspector>button{margin-top:18px;padding:8px 10px;color:var(--atlas-blue);font:9px var(--vp-font-family-mono);text-transform:uppercase;border:1px solid var(--atlas-blue);background:transparent;cursor:pointer}.empty-state,.view-loading{display:grid;min-height:80px;place-items:center;color:var(--atlas-axis);font:9px var(--vp-font-family-mono);text-transform:uppercase}.view-loading{min-height:calc(100vh - 64px);margin:0;background:var(--operator-black)}
@keyframes live-pulse{0%,100%{box-shadow:0 0 0 0 color-mix(in srgb,var(--atlas-green) 0%,transparent)}40%{box-shadow:0 0 0 5px color-mix(in srgb,var(--atlas-green) 13%,transparent)}}@keyframes orbit-rail{to{transform:rotate(360deg)}}@media(max-width:1180px){.entity-rail{top:105px}}@media(max-width:820px){.operations-layout,.operations-layout[data-rail=collapsed]{display:block;border:0}.entity-rail{position:sticky;top:105px;display:flex;min-height:45px;padding:0;overflow-x:auto;border-right:0;border-bottom:1px solid var(--atlas-rule)}.rail-toggle,.runtime-facts,.rail-resizer{display:none!important}.entity-rail>button[role=tab],.operations-layout[data-rail=collapsed] .entity-rail>button[role=tab]{display:grid;min-width:112px;min-height:45px;grid-template-columns:22px auto auto;gap:7px;padding:0 11px;text-align:left}.operations-layout[data-rail=collapsed] .entity-rail>button[role=tab] em,.operations-layout[data-rail=collapsed] .entity-rail>button[role=tab] span{display:block}.entity-rail>button[role=tab][aria-selected=true]{box-shadow:inset 0 -2px 0 var(--atlas-blue)}.alerts-layout{grid-template-columns:1fr}.alert-register{min-height:0;border-right:0;border-bottom:1px solid var(--atlas-rule)}.alerts-header{align-items:flex-start;flex-direction:column;gap:14px;padding:16px}.alert-filters{width:100%;overflow-x:auto}.alert-filters button{flex:1;min-width:max-content}}@media(max-width:720px){.entity-rail{top:0}}@media(max-width:620px){.alert-inspector{padding:26px 18px}.alert-inspector dl{grid-template-columns:1fr}}@media(prefers-reduced-motion:reduce){.rail-orbit,.unavailable>span{animation:none}}
</style>
