<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import type { BrokerSync, StrategyBook, StrategyFeed } from "../operations/broker-holdings";
import type { OperationsSnapshot } from "../operations/operations-port";
import type { TimeSeriesData, TimeSeriesPort } from "../operations/time-series-port";
import { loadEventRadar, type EventRadarSnapshot } from "../operations/event-radar-port";
import { ageLabel, bookState, readable } from "../operations/running-index";
const props = defineProps<{ sync: BrokerSync; now: number; snapshot: OperationsSnapshot; port: TimeSeriesPort }>();
const events = ref<EventRadarSnapshot|null>(null);
const eventError = ref(false);
const channels = ref<readonly TimeSeriesData[]>([]);
const error = ref('');
const loaded = ref(false);
let timer: ReturnType<typeof setInterval> | undefined;
let busy = false, disposed = false, lastHistoryLoad = 0;
const books = computed(() => props.sync.strategy?.books ?? []);
const feeds = computed(() => {
  const entries = new Map<string, { feed: StrategyFeed; book: StrategyBook }>();
  for (const book of books.value) for (const feed of book.feeds ?? []) entries.set(`${book.id}:${feed.id}`, { feed, book });
  return [...entries.values()];
});
const markets = computed(() => books.value.filter(book => book.reference_source && book.reference_source !== 'unknown' && book.reference_channel !== 'public_rest'));
const scientific = computed(() => channels.value.filter(series => series.descriptor.domain === 'source' && series.descriptor.sourceNames?.some(name => /^(noaa-|nasa-)/.test(name))));
const latest = (series: TimeSeriesData) => series.points.length ? Date.parse(series.points.at(-1)!.timestamp) : undefined;
const marketAge = (book: StrategyBook) => Object.values(book.quote_age_ms ?? {}).length ? Math.max(...Object.values(book.quote_age_ms!)) + Math.max(0, props.now - book.updated_ms) : undefined;
const marketHealthy = (book: StrategyBook) => bookState(book, props.sync.strategy, props.now) === 'running' && book.feed_connected && (marketAge(book) ?? Infinity) <= 15_000;
const feedHealthy = (feed: StrategyFeed, book: StrategyBook) => bookState(book, props.sync.strategy, props.now) === 'running' && feed.state === 'running' && feed.last_ok_ms != null && props.now >= feed.last_ok_ms && props.now - feed.last_ok_ms < Math.max(60_000, feed.poll_seconds * 3000);
const atlasSample = (book: StrategyBook) => {
  const times = channels.value.filter(series => series.descriptor.sourceNames?.includes(book.reference_source ?? '')).map(latest).filter((at): at is number => at != null);
  return times.length ? Math.max(...times) : undefined;
};
const observed = computed(() => scientific.value.filter(series => latest(series) != null).length);
const connected = computed(() => markets.value.filter(marketHealthy).length + feeds.value.filter(({feed, book}) => feedHealthy(feed, book)).length);
const eventHealthy = (source: EventRadarSnapshot['sources'][number]) => !eventError.value && events.value?.online && props.now - events.value.observed_ms < 15000 && source.state === 'receiving' && source.last_ok_ms != null && props.now - source.last_ok_ms < 90000;
async function loadEvents() {
  try { const next = await loadEventRadar(); if (!disposed) { events.value = next; eventError.value = false; } }
  catch { if (!disposed) eventError.value = true; }
}
async function load() {
  if (busy || Date.now() - lastHistoryLoad < 30_000) return;
  busy = true; lastHistoryLoad = Date.now();
  try {
    const catalog = await props.port.catalog(props.snapshot.context);
    const now = Date.now();
    const result = await props.port.query({context: props.snapshot.context, seriesIds: catalog.filter(series => series.domain === 'source' || series.domain === 'market').map(series => series.id), from: new Date(now - 86_400_000).toISOString(), to: new Date(now).toISOString(), maxPoints: 2});
    if (!disposed) { channels.value = result.series; loaded.value = true; error.value = ''; }
  } catch { if (!disposed) error.value = 'Observation history could not refresh. Prior timestamps are preserved.'; }
  finally { busy = false; }
}
onMounted(() => { void load(); void loadEvents(); timer = setInterval(() => { if (!document.hidden) { void load(); void loadEvents(); } }, 10_000); });
onBeforeUnmount(() => { disposed = true; clearInterval(timer); });
</script>
<template>
  <div class="source-monitor" aria-label="Live source inventory">
    <div class="source-overview"><div><span>Strategy inputs</span><strong v-if="sync.strategy">{{ connected }} <small>/ {{ markets.length + feeds.length }} healthy</small></strong><strong v-else>Unavailable</strong><p>Reported by Golem · updates every 5s</p></div><div><span>Scientific channels</span><strong v-if="loaded">{{ observed }} <small>/ {{ scientific.length }} observed</small></strong><strong v-else>{{ error ? 'Unavailable' : 'Loading…' }}</strong><p>Samples in the last 24 hours</p></div><div><span>Broker account</span><strong :data-ok="sync.last_success_ms && now - sync.last_success_ms < 90_000">{{ sync.last_success_ms ? ageLabel(sync.last_success_ms, now) : 'Unavailable' }}</strong><p>Robinhood MCP · {{ sync.interval_ms ? sync.interval_ms / 1000 : '—' }}s polling</p></div></div>
    <p v-if="error" role="status" class="source-warning">{{ error }}</p>
    <section><header><div><h2>Market data</h2><p>Reference quotes for strategy research. Robinhood supplies the broker’s separate account marks.</p></div><span>Streaming</span></header>
      <div class="source-cards"><article v-for="book in markets" :key="book.id"><div class="source-title"><span class="source-monogram">↗</span><div><h3>{{ readable(book.reference_source!) }}</h3><small>{{ book.reference_channel }} · WebSocket</small></div><b :data-ok="marketHealthy(book)">{{ marketHealthy(book) ? 'Receiving' : 'Delayed / offline' }}</b></div><div class="source-instruments"><span v-for="symbol in Object.keys(book.quote_age_ms ?? {})" :key="symbol">{{ symbol }}</span></div><dl><div><dt>Used by</dt><dd>{{ readable(book.engine) }} · {{ book.mode }}</dd></div><div><dt>Quote age at heartbeat</dt><dd>{{ Object.entries(book.quote_age_ms ?? {}).map(([symbol, age]) => `${symbol}: ${age}ms`).join(' · ') || 'Unknown' }}</dd></div><div><dt>Heartbeat</dt><dd>{{ ageLabel(book.updated_ms, now) }}</dd></div><div><dt>Latest Atlas sample</dt><dd>{{ ageLabel(atlasSample(book), now) }}</dd></div><div><dt>Atlas history</dt><dd>2s samples · 24h retained · 5s display updates</dd></div></dl></article></div>
      <p v-if="!markets.length" class="empty">Market-feed status is unavailable.</p>
    </section>
    <section><header><div><h2>News &amp; events</h2><p>Polling health and publisher cache age are separate. A successful fetch does not mean a new event.</p></div><span>{{ feeds.length }} feeds</span></header><div class="news-grid"><article v-for="{feed, book} in feeds" :key="`${book.id}:${feed.id}`"><div class="news-title"><h3>{{ readable(feed.id) }}</h3><b :data-ok="feedHealthy(feed, book)">{{ feedHealthy(feed, book) ? 'Polling' : readable(feed.state) === 'running' ? 'Delayed' : readable(feed.state) }}</b></div><dl><div><dt>Last successful poll</dt><dd>{{ ageLabel(feed.last_ok_ms, now) }}</dd></div><div><dt>Cadence</dt><dd>{{ feed.poll_seconds }}s</dd></div><div><dt>Publisher cache age</dt><dd>{{ feed.cache_age_seconds == null ? 'Not reported' : `${feed.cache_age_seconds}s` }}</dd></div><div><dt>Used by</dt><dd>{{ readable(book.engine) }}</dd></div></dl></article></div></section>
    <section v-if="events"><header><div><h2>Prediction markets</h2><p>Public order books → durable event registry → probability-shock signals. Inspect contracts in Market Atlas → Events &amp; signals.</p></div><span>{{events.config.poll_seconds}}s polling</span></header><div class="source-cards"><article v-for="source in events.sources" :key="source.id"><div class="source-title"><span class="source-monogram">◎</span><div><h3>{{readable(source.id)}}</h3><small>Public REST · research</small></div><b :data-ok="eventHealthy(source)">{{eventHealthy(source)?'Receiving':'Delayed / offline'}}</b></div><dl><div><dt>Watched contracts</dt><dd>{{source.markets}}</dd></div><div><dt>Two-sided quotes</dt><dd>{{source.quotes}}</dd></div><div><dt>Latest successful poll</dt><dd>{{ageLabel(source.last_ok_ms,now)}}</dd></div><div><dt>Detection</dt><dd>{{events.config.enabled?'Enabled':'Paused'}} · rule v{{events.config.revision}}</dd></div><div><dt>Storage</dt><dd>{{events.config.retention_days}} days locally · stream acknowledgements</dd></div></dl></article></div></section>
    <section><header><div><h2>Space weather</h2><p>NASA / NOAA observations for research. Their timestamps show observation age, not collector uptime.</p></div><span>60s collection</span></header><div class="science-list"><article v-for="series in scientific" :key="series.descriptor.id"><i :style="{background: series.descriptor.color}"></i><div><h3>{{ series.descriptor.label }}</h3><small>{{ series.descriptor.provenance }}</small></div><span>{{ latest(series) == null ? 'No sample in 24h' : ageLabel(latest(series), now) }}<small>Latest observation</small></span></article></div></section>
  </div>
</template>
<style scoped>
.source-monitor{padding:4px 28px 28px;min-width:0}.source-overview{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));border:1px solid var(--ui-border);border-radius:10px;overflow:hidden;margin:16px 0 28px;background:var(--ui-panel)}.source-overview>div{padding:20px 22px}.source-overview>div+div{border-left:1px solid var(--ui-border)}.source-overview span{display:block;font-size:11px;color:var(--ui-muted)}.source-overview strong{display:block;margin-top:10px;font-size:27px;font-weight:550;letter-spacing:-.035em}.source-overview strong small{font:12px var(--vp-font-family-base);color:var(--ui-muted);letter-spacing:0}.source-overview p{font-size:10px;margin:8px 0 0;color:var(--ui-subtle)}section+section{margin-top:28px}section>header{display:flex;justify-content:space-between;align-items:center;gap:14px;margin-bottom:14px}h2{font-size:16px;font-weight:550;margin:0}header p{color:var(--ui-muted);font-size:11px;line-height:1.6;margin:6px 0 0}header>span{white-space:nowrap;color:var(--ui-subtle);font-size:10px}article{border:1px solid var(--ui-border);background:var(--ui-panel);border-radius:9px;padding:18px}.source-cards{display:grid;grid-template-columns:repeat(auto-fit,minmax(min(100%,400px),1fr));gap:14px}.source-title{display:flex;align-items:center;gap:12px}h3{margin:0;font-size:13px;font-weight:550}.source-title small{display:block;margin-top:5px}.source-monogram{display:grid;place-items:center;width:35px;height:35px;background:#233244;border:1px solid #39506b;border-radius:8px;font-size:20px;color:#94bde6}small{font-size:10px;color:var(--ui-subtle)}b{margin-left:auto;font-weight:400;font-size:10px;color:#d4b080}b[data-ok=true]{color:#8fcea9}b:before{content:'';display:inline-block;width:5px;height:5px;background:currentColor;border-radius:50%;margin-right:6px}.source-instruments{display:flex;gap:6px;margin:16px 0}.source-instruments span{border:1px solid #374654;border-radius:4px;padding:5px 8px;font-size:10px;color:#b6cce0}dl{margin:12px 0 0}dl>div{display:flex;justify-content:space-between;gap:16px;padding:7px 0;font-size:11px}dt{color:var(--ui-muted)}dd{margin:0;text-align:right;overflow-wrap:anywhere}.news-grid{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:12px}.news-title{display:flex;align-items:center;justify-content:space-between;gap:8px}.science-list{border:1px solid var(--ui-border);border-radius:9px;overflow:hidden}.science-list article{display:flex;gap:12px;align-items:center;border:0;border-radius:0;padding:14px 18px}.science-list article+article{border-top:1px solid var(--ui-border)}.science-list i{width:6px;height:6px;border-radius:50%;flex:none}.science-list small{display:block;margin-top:5px;line-height:1.5}.science-list article>span{margin-left:auto;text-align:right;font-size:12px;flex:none}.source-warning,.empty{color:#d4b080;font-size:12px}@media(max-width:1100px){.news-grid{grid-template-columns:repeat(2,minmax(0,1fr))}}@media(max-width:720px){.source-monitor{padding:0 16px 20px}.source-overview{grid-template-columns:1fr}.source-overview>div+div{border-left:0;border-top:1px solid var(--ui-border)}.source-overview>div{padding:15px 18px}.news-grid{grid-template-columns:1fr}section>header>span{display:none}.science-list article{padding:14px 12px}.science-list article>div{min-width:0}.science-list article>div small{overflow-wrap:anywhere}}
</style>
