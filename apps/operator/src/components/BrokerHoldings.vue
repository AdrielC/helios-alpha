<script setup lang="ts">
import { computed, ref } from "vue";
import { brokerMoney, decimalUnits, holdingValues, type BrokerObservation, type BrokerSync } from "../operations/broker-holdings";
import StrategyMonitor from "./StrategyMonitor.vue";
const props = defineProps<{ observation: BrokerObservation | null; sync: BrokerSync; error: string; loading: boolean; now: number }>();
defineEmits<{ refresh: []; connect: [] }>();
const selected = ref("");
const search = ref("");
const age = computed(() => props.observation ? Math.max(0, props.now - props.observation.observed_ms) : 0);
function elapsed(ms: number): string { return ms < 60_000 ? `${Math.floor(ms / 1000)}s` : ms < 3_600_000 ? `${Math.floor(ms / 60_000)}m` : `${Math.floor(ms / 3_600_000)}h`; }
const stale = computed(() => !props.observation || age.value > 60_000 || Boolean(props.error));
const connected = computed(() => ["connected", "syncing"].includes(props.sync.state) && !!props.sync.last_success_ms);
const busy = computed(() => ["syncing", "authorizing"].includes(props.sync.state));
const status = computed(() => props.sync.state === "authorizing" ? "Awaiting authorization" : props.sync.state === "syncing" ? "Syncing account" : connected.value ? (stale.value ? "Account delayed" : "Account synced") : props.sync.state === "degraded" ? "Sync interrupted" : "Connection needed");
const observedAt = computed(() => props.observation ? new Date(props.observation.observed_ms).toISOString() : "");
const observedLabel = computed(() => observedAt.value ? new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "medium" }).format(new Date(observedAt.value)) : "Never");
const rows = computed(() => (props.observation?.positions ?? []).map(position => ({ ...position, ...holdingValues(position, props.observation?.quotes ?? []) })));
const filtered = computed(() => rows.value.filter(row => row.asset.toLowerCase().includes(search.value.toLowerCase())));
const detail = computed(() => rows.value.find(row => row.asset === selected.value) ?? rows.value[0]);
const permissions = computed(() => props.sync.account_permissions && props.now - props.sync.account_permissions.observed_ms <= 60_000 ? props.sync.account_permissions : undefined);
const tradeUrl = computed(() => detail.value ? `https://robinhood.com/us/en/crypto/${encodeURIComponent(detail.value.asset.toLowerCase())}/` : 'https://robinhood.com/');
const nextRead = computed(() => props.sync.next_poll_ms ? elapsed(Math.max(0, props.sync.next_poll_ms - props.now)) : null);
const money = (value: string | null | undefined) => value == null ? '—' : brokerMoney(value);
const pnlClass = (value: string | null) => value === null ? '' : decimalUnits(value) < 0n ? 'negative' : decimalUnits(value) > 0n ? 'positive' : '';
</script>

<template>
  <section class="broker-holdings" aria-labelledby="broker-holdings-heading">
    <header class="account-heading">
      <div class="account-identity"><span class="account-icon" aria-hidden="true">R</span><div><h2 id="broker-holdings-heading">Robinhood holdings <span>{{ observation?.account.crypto ?? '' }}</span></h2></div></div>
      <div class="account-actions"><span class="connection-status" :data-ok="connected && !stale"><i></i>{{ status }}</span><button class="refresh-button" :disabled="busy || !connected" @click="$emit('refresh')" :title="connected ? 'Request current holdings and prices from Robinhood' : 'Connect Robinhood to enable account refresh'"><span aria-hidden="true">↻</span> {{ busy ? 'Syncing…' : 'Refresh' }}</button></div>
    </header>
    <div v-if="!connected || error" class="connection-notice" role="status">
      <span class="notice-icon" aria-hidden="true">↗</span><div><strong>{{ error ? 'Account update unavailable' : sync.state === 'authorizing' ? 'Finish connecting Robinhood' : 'Connect for automatic account updates' }}</strong><p>{{ error || (sync.state === 'authorizing' ? 'Authorize Helios in Robinhood. Your holdings will appear here automatically.' : 'A direct connection keeps balances, positions and prices current, including when this chat is closed.') }}</p></div>
      <a v-if="sync.authorization_url" :href="sync.authorization_url" target="_blank" rel="noopener noreferrer">Continue to Robinhood ↗</a>
      <button v-else :disabled="busy || sync.state === 'offline'" @click="$emit('connect')">{{ busy ? 'Connecting…' : 'Connect Robinhood' }}</button>
    </div>
    <template v-if="observation">
      <div class="account-metrics">
        <div class="metric primary"><span>ACCOUNT VALUE <span v-if="stale" class="aging">LAST KNOWN</span></span><strong>{{ money(observation.portfolio.total_value) }}</strong><small>USD</small></div>
        <div class="metric"><span>CRYPTO EXPOSURE</span><strong>{{ money(observation.portfolio.crypto_value) }}</strong><small>{{ observation.positions.length }} {{ observation.positions.length === 1 ? 'holding' : 'holdings' }}</small></div>
        <div class="metric"><span>AVAILABLE CASH</span><strong>{{ money(observation.portfolio.cash) }}</strong><small>Uninvested balance</small></div>
        <div class="metric"><span>CRYPTO BUYING POWER</span><strong>{{ money(observation.portfolio.crypto_buying_power) }}</strong><small>{{ observation.portfolio.crypto_buying_power === null ? 'Not supplied by broker' : 'Reported by Robinhood' }}</small></div>
      </div>
      <div class="portfolio-grid">
        <div class="position-register">
          <div class="register-heading"><div><h3>Open positions <span>{{ rows.length }}</span></h3></div><label class="asset-search"><span aria-hidden="true">⌕</span><input v-model="search" aria-label="Filter holdings" placeholder="Filter assets" /></label></div>
          <div class="broker-table-scroll" tabindex="0" aria-label="Robinhood holdings. Scroll horizontally for quantities."><table><thead><tr><th>Instrument</th><th class="number">Quantity</th><th class="number">Avg. cost</th><th class="number">Mark</th><th class="number">Market value</th><th class="number">Price P&amp;L</th></tr></thead><tbody>
            <tr v-for="row in filtered" :key="row.asset" :class="{ selected: detail?.asset === row.asset }"><th><button class="asset-button" @click="selected = row.asset"><span class="asset-icon">{{ row.asset.slice(0, 3) }}</span><span><strong>{{ row.asset }}</strong><small class="asset-pair">{{ row.asset }} / USD</small><small class="compact-quantity">{{ row.quantity }}</small></span></button></th><td class="number">{{ row.quantity }} <span class="sr-only">{{ row.asset }}</span></td><td class="number">{{ money(row.average) }}</td><td class="number"><span>{{ money(row.mark) }}</span><small v-if="row.quote" :class="{ 'old-quote': now - row.quote.quoted_ms > 60_000 }">{{ elapsed(Math.max(0, now - row.quote.quoted_ms)) }} ago</small></td><td class="number value-cell">{{ money(row.value) }}</td><td class="number" :class="pnlClass(row.pnl)">{{ row.pnl !== null && decimalUnits(row.pnl) > 0n ? '+' : '' }}{{ money(row.pnl) }}</td></tr>
            <tr v-if="rows.length === 0"><td colspan="6" class="empty-account">Robinhood reported no crypto holdings at this account read.</td></tr><tr v-else-if="!filtered.length"><td colspan="6" class="empty-account">No assets match your filter.</td></tr>
          </tbody></table></div>
          <footer class="register-footer"><span>Price P&amp;L excludes trading fees.</span><span>{{ observation.open_order_count }} open {{ observation.open_order_count === 1 ? 'order' : 'orders' }}</span></footer>
        </div>
        <div class="book-monitor"><StrategyMonitor :sync="sync" :now="now" /></div>
        <aside class="position-detail" aria-label="Selected holding details">
          <template v-if="detail"><div class="detail-title"><span class="eyebrow">SELECTED POSITION</span><h3>{{ detail.asset }} <span>/ USD</span></h3></div>
            <div class="exit-state"><strong>Manual position</strong><p>No automated exit</p></div>
            <a class="trade-link" :href="tradeUrl" target="_blank" rel="noopener noreferrer">Trade {{ detail.asset }} at Robinhood <span aria-hidden="true">↗</span></a>
            <p class="trade-explainer">Review and submit at Robinhood · {{ observation.account.crypto }}</p>
            <dl><div><dt>Available to sell</dt><dd>{{ detail.sellable_quantity }} <small>{{ detail.asset }}</small></dd></div><div><dt>Reserved for sells</dt><dd>{{ detail.quantity_in_open_sells }} <small>{{ detail.asset }}</small></dd></div><div><dt>Cost basis</dt><dd>{{ money(detail.cost_basis) }}</dd></div><div><dt>Bid / ask</dt><dd>{{ money(detail.quote?.bid) }} <span>/</span> {{ money(detail.quote?.ask) }}</dd></div><div><dt>Price source</dt><dd>Robinhood <small v-if="detail.quote">· {{ elapsed(Math.max(0, now - detail.quote.quoted_ms)) }} ago</small></dd></div></dl><p v-if="detail.quote && !detail.quote.book_valid" class="quote-note">Broker bid/ask is crossed. Mark and P&amp;L are indicative.</p><div class="broker-permissions"><div><span>Robinhood approvals</span><strong>{{ permissions ? permissions.approval_required ? 'Required' : 'Off' : 'Unknown' }}</strong></div><div><span>Options account access</span><strong>{{ permissions ? permissions.options_level === 'none' ? 'Not approved' : permissions.options_level === 'option_level_2' ? 'Level 2' : 'Level 3' : 'Unknown' }}</strong></div><p v-if="permissions && permissions.options_level !== 'none'">Options available via MCP · no active options strategy</p></div></template>
          <div v-else class="detail-empty">Select a holding to inspect its availability and pricing.</div>
        </aside>
      </div>
      <footer class="account-footer"><span :class="{ aging: stale }"><i></i>{{ stale ? 'Last known account read' : 'Account read' }} <time :datetime="observedAt" :title="observedLabel">{{ elapsed(age) }} ago</time></span><span>{{ observedLabel }}</span><span v-if="connected">Auto sync {{ (sync.interval_ms ?? 30000) / 1000 }}s<span v-if="nextRead"> · next in {{ nextRead }}</span></span><span v-else>Automatic sync disconnected</span></footer>
    </template>
    <div v-else class="no-account"><span aria-hidden="true">◈</span><h3>Your portfolio, in one place</h3><p>Connect Robinhood to load your real holdings and balances.</p></div>
  </section>
</template>

<style scoped>
.broker-holdings { --panel:#101822; --line:#263342; --muted:#98a8ba; padding:18px 20px; color:#e6edf5; background:#0b111a; min-width:0; font-size:13px; }
.account-heading,.account-identity,.account-actions { display:flex; align-items:center; }
.account-heading { justify-content:space-between; gap:16px; margin-bottom:16px; }
.account-identity { gap:10px; min-width:0; }
.account-icon { display:grid; place-items:center; width:30px; height:30px; flex-shrink:0; background:#18312e; color:#9ddcc8; border:1px solid #305049; border-radius:8px; font-weight:650; }
h2 { margin:0; font-size:16px; font-weight:600; letter-spacing:-.2px; }
h2 span { font:11px var(--vp-font-family-mono); color:var(--muted); margin-left:6px; }
.account-actions { gap:14px; }
.connection-status { display:flex; align-items:center; gap:6px; font-size:11px; color:#d4ad72; white-space:nowrap; }
.connection-status i,.account-footer i { width:5px; height:5px; border-radius:50%; background:currentColor; display:inline-block; }
.connection-status[data-ok=true] { color:#82cda8; }
button,.connection-notice a { font:500 12px var(--vp-font-family-base); cursor:pointer; border:1px solid #34485e; border-radius:5px; background:#172333; color:#dbeaff; padding:7px 11px; text-decoration:none; white-space:nowrap; }
button:hover:not(:disabled),.connection-notice a:hover { background:#22364c; border-color:#6b96bb; }
button:disabled { opacity:.45; cursor:default; }
.refresh-button span { font-size:16px; margin-right:3px; }
.connection-notice { display:flex; align-items:center; gap:12px; padding:14px; margin-bottom:16px; border:1px solid #4e4330; border-radius:7px; background:#211f19; color:#d9c4a5; }
.notice-icon { font-size:20px; }
.connection-notice>div { flex:1; }
.connection-notice p { margin:4px 0 0; font-size:12px; line-height:1.5; color:#b2a995; }
.account-metrics { display:grid; grid-template-columns:repeat(4,minmax(0,1fr)); margin-bottom:16px; border:1px solid var(--line); border-radius:8px; background:var(--panel); overflow:hidden; }
.metric { padding:16px 18px; border-left:1px solid var(--line); display:flex; flex-direction:column; gap:6px; min-width:0; }
.metric:first-child { border-left:0; background:linear-gradient(120deg,#192b38,#101822); }
.metric>span { font:10px var(--vp-font-family-base); letter-spacing:.04em; color:var(--muted); }
.metric strong { font:500 25px var(--vp-font-family-mono); letter-spacing:-1px; font-variant-numeric:tabular-nums; }
.metric.primary strong { color:#f2f7fa; }
.metric small { font-size:11px; color:var(--muted); }
.aging { color:#d6aa68!important; }
.metric .aging { font-size:8px; margin-left:4px; }
.portfolio-grid { display:grid; grid-template-columns:minmax(0,1fr) 300px; grid-template-areas:"register detail" "monitor detail"; grid-template-rows:min-content 1fr; gap:16px; align-items:start; }
.position-register { grid-area:register; min-width:0; border:1px solid var(--line); border-radius:8px; background:var(--panel); overflow:hidden; }
.register-heading { display:flex; justify-content:space-between; align-items:center; padding:12px 16px; gap:12px; }
h3 { margin:0; font-size:14px; font-weight:600; }
.register-heading h3 span { color:var(--muted); font:11px var(--vp-font-family-mono); margin-left:6px; }
.asset-search { display:flex; align-items:center; border:1px solid var(--line); border-radius:5px; padding:6px 8px; gap:6px; max-width:140px; color:var(--muted); }
.asset-search input { width:100%; min-width:0; color:#e5edf4; background:transparent; border:0; outline:0; font-size:12px; }
.broker-table-scroll { position:relative; overflow-x:auto; }
table { width:100%; min-width:620px; border-collapse:collapse; }
th,td { padding:13px 12px; text-align:left; border-bottom:1px solid var(--line); font-size:12px; white-space:nowrap; }
thead th { padding:10px 12px; background:#131f2d; color:#a1b1c3; font:11px var(--vp-font-family-base); border-top:1px solid var(--line); }
tbody tr.selected { background:#152638; }
thead th:first-child,tbody th:first-child { position:sticky; left:0; z-index:1; background:#131f2d; }
tbody tr.selected th:first-child { background:#152638; }
tbody tr:hover { background:#1b3044; }
.number { text-align:right; font-family:var(--vp-font-family-mono); font-variant-numeric:tabular-nums; }
.number small { display:block; font:10px var(--vp-font-family-mono); color:var(--muted); margin-top:4px; }
.number small.old-quote { color:#d6aa68; }
.value-cell { font-weight:650; }
.asset-button { display:flex; align-items:center; gap:9px; border:0; padding:0; background:transparent; text-align:left; }
.asset-button:hover:not(:disabled) { background:transparent; }
.asset-icon { display:grid; place-items:center; width:30px; height:30px; border-radius:50%; border:1px solid #395368; background:#21384b; color:#c8ddee; font:9px var(--vp-font-family-mono); }
.asset-button strong { font-size:13px; }
.asset-button small { display:block; color:var(--muted); font:10px var(--vp-font-family-mono); margin-top:4px; }
.asset-button .compact-quantity { display:none; }
.positive { color:#83d4ac; } .negative { color:#f19b8e; }
.register-footer { display:flex; justify-content:space-between; gap:12px; padding:10px 14px; color:var(--muted); font-size:10px; line-height:1.5; }
.register-footer>span:last-child { white-space:nowrap; }
.book-monitor { grid-area:monitor; min-width:0; align-self:start; }
.position-detail { grid-area:detail; border:1px solid var(--line); border-radius:8px; background:var(--panel); padding:16px; min-width:0; }
.eyebrow { font:10px var(--vp-font-family-base); letter-spacing:.06em; color:var(--muted); }
.detail-title h3 { margin:8px 0 0; font:500 23px var(--vp-font-family-mono); }
.detail-title h3 span { font-size:12px; color:var(--muted); }
.exit-state { padding:10px 12px; border-left:2px solid #b59864; background:#211f1b; border-radius:0 5px 5px 0; margin:14px 0 12px; }
.exit-state strong { font-size:12px; font-weight:500; color:#e5cfa8; }
.exit-state p { margin:4px 0 0; font-size:11px; color:#b6ad9e; }
.trade-link { display:flex; justify-content:space-between; gap:12px; padding:11px 12px; background:#d4def5; border:1px solid #d4def5; color:#162034; font-size:12px; font-weight:600; border-radius:5px; text-decoration:none; }
.trade-link:hover { background:#edf2ff; }
.trade-explainer { margin:8px 0 0; font-size:10px; line-height:1.5; color:var(--muted); }
.position-detail dl { margin:12px 0 0; }
.position-detail dl>div { display:flex; justify-content:space-between; gap:10px; padding:9px 0; align-items:baseline; border-bottom:1px solid var(--line); }
dt { font-size:11px; color:var(--muted); }
dd { margin:0; font:11px var(--vp-font-family-mono); text-align:right; overflow-wrap:anywhere; }
dd small,dd span { color:var(--muted); font-size:10px; }
.quote-note { margin:12px 0 0; font-size:11px; line-height:1.5; color:#d6aa68; }
.broker-permissions { margin-top:12px; }
.broker-permissions>div { display:flex; justify-content:space-between; gap:8px; font-size:11px; margin:8px 0; color:var(--muted); }
.broker-permissions strong { color:#cfdaeb; font-weight:500; }
.broker-permissions p { margin:10px 0 0; font-size:10px; color:var(--muted); line-height:1.5; }
.account-footer { display:flex; gap:12px; align-items:center; flex-wrap:wrap; margin-top:14px; color:var(--muted); font:10px var(--vp-font-family-mono); }
.account-footer>span:first-child { color:#8bbda8; }
.account-footer i { margin-right:6px; }
.account-footer>span:last-child { margin-left:auto; }
.empty-account { padding:28px 16px; text-align:center; color:var(--muted); }
.detail-empty { line-height:1.7; color:var(--muted); }
.no-account { display:grid; place-items:center; padding:60px 20px; text-align:center; border:1px dashed var(--line); border-radius:8px; }
.no-account>span { font-size:32px; color:#79a3c3; margin-bottom:16px; }
.no-account p { color:var(--muted); line-height:1.6; }
.sr-only { position:absolute; width:1px; height:1px; padding:0; margin:-1px; overflow:hidden; clip:rect(0,0,0,0); white-space:nowrap; border:0; }
button:focus-visible,a:focus-visible,.broker-table-scroll:focus-visible,.asset-search:focus-within { outline:2px solid #7db5e7; outline-offset:3px; }
@media(max-width:1250px) {
  .portfolio-grid { grid-template-columns:minmax(0,1fr); grid-template-areas:"register" "detail" "monitor"; grid-template-rows:auto; }
  .position-detail { display:grid; grid-template-columns:minmax(0,1fr) minmax(0,1fr); gap:0 24px; }
  .detail-title,.exit-state,.trade-link,.trade-explainer { grid-column:1; }
  .position-detail dl { grid-column:2; grid-row:1/5; margin:0; align-self:start; }
  .broker-permissions { grid-column:1/-1; display:flex; flex-wrap:wrap; gap:8px 24px; align-items:baseline; border-top:1px solid var(--line); padding-top:10px; }
  .broker-permissions>div,.broker-permissions p { margin:0; }
  .quote-note { grid-column:1/-1; }
}
@media(max-width:900px) {
  table { min-width:0; }
  th:nth-child(3),td:nth-child(3),th:nth-child(4),td:nth-child(4) { display:none; }
  .account-metrics { grid-template-columns:repeat(2,minmax(0,1fr)); }
  .metric:nth-child(3) { border-left:0; }
  .metric:nth-child(n+3) { border-top:1px solid var(--line); }
  .metric { padding:12px 14px; gap:4px; }
  .metric strong { font-size:23px; }
  .account-heading { flex-wrap:wrap; gap:10px; }
  .account-actions { flex:1; justify-content:flex-end; }
  .connection-notice { flex-wrap:wrap; }
  .account-footer>span:last-child { margin-left:0; }
}
@media(max-width:700px) {
  .broker-holdings { padding:14px 16px; }
  .account-icon { display:none; }
  h2 { font-size:14px; } h2 span { display:none; }
  .connection-status { font-size:10px; }
  .account-actions { gap:10px; }
  .refresh-button { padding:5px 8px; font-size:11px; }
  .register-heading { padding:10px 12px; } .register-heading h3 { font-size:13px; }
  .asset-search { max-width:120px; }
  .position-detail { gap:0 16px; padding:14px; }
  .position-detail dl>div { flex-direction:column; gap:4px; padding:6px 0; }
  dd { text-align:left; }
}
@media(max-width:600px) {
  th:nth-child(2),td:nth-child(2) { display:none; }
  .asset-button .compact-quantity { display:block; }
  .asset-button .asset-pair { display:none; }
  th,td { padding:12px 10px; font-size:11px; }
  thead th { padding:9px 10px; font-size:10px; }
  .asset-icon { display:none; }
  .account-heading { gap:8px; }
  .metric>span { font-size:9px; }
  .trade-link { font-size:11px; padding:10px; }
  .register-footer { flex-wrap:wrap; gap:4px; }
}
@media(prefers-reduced-motion:no-preference) { button,a { transition:background .15s,border-color .15s; } }
</style>
