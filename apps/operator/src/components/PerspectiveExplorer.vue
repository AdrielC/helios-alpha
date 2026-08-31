<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from "vue";
import type { OperationsSnapshot } from "../operations/operations-port";

type ExplorerScope = "all" | "activity" | "sources";
const props = withDefaults(defineProps<{ snapshot: OperationsSnapshot; scope?: ExplorerScope; compact?: boolean }>(), {
  scope: "all",
  compact: false,
});

const host = ref<HTMLElement | null>(null);
const state = ref<"loading" | "ready" | "error">("loading");
const message = ref("Loading");

let viewer: HTMLElement & {
  load(client: unknown): Promise<void>;
  restore(config: Record<string, unknown>): Promise<void>;
  delete?(): Promise<void>;
};
let table: {
  update(rows: readonly Record<string, unknown>[]): Promise<void>;
  delete(): Promise<void>;
};
let worker: { table(data: unknown, options?: Record<string, unknown>): Promise<unknown>; terminate?(): void };

async function within<T>(promise: Promise<T>, stage: string, timeoutMs = 12_000): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<never>((_, reject) => {
    timer = setTimeout(() => reject(new Error(`${stage} exceeded ${timeoutMs} ms`)), timeoutMs);
  });
  try {
    return await Promise.race([promise, timeout]);
  } finally {
    clearTimeout(timer);
  }
}

async function fetchWasm(url: string, asset: string): Promise<Response> {
  const response = await fetch(url);
  if (!response.ok) throw new Error(`${asset} failed with HTTP ${response.status}`);
  return response;
}

function explorerRows(snapshot: OperationsSnapshot, scope: ExplorerScope): readonly Record<string, unknown>[] {
  const observedAt = new Date(snapshot.observedAt);
  const signalRows = snapshot.signals.map((signal) => ({
    row_id: `signal:${signal.id}`,
    kind: "signal",
    instrument: signal.instrument,
    strategy: signal.hypothesis,
    state: signal.state,
    value: signal.posteriorBps / 100,
    unit: "% posterior",
    event_time: observedAt,
    detail: signal.blocker ?? signal.action,
    sequence: snapshot.sequence,
  }));
  const positionRows = snapshot.positions.map((position) => ({
    row_id: `position:${position.instrument}:${position.strategy}`,
    kind: "position",
    instrument: position.instrument,
    strategy: position.strategy,
    state: Number(position.unrealizedPnlMicros) >= 0 ? "gain" : "loss",
    value: Number(position.unrealizedPnlMicros) / 1_000_000,
    unit: "USD unrealized",
    event_time: observedAt,
    detail: `${Number(position.quantityMicros) / 1_000_000} units`,
    sequence: snapshot.sequence,
  }));
  const orderRows = snapshot.orders.map((order) => ({
    row_id: `order:${order.clientOrderId}`,
    kind: "order",
    instrument: order.instrument,
    strategy: order.strategy,
    state: order.state,
    value: Number(order.filledQuantityMicros) / 1_000_000,
    unit: `${order.side} filled`,
    event_time: observedAt,
    detail: `${order.venue} · ${order.reconciliation}`,
    sequence: snapshot.sequence,
  }));
  const fillRows = snapshot.fills.map((fill) => ({
    row_id: `fill:${fill.executionId}`,
    kind: "fill",
    instrument: fill.instrument,
    strategy: fill.strategy,
    state: `${fill.side} · ${fill.liquidity}`,
    value: Number(fill.quantityMicros) / 1_000_000,
    unit: "units executed",
    event_time: observedAt,
    detail: `${fill.venue} · ${Number(fill.priceMicros) / 1_000_000}`,
    sequence: snapshot.sequence,
  }));
  const sourceRows = snapshot.sources.map((source) => ({
    row_id: `source:${source.name}:${source.channel}`,
    kind: "source",
    instrument: source.name,
    strategy: source.channel,
    state: source.health,
    value: source.lagMs,
    unit: "ms lag",
    event_time: observedAt,
    detail: source.detail,
    sequence: snapshot.sequence,
  }));
  const alertRows = snapshot.alerts.map((alert) => ({
    row_id: `alert:${alert.id}`,
    kind: "alert",
    instrument: alert.relatedEntity?.label ?? snapshot.context.accountName,
    strategy: alert.category,
    state: `${alert.severity} · ${alert.status}`,
    value: alert.severity === "critical" ? 3 : alert.severity === "warning" ? 2 : 1,
    unit: "severity",
    event_time: new Date(alert.updatedAt),
    detail: alert.title,
    sequence: snapshot.sequence,
  }));
  const activityRows = snapshot.activity.map((activity) => ({
    row_id: `activity:${activity.id}`,
    kind: activity.category,
    instrument: activity.entity,
    strategy: activity.stage,
    state: activity.outcome,
    value: activity.sequence,
    unit: "sequence",
    event_time: observedAt,
    detail: activity.source,
    sequence: activity.sequence,
  }));
  if (scope === "activity") return activityRows;
  if (scope === "sources") return sourceRows;
  return [...signalRows, ...positionRows, ...orderRows, ...fillRows, ...sourceRows, ...alertRows, ...activityRows];
}

function scopeTitle(scope: ExplorerScope): string {
  if (scope === "activity") return "Activity explorer";
  if (scope === "sources") return "Source explorer";
  return "Data explorer";
}

onMounted(async () => {
  try {
    // Keep WASM as cacheable binary assets instead of embedding two multi-MB
    // base64 payloads in JavaScript. The entire explorer remains on-demand.
    const [perspective, clientWasmUrl, serverWasmUrl] = await Promise.all([
      import("@perspective-dev/client"),
      import("@perspective-dev/client/dist/wasm/perspective-js.wasm?url"),
      import("@perspective-dev/server/dist/wasm/perspective-server.wasm?url"),
    ]);
    const [clientWasm, serverWasm] = await Promise.all([
      fetchWasm(clientWasmUrl.default, "Perspective client WASM"),
      fetchWasm(serverWasmUrl.default, "Perspective server WASM"),
    ]);
    perspective.default.init_server(serverWasm);
    perspective.default.init_client(clientWasm);

    message.value = "Loading viewer";
    const [perspectiveViewer, viewerWasmUrl, viewerWasmModule] = await Promise.all([
      import("@perspective-dev/viewer"),
      import("@perspective-dev/viewer/dist/wasm/perspective-viewer.wasm?url"),
      import("@perspective-dev/viewer/dist/wasm/perspective-viewer.js"),
      import("@perspective-dev/viewer-datagrid"),
      import("../perspective-darkwater.css"),
    ]);
    const viewerWasm = await fetchWasm(viewerWasmUrl.default, "Perspective viewer WASM");
    await perspectiveViewer.init_client(viewerWasm, viewerWasmModule);
    await customElements.whenDefined("perspective-viewer");
    message.value = "Starting worker";
    worker = (await within(perspective.default.worker(), "Perspective worker startup")) as typeof worker;
    message.value = "Indexing snapshot";
    table = (await within(
      worker.table(explorerRows(props.snapshot, props.scope), {
        index: "row_id",
        name: "helios_operations",
      }),
      "Perspective table creation",
    )) as typeof table;
    viewer = document.createElement("perspective-viewer") as typeof viewer;
    viewer.setAttribute("theme", "Helios Darkwater");
    viewer.setAttribute("aria-label", "Live operations data explorer");
    host.value?.replaceChildren(viewer);
    message.value = "Drawing table";
    await within(viewer.load(worker), "Perspective viewer connection");
    await within(
      viewer.restore({
        table: "helios_operations",
        title: scopeTitle(props.scope),
        plugin: "Datagrid",
        columns: ["kind", "instrument", "strategy", "state", "value", "unit", "detail", "event_time"],
        sort: [["event_time", "desc"]],
        settings: false,
      }),
      "Perspective viewer restore",
    );
    state.value = "ready";
    message.value = "Ready";
  } catch (error) {
    console.error(error);
    state.value = "error";
    message.value = "Explorer unavailable";
  }
});

watch(
  () => [props.snapshot, props.scope] as const,
  async ([snapshot, scope]) => {
    if (state.value === "ready" && table) await table.update(explorerRows(snapshot, scope));
  },
);

onBeforeUnmount(() => {
  host.value?.replaceChildren();
  if (typeof table?.delete === "function") void table.delete().catch(() => undefined);
  if (typeof worker?.terminate === "function") worker.terminate();
});
</script>

<template>
  <section class="perspective-explorer" :class="{ compact }" aria-labelledby="explorer-heading">
    <header>
      <div>
        <h2 id="explorer-heading">{{ scopeTitle(scope) }}</h2>
      </div>
      <div class="perspective-state" :data-state="state" role="status">
        <span aria-hidden="true"></span>
        {{ message }}
      </div>
    </header>
    <div class="perspective-stage">
      <div ref="host" class="perspective-host" :aria-busy="state === 'loading'"></div>
      <div v-if="state === 'loading'" class="perspective-skeleton" aria-hidden="true">
        <span v-for="index in 8" :key="index"></span>
      </div>
    </div>
    <footer>
      <span>{{ snapshot.context.accountName }}</span>
      <span>Sequence {{ snapshot.sequence.toLocaleString() }}</span>
    </footer>
  </section>
</template>

<style scoped>
.perspective-explorer {
  min-height: calc(100vh - 70px);
  color: var(--atlas-ink);
  background: var(--atlas-ground);
}

.perspective-explorer > header,
.perspective-explorer > footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 24px;
  padding: 16px 22px;
  border-bottom: 1px solid var(--atlas-rule);
}

.perspective-explorer h2 {
  margin: 0 0 3px;
  font-size: 18px;
  line-height: 1.2;
  letter-spacing: -0.02em;
}

.perspective-explorer p {
  max-width: 72ch;
  margin: 0;
  color: var(--atlas-muted);
  font-size: 13px;
}

.perspective-state,
.perspective-explorer > footer {
  font-family: var(--vp-font-family-mono);
  font-size: 10px;
  font-variation-settings: "MONO" 1, "CASL" 0;
  letter-spacing: 0.04em;
  text-transform: uppercase;
}

.perspective-state {
  display: flex;
  align-items: center;
  gap: 7px;
  white-space: nowrap;
}

.perspective-state span {
  width: 7px;
  height: 7px;
  background: var(--atlas-blue);
  border-radius: 50%;
}

.perspective-state[data-state="ready"] span {
  background: var(--atlas-green);
}

.perspective-state[data-state="error"] span {
  background: var(--atlas-oxide);
}

.perspective-stage {
  position: relative;
  height: calc(100vh - 190px);
  min-height: 560px;
  padding: 14px;
  background: #05090d;
}

.perspective-host {
  height: 100%;
}

.perspective-host :deep(perspective-viewer) {
  width: 100%;
  height: 100%;
  border: 1px solid var(--atlas-rule);
  background: #05090d;
}

.perspective-skeleton {
  position: absolute;
  inset: 14px;
  display: grid;
  grid-template-rows: repeat(8, 1fr);
  border: 1px solid var(--atlas-rule);
  background: #05090d;
}

.perspective-skeleton span {
  border-bottom: 1px solid var(--atlas-rule-soft);
  background: linear-gradient(90deg, transparent 0 22%, var(--atlas-surface-strong) 22% 42%, transparent 42% 100%);
  background-size: 240% 100%;
  animation: perspective-load 1.2s linear infinite;
}

.perspective-explorer > footer {
  color: var(--atlas-muted);
  border-top: 1px solid var(--atlas-rule);
  border-bottom: 0;
}

.perspective-explorer.compact {
  min-height: calc(100vh - 136px);
}

.perspective-explorer.compact > header {
  display: none;
}

.perspective-explorer.compact .perspective-stage {
  height: calc(100vh - 184px);
  min-height: 520px;
  padding: 10px;
}

.perspective-explorer.compact > footer {
  padding: 10px 14px;
}

@keyframes perspective-load {
  to { background-position: -140% 0; }
}

@media (max-width: 720px) {
  .perspective-explorer > header,
  .perspective-explorer > footer {
    align-items: flex-start;
    flex-direction: column;
    gap: 10px;
  }

  .perspective-stage {
    min-height: 620px;
    padding: 8px;
  }

  .perspective-skeleton { inset: 8px; }
}

@media (prefers-reduced-motion: reduce) {
  .perspective-skeleton span { animation: none; }
}
</style>
