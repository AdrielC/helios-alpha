<script setup lang="ts">
import { computed, defineAsyncComponent, onMounted, ref, watch } from "vue";
import type { CommandAuthority, CommandPort } from "../operations/command-port";
import type { OperationsSnapshot, PositionView, SourceView } from "../operations/operations-port";
import type { ForecastBundle, TimeSeriesDescriptor, TimeSeriesPort } from "../operations/time-series-port";
import type { InvestigationPort } from "../operations/investigation-port";

type OperationsPane = "overview" | "positions" | "orders" | "forecasts" | "activity" | "sources";
const props = defineProps<{
  snapshot: OperationsSnapshot;
  activePane: OperationsPane;
  connectionLabel: string;
  stale: boolean;
  authority: CommandAuthority;
  port: CommandPort;
  timeSeriesPort: TimeSeriesPort;
  investigationPort: InvestigationPort;
  selectedSignalId: string;
  selectedForecastId: string;
  selectedOrderId: string;
}>();
const emit = defineEmits<{
  authority: [authority: CommandAuthority];
  explore: [];
  openForecast: [id: string];
  selectSignal: [id: string];
  selectOrder: [id: string];
}>();

const NewOrderTicket = defineAsyncComponent(() => import("./NewOrderTicket.vue"));
const PerspectiveExplorer = defineAsyncComponent(() => import("./PerspectiveExplorer.vue"));
const SynchronizedEvidenceTimeline = defineAsyncComponent(() => import("./SynchronizedEvidenceTimeline.vue"));

const forecastBundles = ref<readonly ForecastBundle[]>([]);
const seriesCatalog = ref<readonly TimeSeriesDescriptor[]>([]);
const selectedBundleId = ref("");
const selectedSourceKey = ref("");
const orderDialog = ref<HTMLDialogElement | null>(null);
const orderInstrument = ref("");
const orderTicketRevision = ref(0);
let researchLoadGeneration = 0;

const selectedOrder = computed(() => props.snapshot.orders.find((item) => item.clientOrderId === props.selectedOrderId) ?? props.snapshot.orders[0]);
const selectedBundle = computed(() => forecastBundles.value.find((bundle) => bundle.id === selectedBundleId.value) ?? forecastBundles.value[0]);
const selectedBundleSignals = computed(() => {
  const bundle = selectedBundle.value;
  if (!bundle) return props.snapshot.signals;
  return props.snapshot.signals.filter((signal) => bundle.strategyIds.includes(signal.strategyId));
});
const selectedSignal = computed(() => selectedBundleSignals.value.find((item) => item.id === props.selectedSignalId) ?? selectedBundleSignals.value[0]);
const selectedSource = computed(() => props.snapshot.sources.find((source) => sourceKey(source) === selectedSourceKey.value) ?? props.snapshot.sources[0]);
const selectedSourceSeries = computed(() => seriesCatalog.value.filter((descriptor) => selectedSource.value && descriptor.sourceNames?.includes(selectedSource.value.name)));
const unrealizedTotal = computed(() => props.snapshot.positions.reduce((total, position) => total + BigInt(position.unrealizedPnlMicros), 0n));
const marketValueTotal = computed(() => props.snapshot.positions.reduce((total, position) => total + absolute(BigInt(position.marketValueMicros)), 0n));
const dayPnlTotal = computed(() => props.snapshot.positions.reduce((total, position) => total + BigInt(position.dayPnlMicros ?? "0"), 0n));
const grossUtilization = computed(() => ratioPercent(props.snapshot.risk.grossExposureMicros, BigInt(props.snapshot.risk.grossLimitMicros)));

function strategyName(id: string): string { return props.snapshot.strategies.find((item) => item.id === id)?.name ?? id; }
function descriptorName(id: string): string { return seriesCatalog.value.find((item) => item.id === id)?.label ?? id; }
function absolute(value: bigint): bigint { return value < 0n ? -value : value; }
function money(micros: string | bigint, signed = false): string {
  const value = BigInt(micros);
  const negative = value < 0n;
  const magnitude = negative ? -value : value;
  const roundedCents = (magnitude + 5_000n) / 10_000n;
  const dollars = roundedCents / 100n;
  const cents = roundedCents % 100n;
  const prefix = negative ? "-" : signed ? "+" : "";
  return `${prefix}$${new Intl.NumberFormat("en-US", { maximumFractionDigits: 0 }).format(dollars)}${dollars < 100n || cents !== 0n ? `.${cents.toString().padStart(2, "0")}` : ""}`;
}
function quantity(micros: string): string {
  const value = BigInt(micros);
  const negative = value < 0n;
  const magnitude = negative ? -value : value;
  const fraction = (magnitude % 1_000_000n).toString().padStart(6, "0").replace(/0+$/, "");
  return `${negative ? "-" : ""}${new Intl.NumberFormat("en-US", { maximumFractionDigits: 0 }).format(magnitude / 1_000_000n)}${fraction ? `.${fraction}` : ""}`;
}
function percent(bps: number): string { return `${(bps / 100).toFixed(1)}%`; }
function freshness(seconds: number): string {
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3_600) return `${Math.round(seconds / 60)}m`;
  if (seconds < 86_400) return `${Math.round(seconds / 3_600)}h`;
  return `${Math.round(seconds / 86_400)}d`;
}
function ratioPercent(numerator: string, denominator: bigint): number {
  if (denominator === 0n) return 0;
  return Number((BigInt(numerator) * 1_000_000n) / denominator) / 10_000;
}
function positionCostMicros(position: PositionView): bigint { return (absolute(BigInt(position.quantityMicros)) * BigInt(position.averagePriceMicros)) / 1_000_000n; }
function positionOpenBps(position: PositionView): number { return ratioPercent(position.unrealizedPnlMicros, positionCostMicros(position)) * 100; }
function shortId(id: string): string { return id.length <= 14 ? id : `${id.slice(0, 6)}…${id.slice(-4)}`; }
function sourceKey(source: SourceView): string { return `${source.name}:${source.channel}`; }
function chooseBundle(id: string): void {
  selectedBundleId.value = id;
  const bundle = forecastBundles.value.find((item) => item.id === id);
  const signal = props.snapshot.signals.find((candidate) => bundle?.strategyIds.includes(candidate.strategyId));
  if (signal) emit("selectSignal", signal.id);
}
function openOrderTicket(instrument = ""): void {
  orderInstrument.value = instrument;
  orderTicketRevision.value += 1;
  orderDialog.value?.showModal();
}
function closeOrderTicket(): void { orderDialog.value?.close(); }

async function loadResearchContext(): Promise<void> {
  const generation = ++researchLoadGeneration;
  const context = props.snapshot.context;
  const [bundles, catalog] = await Promise.all([
    props.timeSeriesPort.forecastBundles(context),
    props.timeSeriesPort.catalog(context),
  ]);
  if (generation !== researchLoadGeneration || props.snapshot.context.accountId !== context.accountId) return;
  forecastBundles.value = bundles;
  seriesCatalog.value = catalog;
  selectedBundleId.value = props.selectedForecastId && bundles.some((bundle) => bundle.id === props.selectedForecastId) ? props.selectedForecastId : bundles[0]?.id ?? "";
  selectedSourceKey.value = props.snapshot.sources[0] ? sourceKey(props.snapshot.sources[0]) : "";
}

onMounted(() => { void loadResearchContext(); });
watch(() => props.snapshot.context.accountId, () => { void loadResearchContext(); });
</script>

<template>
  <div class="operations-pane">
    <template v-if="activePane === 'overview'">
      <SynchronizedEvidenceTimeline :investigation-port="investigationPort" :port="timeSeriesPort" :selected-forecast-id="selectedForecastId" :selected-order-id="selectedOrderId" :snapshot="snapshot" @select-order="emit('selectOrder', $event)"/>
    </template>

    <section v-else-if="activePane === 'positions'" class="pane-section" aria-labelledby="positions-heading">
      <header class="pane-heading"><div><h1 id="positions-heading">Positions</h1><span>{{ snapshot.positions.length }} open</span></div><div class="heading-actions"><time :datetime="snapshot.observedAt">Marks {{ connectionLabel.toLowerCase() }}</time><button @click="openOrderTicket()">New order</button></div></header>
      <div class="position-summary"><dl><div><dt>Market value</dt><dd>{{ money(marketValueTotal) }}</dd></div><div><dt>Open P&amp;L</dt><dd :class="unrealizedTotal >= 0n ? 'positive' : 'negative'">{{ money(unrealizedTotal, true) }}</dd></div><div><dt>Day P&amp;L</dt><dd :class="dayPnlTotal >= 0n ? 'positive' : 'negative'">{{ money(dayPnlTotal, true) }}</dd></div><div><dt>Gross utilization</dt><dd>{{ grossUtilization.toFixed(1) }}%</dd></div></dl></div>
      <div class="table-scroll" tabindex="0" aria-label="Positions. Scroll horizontally for value and change columns."><table class="positions-table"><thead><tr><th>Instrument</th><th>Strategy</th><th class="number">Quantity</th><th class="number">Average</th><th class="number">Mark</th><th class="number">Market value</th><th class="number">Open P&amp;L</th><th class="number">Open %</th><th class="number">Day P&amp;L</th><th class="number">Day %</th><th class="number">Mark age</th><th><span class="sr-only">Actions</span></th></tr></thead><tbody><tr v-for="position in snapshot.positions" :key="`${position.instrument}:${position.strategy}`"><th>{{ position.instrument }}</th><td>{{ strategyName(position.strategy) }}</td><td class="number">{{ quantity(position.quantityMicros) }}</td><td class="number">{{ money(position.averagePriceMicros) }}</td><td class="number">{{ money(position.markPriceMicros) }}</td><td class="number">{{ money(position.marketValueMicros) }}</td><td class="number" :class="BigInt(position.unrealizedPnlMicros) >= 0n ? 'positive' : 'negative'">{{ money(position.unrealizedPnlMicros, true) }}</td><td class="number" :class="positionOpenBps(position) >= 0 ? 'positive' : 'negative'">{{ percent(positionOpenBps(position)) }}</td><td class="number" :class="BigInt(position.dayPnlMicros ?? '0') >= 0n ? 'positive' : 'negative'">{{ money(position.dayPnlMicros ?? '0', true) }}</td><td class="number" :class="(position.dayChangeBps ?? 0) >= 0 ? 'positive' : 'negative'">{{ percent(position.dayChangeBps ?? 0) }}</td><td class="number">{{ position.freshnessMs }}ms</td><td><button class="row-action" @click="openOrderTicket(position.instrument)">Trade</button></td></tr></tbody></table></div>
    </section>

    <section v-else-if="activePane === 'orders'" class="pane-section" aria-labelledby="orders-heading">
      <header class="pane-heading"><div><h1 id="orders-heading">Orders</h1><span>{{ snapshot.orders.length }} active</span></div><div class="heading-actions"><time :datetime="snapshot.observedAt">OMS {{ snapshot.sequence.toLocaleString() }}</time><button @click="openOrderTicket()">New order</button></div></header>
      <div class="orders-grid"><section class="active-orders"><header class="section-header"><h2>Order register</h2><span>{{ snapshot.orders.length }}</span></header><div class="order-register"><button v-for="order in snapshot.orders" :key="order.clientOrderId" :class="{ selected: selectedOrder?.clientOrderId === order.clientOrderId }" @click="emit('selectOrder', order.clientOrderId)"><span :data-side="order.side">{{ order.side }}</span><strong>{{ order.instrument }}</strong><b>{{ order.state.replaceAll('_', ' ') }}</b><small>{{ strategyName(order.strategy) }} · {{ order.venue }}</small><code>{{ shortId(order.clientOrderId) }}</code></button></div></section><section class="order-inspector"><header class="section-header"><h2>Order detail</h2><span v-if="selectedOrder">{{ shortId(selectedOrder.clientOrderId) }}</span></header><dl v-if="selectedOrder" class="order-detail"><div><dt>Venue</dt><dd>{{ selectedOrder.venue }}</dd></div><div><dt>Quantity</dt><dd>{{ quantity(selectedOrder.quantityMicros) }}</dd></div><div><dt>Filled</dt><dd>{{ quantity(selectedOrder.filledQuantityMicros) }}</dd></div><div><dt>Limit</dt><dd>{{ money(selectedOrder.limitPriceMicros) }}</dd></div><div><dt>Reconciliation</dt><dd>{{ selectedOrder.reconciliation }}</dd></div><div><dt>OMS version</dt><dd>{{ selectedOrder.omsVersion ?? "n/a" }}</dd></div><div><dt>Time in force</dt><dd>{{ selectedOrder.timeInForce?.replaceAll('_', ' ') ?? 'day' }}</dd></div><div><dt>Submitted</dt><dd>{{ selectedOrder.submittedAt }}</dd></div></dl><p v-else class="empty-state">No active orders</p></section></div>
      <section class="fills-section"><header class="section-header"><h2>Executions</h2><span>{{ snapshot.fills.length }}</span></header><div class="table-scroll" tabindex="0"><table><thead><tr><th>Execution</th><th>Instrument</th><th>Side</th><th class="number">Quantity</th><th class="number">Price</th><th>Venue</th><th>Strategy</th><th>Liquidity</th><th>Time</th></tr></thead><tbody><tr v-for="fill in snapshot.fills" :key="fill.executionId"><th>{{ shortId(fill.executionId) }}</th><td>{{ fill.instrument }}</td><td :class="fill.side === 'buy' ? 'positive' : 'negative'">{{ fill.side }}</td><td class="number">{{ quantity(fill.quantityMicros) }}</td><td class="number">{{ money(fill.priceMicros) }}</td><td>{{ fill.venue }}</td><td>{{ strategyName(fill.strategy) }}</td><td>{{ fill.liquidity }}</td><td>{{ fill.executedAt }}</td></tr></tbody></table></div></section>
    </section>

    <section v-else-if="activePane === 'forecasts'" class="pane-section" aria-labelledby="forecasts-heading">
      <header class="pane-heading"><div><h1 id="forecasts-heading">Forecasts</h1><span>{{ forecastBundles.length }} bundles</span></div><time :datetime="snapshot.observedAt">Research evidence</time></header>
      <div class="forecast-layout"><nav class="forecast-register" aria-label="Forecast bundles"><button v-for="bundle in forecastBundles" :key="bundle.id" :class="{ selected: selectedBundle?.id === bundle.id }" @click="chooseBundle(bundle.id)"><span :data-state="bundle.state"></span><div><strong>{{ bundle.label }}</strong><small>{{ bundle.horizon }} · v{{ bundle.bundleVersion }} · {{ bundle.seriesIds.length }} inputs</small></div><b>{{ bundle.state }}</b></button></nav><article v-if="selectedBundle" class="forecast-inspector"><header><div><span>Forecast bundle · {{ selectedBundle.horizon }}</span><h2>{{ selectedBundle.label }}</h2><p>{{ selectedBundle.thesis }}</p></div><button @click="emit('openForecast', selectedBundle.id)">Open in Market Atlas</button></header><section class="forecast-inputs"><div><h3>Observation contract</h3><p>Inputs are ordered, source-bound, and freshness-gated.</p></div><ul><li v-for="input in selectedBundle.inputContract" :key="input.seriesId" :title="input.role"><span :style="{ background: seriesCatalog.find((item) => item.id === input.seriesId)?.color }"></span><div><strong>{{ descriptorName(input.seriesId) }}</strong><small>{{ input.role }}</small></div><b>{{ input.required ? 'Required' : 'Optional' }} · {{ freshness(input.maxAgeSeconds) }}</b></li></ul></section><section class="forecast-candidates"><header class="section-header"><h3>Decision candidates</h3><span>{{ selectedBundleSignals.length }}</span></header><button v-for="signal in selectedBundleSignals" :key="signal.id" :class="{ selected: selectedSignal?.id === signal.id }" @click="emit('selectSignal', signal.id)"><i :data-state="signal.state"></i><span><strong>{{ signal.hypothesis }}</strong><small>{{ signal.instrument }} · {{ signal.horizon }} · {{ signal.trigger }}</small></span><b>{{ percent(signal.posteriorBps) }}</b></button><p v-if="selectedBundleSignals.length === 0" class="empty-state">No decision candidates for this bundle</p></section><dl v-if="selectedSignal" class="forecast-decision"><div><dt>Decision cut</dt><dd>{{ selectedSignal.decisionCut }}</dd></div><div><dt>Action</dt><dd>{{ selectedSignal.action }}</dd></div><div><dt>Availability</dt><dd>{{ selectedSignal.availableAt }}</dd></div><div><dt>Gate</dt><dd :class="selectedSignal.blocker ? 'negative' : 'positive'">{{ selectedSignal.blocker ?? 'Eligible' }}</dd></div></dl></article></div>
    </section>

    <section v-else-if="activePane === 'activity'" class="pane-section activity-pane" aria-labelledby="activity-heading"><header class="pane-heading"><div><h1 id="activity-heading">Activity</h1><span>{{ snapshot.activity.length }} recent</span></div><time :datetime="snapshot.observedAt">Sequence {{ snapshot.sequence.toLocaleString() }}</time></header><Suspense><PerspectiveExplorer :snapshot="snapshot" scope="activity" compact/><template #fallback><div class="empty-state">Loading activity explorer</div></template></Suspense></section>

    <section v-else class="pane-section sources-section" aria-labelledby="sources-heading">
      <header class="pane-heading"><div><h1 id="sources-heading">Sources</h1><span>{{ snapshot.sources.length }} connected</span></div><button @click="emit('explore')">Open data explorer</button></header>
      <div class="source-layout"><nav aria-label="Data sources"><button v-for="source in snapshot.sources" :key="sourceKey(source)" :class="{ selected: sourceKey(selectedSource ?? source) === sourceKey(source) }" :data-health="source.health" @click="selectedSourceKey = sourceKey(source)"><i></i><span><strong>{{ source.name }}</strong><small>{{ source.channel }}</small></span><b>{{ source.lagMs.toLocaleString() }}ms</b></button></nav><article v-if="selectedSource" class="source-inspector"><header><div><span :data-health="selectedSource.health">{{ selectedSource.health }}</span><h2>{{ selectedSource.name }}</h2><p>{{ selectedSource.channel }}</p></div><strong>{{ selectedSource.lagMs.toLocaleString() }}<small>ms lag</small></strong></header><dl><div><dt>Watermark</dt><dd>{{ selectedSource.watermark }}</dd></div><div><dt>Ordering</dt><dd>{{ selectedSource.detail }}</dd></div><div><dt>Data class</dt><dd>{{ snapshot.dataClass }}</dd></div><div><dt>Adapter state</dt><dd>{{ selectedSource.health === 'healthy' ? 'Current' : 'Inspect' }}</dd></div></dl><section><header class="section-header"><h3>Published observations</h3><span>{{ selectedSourceSeries.length }}</span></header><ul><li v-for="series in selectedSourceSeries" :key="series.id"><i :style="{ background: series.color }"></i><span><strong>{{ series.label }}</strong><small>{{ series.provenance }}</small></span><b>{{ series.freshness }}</b></li></ul></section></article></div>
    </section>

    <Teleport to="body"><dialog ref="orderDialog" class="order-dialog" @click.self="closeOrderTicket"><div class="dialog-shell"><header><div><span>OMS command</span><h2>New order</h2></div><button aria-label="Close order ticket" @click="closeOrderTicket">×</button></header><Suspense><NewOrderTicket :key="orderTicketRevision" :authority="authority" :initial-instrument="orderInstrument" :port="port" :snapshot="snapshot" :stale="stale" @authority="emit('authority', $event)"/><template #fallback><div class="ticket-loading">Loading order ticket</div></template></Suspense></div></dialog></Teleport>
  </div>
</template>

<style scoped>
.operations-pane{min-width:0}.sr-only{position:absolute;width:1px;height:1px;overflow:hidden;clip:rect(0,0,0,0)}.positive{color:var(--atlas-green-ink)!important}.negative{color:var(--atlas-oxide)!important}.pane-section{min-height:calc(100vh - 64px);background:var(--operator-black)}
.pane-heading,.section-header{display:flex;justify-content:space-between;gap:18px;align-items:center;border-bottom:1px solid var(--atlas-rule)}.pane-heading{min-height:72px;padding:0 20px}.pane-heading>div:first-child{display:flex;gap:10px;align-items:baseline}.pane-heading h1{margin:0;font-size:24px;letter-spacing:-.025em}.pane-heading span,.pane-heading time,.section-header>span{color:var(--atlas-axis);font:8px var(--vp-font-family-mono);text-transform:uppercase}.heading-actions{display:flex!important;gap:12px!important;align-items:center!important}.pane-heading button,.row-action,.forecast-inspector>header button{min-height:34px;padding:0 11px;color:var(--atlas-blue);font:8px var(--vp-font-family-mono);text-transform:uppercase;border:1px solid var(--atlas-rule);background:transparent;cursor:pointer}.section-header{min-height:48px;padding:0 16px}.section-header h2,.section-header h3{margin:0;font-size:14px}
.position-summary{overflow-x:auto;border-bottom:1px solid var(--atlas-rule)}.position-summary dl{display:grid;grid-template-columns:repeat(4,minmax(180px,1fr));min-width:720px;margin:0}.position-summary dl>div{padding:15px 18px;border-right:1px solid var(--atlas-rule)}dt{color:var(--atlas-axis);font:8px var(--vp-font-family-mono);text-transform:uppercase}dd{margin:6px 0 0}.position-summary dd{font:620 18px var(--vp-font-family-mono)}.table-scroll{overflow-x:auto}table{width:100%;min-width:980px;border-collapse:collapse}th,td{height:42px;padding:0 13px;color:var(--atlas-muted);font-size:10px;text-align:left;border-right:1px solid var(--atlas-rule-soft);border-bottom:1px solid var(--atlas-rule-soft)}thead th{height:31px;color:var(--atlas-axis);font:8px var(--vp-font-family-mono);text-transform:uppercase;background:var(--atlas-surface-alt)}tbody th{color:var(--atlas-ink);font:610 11px var(--vp-font-family-mono)}.number{font-family:var(--vp-font-family-mono);font-variant-numeric:tabular-nums;text-align:right}.positions-table{min-width:1400px}.positions-table th:first-child{position:sticky;z-index:1;left:0;background:var(--operator-black)}.positions-table thead th:first-child{z-index:2;background:var(--atlas-surface-alt)}.row-action{min-height:28px;padding:0 9px}
.orders-grid{display:grid;grid-template-columns:minmax(320px,.75fr) minmax(420px,1.25fr);gap:14px;padding:14px;border-bottom:1px solid var(--atlas-rule)}.active-orders,.order-inspector{min-width:0;border:1px solid var(--atlas-rule)}.order-register>button{display:grid;width:100%;grid-template-columns:34px 82px auto;gap:4px 9px;align-items:center;min-height:58px;padding:8px 12px;text-align:left;border:0;border-bottom:1px solid var(--atlas-rule-soft);background:transparent;cursor:pointer}.order-register>button:hover,.order-register>button.selected{background:var(--atlas-blue-soft)}.order-register span{color:var(--atlas-oxide);font:8px var(--vp-font-family-mono);text-transform:uppercase}.order-register span[data-side=buy]{color:var(--atlas-green-ink)}.order-register strong{font:10px var(--vp-font-family-mono)}.order-register b{justify-self:end;color:var(--atlas-blue);font:500 8px var(--vp-font-family-mono);text-transform:uppercase}.order-register small{grid-column:2/4;color:var(--atlas-muted);font-size:9px}.order-register code{grid-row:2;color:var(--atlas-axis);font-size:7px}.order-detail{display:grid;grid-template-columns:repeat(2,1fr);margin:0}.order-detail>div{padding:12px;border-right:1px solid var(--atlas-rule-soft);border-bottom:1px solid var(--atlas-rule-soft)}.order-detail dd{color:var(--atlas-ink);font:9px var(--vp-font-family-mono)}.fills-section{border-bottom:1px solid var(--atlas-rule)}
.forecast-layout{display:grid;grid-template-columns:minmax(290px,.62fr) minmax(0,1.38fr);min-height:calc(100vh - 136px)}.forecast-register{border-right:1px solid var(--atlas-rule)}.forecast-register button,.forecast-candidates>button{display:grid;width:100%;grid-template-columns:8px minmax(0,1fr) auto;gap:10px;align-items:start;padding:14px;text-align:left;border:0;border-bottom:1px solid var(--atlas-rule-soft);background:transparent;cursor:pointer}.forecast-register button{min-height:76px}.forecast-register button:hover,.forecast-register button.selected,.forecast-candidates>button:hover,.forecast-candidates>button.selected{background:var(--atlas-blue-soft)}.forecast-register button>span,.forecast-candidates i{width:7px;height:7px;margin-top:3px;border-radius:50%;background:var(--atlas-blue)}.forecast-register button>span[data-state=eligible],.forecast-candidates i[data-state=eligible]{background:var(--atlas-green)}.forecast-register button>span[data-state=blocked],.forecast-candidates i[data-state=blocked]{background:var(--atlas-oxide)}.forecast-register div,.forecast-candidates button>span{display:grid;gap:5px;min-width:0}.forecast-register strong,.forecast-candidates strong{font-size:11px}.forecast-register small,.forecast-candidates small{overflow:hidden;color:var(--atlas-axis);font:8px var(--vp-font-family-mono);text-overflow:ellipsis;white-space:nowrap}.forecast-register b{color:var(--atlas-axis);font:8px var(--vp-font-family-mono);text-transform:uppercase}.forecast-inspector{min-width:0;padding:20px}.forecast-inspector>header{display:flex;justify-content:space-between;gap:22px;align-items:flex-start}.forecast-inspector>header span{color:var(--atlas-blue);font:8px var(--vp-font-family-mono);text-transform:uppercase}.forecast-inspector h2{margin:5px 0 0;font-size:25px}.forecast-inspector>header p{max-width:65ch;margin:9px 0 0;color:var(--atlas-muted);font-size:11px;line-height:1.5}.forecast-inputs{display:grid;grid-template-columns:minmax(190px,.5fr) minmax(0,1.5fr);gap:18px;margin-top:20px;padding:16px;border:1px solid var(--atlas-rule)}.forecast-inputs h3{margin:0;font-size:13px}.forecast-inputs p{margin:6px 0 0;color:var(--atlas-muted);font-size:10px;line-height:1.45}.forecast-inputs ul{display:grid;grid-template-columns:repeat(auto-fit,minmax(210px,1fr));gap:6px;margin:0;padding:0;list-style:none}.forecast-inputs li{display:grid;grid-template-columns:5px minmax(0,1fr) auto;gap:7px;align-items:center;min-width:0;padding:7px 8px;border:1px solid var(--atlas-rule);background:var(--atlas-surface-alt)}.forecast-inputs li>span{width:5px;height:24px}.forecast-inputs li>div{display:grid;gap:3px;min-width:0}.forecast-inputs li strong{overflow:hidden;font-size:9px;text-overflow:ellipsis;white-space:nowrap}.forecast-inputs li small{overflow:hidden;color:var(--atlas-axis);font:7px var(--vp-font-family-mono);text-overflow:ellipsis;white-space:nowrap}.forecast-inputs li b{color:var(--atlas-blue);font:7px var(--vp-font-family-mono);text-transform:uppercase;white-space:nowrap}.forecast-candidates{margin-top:14px;border:1px solid var(--atlas-rule)}.forecast-candidates>button{min-height:64px}.forecast-candidates b{color:var(--atlas-green-ink);font:11px var(--vp-font-family-mono)}.forecast-decision{display:grid;grid-template-columns:repeat(4,1fr);margin:14px 0 0;border:1px solid var(--atlas-rule)}.forecast-decision>div{min-width:0;padding:11px;border-right:1px solid var(--atlas-rule-soft)}.forecast-decision dd{overflow:hidden;color:var(--atlas-muted);font-size:9px;text-overflow:ellipsis;white-space:nowrap}
.activity-pane :deep(.perspective-explorer){min-height:calc(100vh - 136px)}.source-layout{display:grid;grid-template-columns:minmax(280px,.62fr) minmax(0,1.38fr);min-height:calc(100vh - 136px)}.source-layout>nav{border-right:1px solid var(--atlas-rule)}.source-layout>nav button{display:grid;width:100%;grid-template-columns:8px minmax(0,1fr) auto;gap:10px;align-items:center;min-height:65px;padding:10px 14px;text-align:left;border:0;border-bottom:1px solid var(--atlas-rule-soft);background:transparent;cursor:pointer}.source-layout>nav button:hover,.source-layout>nav button.selected{background:var(--atlas-blue-soft)}.source-layout>nav i{width:7px;height:7px;border-radius:50%;background:var(--atlas-green)}.source-layout>nav button[data-health=degraded] i,.source-layout>nav button[data-health=stale] i{background:var(--atlas-oxide)}.source-layout>nav span{display:grid;gap:4px}.source-layout>nav strong{font-size:11px}.source-layout>nav small,.source-layout>nav b{color:var(--atlas-axis);font:8px var(--vp-font-family-mono)}.source-inspector{padding:22px}.source-inspector>header{display:flex;justify-content:space-between;gap:20px}.source-inspector>header span{color:var(--atlas-green-ink);font:8px var(--vp-font-family-mono);text-transform:uppercase}.source-inspector>header span[data-health=degraded],.source-inspector>header span[data-health=stale]{color:var(--atlas-oxide)}.source-inspector h2{margin:5px 0 0;font-size:25px}.source-inspector>header p{margin:3px 0 0;color:var(--atlas-muted);font:9px var(--vp-font-family-mono)}.source-inspector>header>strong{display:grid;color:var(--atlas-ink);font:24px var(--vp-font-family-mono);text-align:right}.source-inspector>header>strong small{color:var(--atlas-axis);font-size:7px;text-transform:uppercase}.source-inspector>dl{display:grid;grid-template-columns:repeat(4,1fr);margin:20px 0;border:1px solid var(--atlas-rule)}.source-inspector>dl>div{padding:11px;border-right:1px solid var(--atlas-rule-soft)}.source-inspector dd{color:var(--atlas-muted);font:9px var(--vp-font-family-mono)}.source-inspector>section{border:1px solid var(--atlas-rule)}.source-inspector ul{margin:0;padding:0;list-style:none}.source-inspector li{display:grid;grid-template-columns:7px minmax(0,1fr) auto;gap:10px;align-items:center;min-height:54px;padding:8px 13px;border-bottom:1px solid var(--atlas-rule-soft)}.source-inspector li>i{width:5px;height:20px}.source-inspector li>span{display:grid;gap:3px}.source-inspector li strong{font-size:10px}.source-inspector li small,.source-inspector li b{color:var(--atlas-axis);font:8px var(--vp-font-family-mono)}.empty-state,.ticket-loading{display:grid;min-height:100px;place-items:center;color:var(--atlas-axis);font:8px var(--vp-font-family-mono);text-transform:uppercase}
.order-dialog{width:min(720px,calc(100vw - 28px));max-height:calc(100dvh - 28px);padding:0;color:var(--atlas-ink);border:1px solid var(--atlas-blue);background:var(--operator-black,#050b12);box-shadow:0 24px 90px #000}.order-dialog::backdrop{background:rgba(1,5,9,.76);backdrop-filter:blur(5px)}.dialog-shell>header{display:flex;justify-content:space-between;align-items:center;min-height:60px;padding:0 16px;border-bottom:1px solid var(--atlas-rule)}.dialog-shell>header span{color:var(--atlas-blue);font:8px var(--vp-font-family-mono);text-transform:uppercase}.dialog-shell>header h2{margin:2px 0 0;font-size:19px}.dialog-shell>header button{width:36px;height:36px;color:var(--atlas-muted);font-size:25px;border:0;background:transparent;cursor:pointer}.dialog-shell :deep(.order-ticket){border:0}.dialog-shell :deep(.order-ticket>header){display:none}
button:focus-visible,.table-scroll:focus-visible{outline:2px solid var(--atlas-blue);outline-offset:-2px}@media(max-width:900px){.orders-grid,.forecast-layout,.source-layout{grid-template-columns:1fr}.forecast-register,.source-layout>nav{border-right:0;border-bottom:1px solid var(--atlas-rule)}.forecast-inspector>header{flex-direction:column}.forecast-inputs{grid-template-columns:1fr}.forecast-decision,.source-inspector>dl{grid-template-columns:repeat(2,1fr)}}@media(max-width:620px){.pane-section{min-height:calc(100vh - 109px)}.pane-heading{align-items:flex-start;min-height:76px;flex-direction:column;padding:12px}.heading-actions{width:100%;justify-content:space-between}.pane-heading time{display:none}.position-summary dl{grid-template-columns:repeat(4,minmax(150px,1fr));min-width:600px}.orders-grid{gap:9px;padding:9px}.forecast-inspector,.source-inspector{padding:14px 12px}.forecast-decision,.source-inspector>dl{grid-template-columns:1fr}.order-dialog{width:100vw;max-width:none;max-height:100dvh;margin:0;border-right:0;border-left:0}.source-inspector>header{align-items:flex-start}.source-inspector>header>strong{font-size:18px}}
</style>
