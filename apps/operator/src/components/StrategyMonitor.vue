<script setup lang="ts">
import OperatorIcon from "./OperatorIcon.vue";
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
          <div class="book-title"><span class="book-icon"><OperatorIcon name="forecasts" /></span><div><h4>{{ label(book.engine) }}</h4><small>{{ book.id }}</small></div><span class="mode">{{ book.mode }}</span><span class="health" :data-running="running(book)">{{ !fresh(book) ? 'Heartbeat delayed' : current.paused ? 'Paused' : label(book.state) }}</span></div>
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
.strategy-monitor{border:1px solid var(--ui-border);border-radius:var(--ui-radius);background:var(--ui-panel);color:var(--atlas-ink);min-width:0;overflow:hidden}.strategy-monitor>header{display:flex;align-items:center;justify-content:space-between;gap:16px;padding:14px 18px;border-bottom:1px solid var(--ui-border)}h3{font-size:15px;font-weight:550;letter-spacing:-.15px;margin:0}h4{font-size:14px;font-weight:550;margin:0}.status{display:flex;align-items:center;gap:7px;font-size:11px;color:#aab8c8;white-space:nowrap}.status:before{content:"";height:5px;width:5px;border-radius:50%;background:#8190a1}.status[data-active=true]{color:#83e3be}.status[data-active=true]:before{background:#83e3be}.strategy-body{display:grid;grid-template-columns:minmax(0,1.6fr) minmax(240px,1fr)}.books{padding:14px 18px}.book+.book{margin-top:20px;padding-top:20px;border-top:1px solid var(--ui-border)}.book-title{display:flex;align-items:center;gap:11px;margin-bottom:16px}.book-title small{display:block;color:var(--ui-subtle);font-size:10px;margin-top:5px}.book-icon{height:35px;width:35px;display:grid;place-items:center;background:#26342b;border:1px solid #3d4c3b;border-radius:9px;color:var(--ui-accent)}.book-icon svg{width:19px;height:19px}.mode{padding:4px 7px;border:1px solid #3a4758;border-radius:5px;background:#202b39;color:#b2c2d6;font-size:9px;letter-spacing:.05em;text-transform:uppercase;margin-left:auto}.health{display:flex;align-items:center;gap:5px;font-size:10px;color:#e4bd85}.health[data-running=true]{color:#83d6b9}.health[data-running=true]:before{content:"";height:4px;width:4px;border-radius:50%;background:currentColor}.signal{display:grid;grid-template-columns:80px 1fr;gap:12px;padding:10px 0;border-top:1px solid #293646;font-size:12px}.signal strong{font-weight:550}.signal>span{color:#a9b9ca;line-height:1.4}.signal small{color:#e4bd85}.book footer{font-size:10px;color:var(--ui-subtle);display:flex;gap:10px;flex-wrap:wrap;margin-top:12px;line-height:1.5}.book footer span{margin-left:auto}.strategy-evidence{border-left:1px solid var(--ui-border);padding:14px 18px;background:#111a25}.strategy-evidence h4{font-size:12px;color:#c3cedd}.strategy-evidence dl{margin:12px 0}.strategy-evidence dl>div{display:flex;justify-content:space-between;gap:12px;font-size:11px;padding:7px 0}.strategy-evidence dt{color:var(--ui-muted)}.strategy-evidence dd{margin:0;color:#dce7f2;font-variant-numeric:tabular-nums}details{border-top:1px solid var(--ui-border);padding-top:11px;font-size:11px;color:#cbb995}summary{cursor:pointer;line-height:1.8}summary span{display:inline-block;min-width:16px;text-align:center;margin-left:6px;border-radius:4px;background:#303027;color:#e4d4ab;font-size:10px}ul{padding-left:18px;line-height:1.8;color:var(--ui-muted)}.quiet,.unavailable{font-size:12px;line-height:1.6;color:var(--ui-muted);margin:14px 0 0}.unavailable{padding:0 20px 20px}
@media(max-width:1000px){.strategy-body{grid-template-columns:1fr}.strategy-evidence{border-left:0;border-top:1px solid var(--ui-border)}.strategy-evidence dl{display:flex;gap:24px;flex-wrap:wrap;margin:12px 0}.strategy-evidence dl>div{display:grid;gap:6px;padding:0}.strategy-evidence dd{font-size:16px}.book-title{flex-wrap:wrap}}
@media(max-width:600px){.strategy-monitor>header{padding:16px;gap:10px}h3{font-size:14px}.books,.strategy-evidence{padding:16px}.book-title{gap:8px}.signal{font-size:11px;grid-template-columns:72px 1fr}.health{font-size:9px}.mode{font-size:9px}.book footer span{margin-left:0}.status{font-size:10px}}
</style>
