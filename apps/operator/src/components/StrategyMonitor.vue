<script setup lang="ts">
import { computed } from "vue";
import type { BrokerSync, StrategyBook } from "../operations/broker-holdings";
const props = defineProps<{ sync: BrokerSync; now: number }>();
const current = computed(() => {
  const state = props.sync.strategy;
  return state && props.now >= state.observed_ms && props.now - state.observed_ms <= 15_000 ? state : undefined;
});
const fresh = (book: StrategyBook) => props.now >= book.updated_ms && props.now - book.updated_ms <= 30_000;
const running = (book: StrategyBook) => fresh(book) && book.state === "running" && !current.value?.paused;
const live = computed(() => current.value?.live_admitted ? current.value.books.filter(book => book.mode === "live" && running(book)).length : 0);
const labels: Record<string, string> = {
  news_shock: "News shock", momentum: "Momentum", no_price_confirmation: "Waiting for price confirmation",
  no_news: "Waiting for a qualifying event", reference_quote_unavailable: "Reference quote unavailable",
  dedicated_broker_credentials_not_validated: "Broker execution credentials need validation",
  account_execution_recovery_not_certified: "Order recovery needs certification",
  strategy_evidence_not_admitted: "Strategy evidence has not passed live admission",
};
const label = (text: string) => labels[text] ?? text.replaceAll("_", " ");
</script>

<template>
  <section class="strategy-monitor" aria-label="Crypto strategy monitor">
    <header><div><h3>Strategies &amp; signals</h3></div><span class="status" :data-active="live > 0">{{ current ? `${live} live strategies` : 'Status unavailable' }}</span></header>
    <div v-if="current" class="strategy-body">
      <div class="books">
        <article v-for="book in current.books" :key="book.id" class="book">
          <div class="book-title"><span class="book-icon" aria-hidden="true">↗</span><div><h4>{{ label(book.engine) }}</h4><small>{{ book.id }}</small></div><span class="mode">{{ book.mode }}</span><span class="health" :data-running="running(book)">{{ !fresh(book) ? 'Heartbeat delayed' : current.paused ? 'Paused' : label(book.state) }}</span></div>
          <div class="signal" v-for="signal in book.signals" :key="signal.symbol"><strong>{{ signal.symbol }}</strong><span>{{ label(signal.reason) }}<small v-if="!fresh(book)"> · last reported</small></span></div>
          <p v-if="!book.signals.length" class="quiet">No signal reported yet.</p>
          <footer>{{ book.active_news }} active news events <span>Feed {{ fresh(book) ? book.feed_connected ? 'connected' : 'disconnected' : 'unverified' }} · heartbeat {{ Math.max(0, Math.floor((now - book.updated_ms) / 1000)) }}s ago</span></footer>
        </article>
        <p v-if="!current.books.length" class="quiet">No strategy runtimes are registered with Golem.</p>
      </div>
      <aside class="strategy-evidence">
        <h4>Execution</h4>
        <dl><div><dt>Paper orders</dt><dd>{{ current.paper_orders }}</dd></div><div><dt>Paper positions</dt><dd>{{ current.paper_positions }}</dd></div><div><dt>Model calls reserved</dt><dd>{{ current.model_calls_reserved }} / {{ current.model_daily_limit }} daily</dd></div></dl>
        <details v-if="current.live_blockers.length"><summary>Live readiness <span>{{ current.live_blockers.length }}</span></summary><ul><li v-for="blocker in current.live_blockers" :key="blocker">{{ label(blocker) }}</li></ul></details>
      </aside>
    </div>
    <p v-else class="unavailable" role="status">Golem strategy status is unavailable or delayed. Active books and exit signals cannot be verified.</p>
  </section>
</template>

<style scoped>
.strategy-monitor{border:1px solid #27303f;border-radius:10px;background:#10161f;color:#e7edf5;min-width:0}.strategy-monitor>header{display:flex;align-items:center;justify-content:space-between;gap:16px;padding:16px 20px;border-bottom:1px solid #27303f}.eyebrow{font-size:10px;letter-spacing:.12em;color:#94a3ba}h3{font-size:17px;letter-spacing:-.2px;margin:6px 0 0}h4{font-size:14px;margin:0}.status{font-size:12px;color:#e4ba7c;background:#2b261f;padding:6px 9px;border-radius:5px;white-space:nowrap}.status[data-active=true]{color:#7de2b8;background:#173027}.strategy-body{display:grid;grid-template-columns:minmax(0,1.5fr) minmax(260px,1fr)}.books{padding:16px 20px}.book+.book{margin-top:20px;padding-top:20px;border-top:1px solid #27303f}.book-title{display:flex;align-items:center;gap:10px;margin-bottom:16px}.book-title small{display:block;color:#7f91aa;font-size:10px;margin-top:4px}.book-icon{width:34px;height:34px;display:grid;place-items:center;font-size:22px;background:#1d2639;border-radius:8px;color:#9cadff}.mode{padding:4px 7px;border-radius:4px;background:#202738;color:#c4cceb;font-size:10px;text-transform:uppercase;margin-left:auto}.health{font-size:11px;color:#e4ba7c}.health[data-running=true]{color:#79d2b0}.signal{display:grid;grid-template-columns:95px 1fr;gap:14px;padding:11px 0;border-top:1px solid #232c3b;font-size:12px}.signal strong{font-family:var(--vp-font-family-mono);font-weight:500}.signal>span{color:#b3c0d4}.signal small{color:#e4ba7c}.book footer{font-size:10px;color:#8596ad;display:flex;gap:12px;flex-wrap:wrap;margin-top:12px}.book footer span{margin-left:auto}.strategy-evidence{border-left:1px solid #27303f;padding:18px 20px}.strategy-evidence dl{margin:15px 0}.strategy-evidence dl>div{display:flex;justify-content:space-between;gap:15px;font-size:12px;padding:6px 0}.strategy-evidence dt{color:#91a2b8}.strategy-evidence dd{margin:0;font-family:var(--vp-font-family-mono)}details{border-top:1px solid #27303f;padding-top:12px;font-size:12px;color:#c6b192}summary{cursor:pointer;line-height:1.6}summary span{margin-left:6px}ul{padding-left:18px;line-height:1.8;color:#94a3ba}.quiet,.unavailable{font-size:12px;line-height:1.6;color:#91a2b8;margin:14px 0 0}.unavailable{padding:0 22px 22px}@media(max-width:900px){.strategy-body{grid-template-columns:1fr}.strategy-evidence{border-left:0;border-top:1px solid #27303f}.strategy-monitor>header{flex-wrap:wrap}.book-title{flex-wrap:wrap}.health{margin-left:0}}

.strategy-monitor{border-radius:8px}.strategy-monitor>header{padding:13px 16px}h3{margin:0;font-size:15px}h4{font-size:13px}.books{padding:14px 16px}.strategy-evidence{padding:14px 16px}.strategy-evidence dl{margin:10px 0}.book-title{margin-bottom:10px}.book-icon{height:30px;width:30px}.signal{padding:9px 0}.status{font-size:11px}.book-title small{font-size:10px}.book footer{font-size:11px;line-height:1.5}.strategy-evidence details{padding-top:10px}
@media(max-width:900px){.strategy-evidence dl{display:flex;gap:20px;flex-wrap:wrap}.strategy-evidence dl>div{display:grid;gap:5px}.strategy-evidence dd{font-size:14px}.strategy-body{grid-template-columns:1fr}}
</style>
