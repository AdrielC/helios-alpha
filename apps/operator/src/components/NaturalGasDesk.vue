<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
interface Desk {
  online: boolean; observed_ms: number; execution_authority: false;
  counts: { evidence: number; quotes: number; decisions: number };
  sources: { source: string; state: string; checked_ms: number }[];
  storage: { period: string; release_date: string; storage_bcf: number; change_bcf: number; seasonal_average_bcf: number } | null;
  weather: { kind: string; period: string; total: number }[];
  signals: { kind: string; value: number; candidate: boolean; z_score: number | null }[];
  premiums: { contract_id: string; kind: string; expiry: string; strike_cents: number; bid_cents: number; ask_cents: number; premium_cents: number; spread_cost_cents: number; observed_ms: number; fresh: boolean }[];
  hypotheses: { id: string; signal: string; requires: string }[];
  exit_profiles: { id: string; take_profit_bps: number; holding_ms: number; stop_loss_bps: number }[]; max_quote_age_ms: number;
  decision_enabled: boolean; backtest: { blockers: string[] };
}
const data = ref<Desk | null>(null), error = ref(false), now = ref(Date.now());
let timer: ReturnType<typeof setInterval> | undefined, busy = false, disposed = false;
const online = computed(() => !error.value && !!data.value?.online && now.value-data.value.observed_ms <= 15000);
const money = (cents: number) => new Intl.NumberFormat("en-US", { style: "currency", currency: "USD" }).format(cents/100);
const date = (ms: number) => new Date(ms).toLocaleString(undefined, { month: "short", day: "numeric", hour: "numeric", minute: "2-digit", timeZoneName: "short" });
async function refresh() {
  if (busy || disposed) return;
  busy = true;
  try {
    const response = await fetch("/api/v1/research/natural-gas", { cache: "no-store", signal: AbortSignal.timeout(5000) });
    if (!response.ok) throw new Error("Unavailable");
    const next = await response.json();
    if (next.schema_version !== 1 || next.execution_authority !== false) throw new Error("Invalid projection");
    if (!disposed) { data.value = next.counts ? next : null; error.value = false; }
  } catch { if (!disposed) error.value = true; }
  finally { busy = false; }
}
onMounted(() => { void refresh(); timer = setInterval(() => { now.value = Date.now(); void refresh(); }, 5000); });
onBeforeUnmount(() => { disposed = true; if (timer) clearInterval(timer); });
</script>

<template>
  <details class="gas-desk" data-testid="natural-gas-desk">
    <summary>
      <span class="gas-mark" aria-hidden="true">NG</span>
      <span class="gas-title"><strong>Natural gas</strong><small>UNG · Event research</small></span>
      <span class="gas-summary" v-if="data">{{ data.counts.evidence }} event vintages <i>·</i> {{ data.premiums.length }} option quotes</span>
      <span class="gas-state" :class="{ online }">{{ online ? 'Capturing' : 'Capture offline' }}</span>
      <span class="gas-open">Explore <span aria-hidden="true">↗</span></span>
    </summary>
    <div class="gas-content" v-if="data">
      <div class="gas-metrics">
        <article><small>LOWER 48 STORAGE</small><strong>{{ data.storage ? `${data.storage.storage_bcf.toLocaleString()} Bcf` : 'Awaiting release' }}</strong><span v-if="data.storage">{{ data.storage.change_bcf > 0 ? '+' : '' }}{{ data.storage.change_bcf }} Bcf · week ending {{ data.storage.period }}</span><span v-else>EIA weekly storage report</span></article>
        <article v-for="kind in ['hdd', 'cdd']" :key="kind"><small>{{ kind === 'hdd' ? 'HEATING DEMAND' : 'COOLING DEMAND' }}</small><strong>{{ data.weather.find(w => w.kind === kind)?.total ?? '—' }} <em>degree days</em></strong><span>7-day forecast · {{ data.weather.find(w => w.kind === kind)?.period ?? 'Waiting for NOAA' }}</span></article>
        <article><small>DECISION REVIEW</small><strong>{{ data.decision_enabled ? 'Event gated' : 'Offline evaluation' }}</strong><span>{{ data.counts.decisions }} model receipts · research only</span></article>
      </div>
      <div class="gas-body">
        <section class="gas-premiums"><header><h2>Option premiums</h2><span>One contract · before fees</span></header>
          <p v-if="!data.premiums.length" class="gas-empty">Waiting for Robinhood option quotes. Capture starts with the connected account monitor.</p>
          <div v-else class="gas-table"><table><thead><tr><th>Contract</th><th>Bid / ask</th><th>Buy at ask</th><th>Spread cost</th><th>Quote time</th></tr></thead><tbody><tr v-for="q in data.premiums" :key="q.contract_id"><td><b>{{ money(q.strike_cents) }} {{ q.kind }}</b><small>{{ q.expiry }}</small></td><td>{{ money(q.bid_cents) }} / {{ money(q.ask_cents) }}</td><td class="gas-price">{{ money(q.premium_cents) }}</td><td>{{ money(q.spread_cost_cents) }}</td><td><span>{{ date(q.observed_ms) }}</span><small :class="{ 'gas-old': !online || !q.fresh || now-q.observed_ms > data.max_quote_age_ms }">{{ online && q.fresh && now-q.observed_ms <= data.max_quote_age_ms ? 'Fresh quote' : 'Dated quote' }}</small></td></tr></tbody></table></div>
          <p class="gas-note">Bid/ask spreads reduce returns. A quoted ask is an observation, not a guaranteed fill. UNG tracks a rolling futures portfolio.</p>
        </section>
        <section class="gas-signals"><header><h2>Event signals</h2><span>Evidence first</span></header>
          <div v-if="!data.signals.length" class="gas-empty">Building the first comparable forecast pair. A single weather vintage cannot establish a revision.</div>
          <div v-for="s in data.signals" :key="s.kind" class="gas-signal"><b>{{ s.kind.toUpperCase() }} revision</b><strong>{{ s.value > 0 ? '+' : '' }}{{ s.value }}</strong><span>{{ s.candidate ? 'Review candidate' : 'Below gate or aged' }} · {{ s.z_score === null ? 'Baseline warming up' : `${s.z_score.toFixed(2)} σ` }}</span></div>
          <ul class="gas-sources"><li v-for="s in data.sources" :key="s.source"><span>{{ s.source }}</span><b :class="{ 'gas-old': s.state !== 'receiving' || !online }">{{ online ? s.state : 'offline' }}</b></li></ul>
        </section>
      </div>
      <div class="gas-research"><section><h2>Research playbook</h2><dl><div v-for="h in data.hypotheses" :key="h.id"><dt>{{ h.id.replaceAll('-', ' ') }}</dt><dd>{{ h.signal }}<small>{{ h.requires }}</small></dd></div></dl></section><section><h2>Backtest readiness</h2><p>Profitability has not been established.</p><ul><li v-for="b in data.backtest.blockers" :key="b">{{ b }}</li></ul><p>Exit studies: {{ data.exit_profiles.map(p => `${p.id.replaceAll('-', ' ')} (+${p.take_profit_bps/100}% target)`).join(', ') }}. These are simulation inputs, not forecasts.</p></section></div>
    </div>
    <p v-else class="gas-empty">{{ error ? 'Research refresh interrupted. Retrying automatically.' : 'Natural-gas capture has not published a snapshot yet.' }}</p>
  </details>
</template>

<style scoped>
.gas-desk{margin:0 0 16px;border:1px solid #2a393b;border-radius:12px;background:linear-gradient(105deg,#15231f,#101a22);color:#e9eeea;font-size:12px;overflow:hidden}.gas-desk summary{display:flex;align-items:center;gap:14px;padding:16px 20px;cursor:pointer;list-style:none}.gas-desk summary::-webkit-details-marker{display:none}.gas-desk summary:focus-visible{outline:2px solid #c9f584;outline-offset:-3px}.gas-mark{display:grid;place-items:center;border:1px solid #668b50;color:#d3f6a5;border-radius:9px;width:38px;height:38px;font-size:12px;font-weight:800}.gas-title{display:grid;gap:5px}.gas-title strong{font-size:17px;letter-spacing:-.4px}.gas-title small,.gas-summary,.gas-note{color:#a3b4b3}.gas-summary{margin-left:auto}.gas-summary i{font-style:normal;margin:0 10px}.gas-state{font-size:10px;text-transform:uppercase;letter-spacing:.7px;color:#bdc8c6;border:1px solid #46544e;border-radius:20px;padding:5px 9px}.gas-state.online{color:#cff491;border-color:#526b39}.gas-open{color:#d3f6a5}.gas-content{border-top:1px solid #2b3c37}.gas-metrics{display:grid;grid-template-columns:repeat(4,minmax(0,1fr));padding:20px;gap:20px}.gas-metrics article{display:grid;gap:9px}.gas-metrics small{color:#9eaaa8;font-size:9px;letter-spacing:1.2px}.gas-metrics strong{font-size:22px;letter-spacing:-.5px}.gas-metrics em{font-size:12px;font-weight:400;font-style:normal;color:#a8b3af}.gas-metrics span{font-size:10px;color:#9eaaa8}.gas-body{display:grid;grid-template-columns:minmax(0,2fr) minmax(230px,1fr);gap:1px;background:#293634;border-block:1px solid #293634}.gas-body section{min-width:0;background:#101a20;padding:20px}.gas-body header{display:flex;align-items:center;justify-content:space-between;gap:10px;margin-bottom:18px}.gas-desk h2{font-size:13px;margin:0;font-weight:650}.gas-body header span{font-size:10px;color:#a5b4b2}.gas-table{overflow:auto}.gas-table table{width:100%;border-collapse:collapse;white-space:nowrap}.gas-table th{font-size:9px;text-transform:uppercase;letter-spacing:.5px;color:#9baaaa;text-align:left;font-weight:500;padding:0 12px 10px 0}.gas-table td{font-size:11px;padding:12px 12px 12px 0;border-top:1px solid #253234;font-variant-numeric:tabular-nums}.gas-table td small{display:block;color:#9baaaa;font-size:10px;margin-top:4px}.gas-table .gas-price{color:#d7f4ac;font-weight:700}.gas-table .gas-old,.gas-old{color:#c9ad7d}.gas-note{font-size:10px;line-height:1.6;margin:14px 0 0}.gas-empty{padding:20px;color:#a9b9b4;line-height:1.7;margin:0}.gas-signals .gas-empty{padding:0 0 18px}.gas-sources{list-style:none;margin:16px 0 0;padding:0;border-top:1px solid #293634}.gas-sources li{display:flex;justify-content:space-between;padding:10px 0;color:#b1c0ba;font-size:10px}.gas-sources b{font-weight:500}.gas-signal{display:flex;gap:10px;align-items:center;flex-wrap:wrap;margin:12px 0}.gas-signal span{width:100%;color:#a9b9b4;font-size:10px}.gas-research{display:grid;grid-template-columns:2fr 1fr;gap:32px;padding:22px;background:#10181d}.gas-research dl{display:grid;gap:14px;margin-bottom:0}.gas-research dt{text-transform:capitalize;font-size:11px;color:#d1e4bc;margin-bottom:4px}.gas-research dd{margin:0;color:#bfcbc7;font-size:11px;line-height:1.6}.gas-research small{display:block;color:#8faaa0}.gas-research p,.gas-research li{font-size:11px;line-height:1.7;color:#a7b9b1}.gas-research ul{padding-left:16px}.gas-research li{margin:10px 0}@media(max-width:900px){.gas-summary{display:none}.gas-state{margin-left:auto}.gas-body,.gas-research{grid-template-columns:1fr}.gas-metrics{grid-template-columns:repeat(2,minmax(0,1fr))}}@media(max-width:500px){.gas-desk summary{padding:12px;gap:8px}.gas-open{display:none}.gas-title strong{font-size:14px}.gas-state{font-size:8px}.gas-metrics strong{font-size:17px}}
</style>
