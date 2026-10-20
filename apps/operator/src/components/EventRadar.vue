<script setup lang="ts">
import {
  computed,
  nextTick,
  onBeforeUnmount,
  onMounted,
  ref,
  watch,
} from "vue";
import {
  eventUrl,
  loadEventRadar,
  setEventDetection,
  type EventMarket,
  type EventRadarSnapshot,
} from "../operations/event-radar-port";
const data = ref<EventRadarSnapshot | null>(null),
  error = ref(""),
  loading = ref(true),
  now = ref(Date.now());
const query = ref(""),
  provider = ref("all"),
  selectedId = ref(""),
  pending = ref("");
const detailDialog = ref<HTMLDialogElement>();
let pendingAt = 0;
watch(selectedId, async (id) => {
  if (id) {
    await nextTick();
    detailDialog.value?.showModal();
  }
});
let timer: ReturnType<typeof setInterval> | undefined,
  busy = false,
  disposed = false;
const online = computed(() =>
  Boolean(data.value?.online && now.value - data.value.observed_ms < 15000),
);
const selected = computed(() =>
  data.value?.markets.find((item) => item.id === selectedId.value),
);
const markets = computed(() =>
  [...(data.value?.markets ?? [])]
    .filter(
      (item) =>
        (provider.value === "all" || item.provider === provider.value) &&
        `${item.title} ${item.topic} ${item.symbols.join(" ")}`
          .toLowerCase()
          .includes(query.value.toLowerCase()),
    )
    .sort(
      (a, b) =>
        Number(b.state === "triggered") - Number(a.state === "triggered") ||
        Math.abs(b.delta_pp ?? 0) - Math.abs(a.delta_pp ?? 0) ||
        a.provider.localeCompare(b.provider) ||
        b.volume_24h - a.volume_24h,
    ),
);
const ready = computed(
  () =>
    data.value?.markets.filter((item) =>
      ["watching", "triggered", "cooldown"].includes(state(item)),
    ).length ?? 0,
);
function state(item: EventMarket): string {
  return !online.value
    ? "offline"
    : item.latest &&
        now.value - item.latest.observed_ms >
          Math.max(90000, (data.value?.config.poll_seconds ?? 30) * 3000)
      ? "stale"
      : item.state;
}
const readable = (value: string) => value.replaceAll("_", " ");
const percent = (value: number | undefined | null) =>
  value == null ? "—" : `${(value * 100).toFixed(1)}%`;
const move = (value: number | null) =>
  value === null
    ? "Building baseline"
    : `${value > 0 ? "+" : ""}${value.toFixed(1)} pp`;
const age = (at: number | undefined | null) => {
  if (at == null) return "Not observed";
  const s = Math.max(0, Math.floor((now.value - at) / 1000));
  return s < 60
    ? `${s}s ago`
    : s < 3600
      ? `${Math.floor(s / 60)}m ago`
      : s < 86400
        ? `${Math.floor(s / 3600)}h ago`
        : `${Math.floor(s / 86400)}d ago`;
};
const money = (value: number) =>
  new Intl.NumberFormat("en-US", {
    style: "currency",
    currency: "USD",
    maximumFractionDigits: 0,
  }).format(value);
function sparks(item: EventMarket): string[] {
  const points = item.history;
  if (points.length < 2) return [];
  const from = points[0].at_ms,
    span = Math.max(1, points.at(-1)!.at_ms - from);
  const segments: string[][] = [[]];
  const gap = (data.value?.config.poll_seconds ?? 30) * 3000;
  for (const [index, point] of points.entries()) {
    if (index && point.at_ms - points[index - 1].at_ms > gap) segments.push([]);
    segments
      .at(-1)!
      .push(
        `${((point.at_ms - from) / span) * 600},${130 - point.probability * 120}`,
      );
  }
  return segments
    .filter((segment) => segment.length > 1)
    .map((segment) => segment.join(" "));
}

async function refresh() {
  if (busy) return;
  busy = true;
  try {
    const next = await loadEventRadar();
    if (disposed) return;
    data.value = next;
    error.value = "";
    if (pending.value && next?.last_command?.id === pending.value) {
      if (next.last_command.state !== "applied")
        error.value =
          "Detection change was rejected. Refresh the current settings.";
      pending.value = "";
    } else if (pending.value && Date.now() - pendingAt > 35000) {
      pending.value = "";
      error.value =
        "The change could not be confirmed. The displayed setting is the last acknowledged state.";
    }
  } catch (cause) {
    if (!disposed)
      error.value =
        cause instanceof Error
          ? cause.message
          : "Event observations unavailable.";
  } finally {
    busy = false;
    loading.value = false;
  }
}
async function toggle() {
  if (!data.value || pending.value) return;
  pending.value = "sending";
  pendingAt = Date.now();
  try {
    pending.value = await setEventDetection(
      !data.value.config.enabled,
      data.value.config.revision,
    );
    await refresh();
  } catch (cause) {
    pending.value = "";
    error.value =
      cause instanceof Error ? cause.message : "Detection change failed.";
  }
}
onMounted(() => {
  void refresh();
  timer = setInterval(() => {
    now.value = Date.now();
    if (!document.hidden) void refresh();
  }, 3000);
});
onBeforeUnmount(() => {
  disposed = true;
  clearInterval(timer);
});
</script>
<template>
  <section class="event-radar" aria-label="Event radar">
    <div class="radar-intro">
      <div>
        <span class="eyebrow">EVENT INTELLIGENCE</span>
        <h2>Events &amp; signals</h2>
        <p>
          Prediction markets → recorded observations → rule triggers → research
          signals
        </p>
      </div>
      <div class="detector">
        <span :data-online="online">{{
          online ? "Collector online" : "Collector offline"
        }}</span
        ><button
          role="switch"
          :aria-checked="data?.config.enabled ?? false"
          :disabled="!online || Boolean(pending)"
          @click="toggle"
        >
          <i></i
          >{{
            pending
              ? "Saving…"
              : data?.config.enabled
                ? "Detection on"
                : "Detection paused"
          }}
        </button>
      </div>
    </div>
    <p v-if="error" class="radar-error" role="alert">
      {{ error }} <button @click="refresh">Refresh</button>
    </p>
    <div v-if="loading" class="radar-empty">Loading the event registry…</div>
    <div v-else-if="!data" class="radar-empty">
      The event collector has not published observations yet.
    </div>
    <template v-else>
      <div class="radar-metrics">
        <div>
          <span>Events registered</span
          ><strong>{{ data.registered_events }}</strong
          ><small>{{ data.markets.length }} watched contracts</small>
        </div>
        <div>
          <span>Recorded observations</span
          ><strong>{{ data.recorded_observations.toLocaleString() }}</strong
          ><small>{{ data.config.retention_days }} days retained locally</small>
        </div>
        <div>
          <span>Ready to detect</span
          ><strong
            >{{ ready }}<small> / {{ data.markets.length }}</small></strong
          ><small>Baseline and quote checks</small>
        </div>
        <div>
          <span>Recent signals</span><strong>{{ data.triggers.length }}</strong
          ><small
            >{{ data.pending_signals }} awaiting stream acknowledgement</small
          >
        </div>
      </div>
      <div class="event-sources">
        <span
          v-for="source in data.sources"
          :key="source.id"
          :data-ok="
            online &&
            source.state === 'receiving' &&
            source.last_ok_ms !== null &&
            now - source.last_ok_ms < 90000
          "
          ><i></i><b>{{ source.id }}</b
          >{{ source.quotes }} / {{ source.markets }} quotes ·
          {{ age(source.last_ok_ms) }}</span
        ><span :data-ok="online && data.transport.state === 'ready'"
          ><i></i>JetStream · {{ data.transport.state }}</span
        ><span :data-ok="online && data.golem.state === 'recorded'"
          ><i></i>Golem · {{ data.golem.state }}</span
        >
      </div>
      <div class="rule-strip" aria-label="Event trigger rule">
        <span class="rule-label">PROBABILITY SHOCK</span
        ><strong
          >±{{ data.config.move_pp }} pp
          <small>in {{ data.config.lookback_seconds / 60 }}m</small></strong
        ><span>Spread ≤ {{ data.config.max_spread_pp }} pp</span
        ><span>Top-book depth ≥ {{ money(data.config.min_depth_usd) }}</span
        ><span
          >{{ data.config.cooldown_seconds / 60 }}m cooldown · reset ≤
          {{ data.config.reset_pp }} pp</span
        ><b>Research signals · execution disabled</b>
      </div>
      <div class="radar-tools">
        <div role="group" aria-label="Prediction market providers">
          <button
            v-for="id in ['all', 'polymarket', 'kalshi']"
            :key="id"
            :aria-pressed="provider === id"
            @click="provider = id"
          >
            {{ id === "all" ? "All markets" : id }}
          </button>
        </div>
        <input
          v-model="query"
          aria-label="Search events"
          placeholder="Search events, topics, or assets"
          type="search"
        /><span>Quotes every {{ data.config.poll_seconds }}s</span>
      </div>
      <div
        class="event-table"
        role="table"
        aria-label="Prediction market registry"
      >
        <div class="event-columns" role="row">
          <span role="columnheader">Event / contract</span
          ><span role="columnheader">Yes midpoint</span
          ><span role="columnheader"
            >{{ data.config.lookback_seconds / 60 }}m change</span
          ><span role="columnheader">Spread / depth</span
          ><span role="columnheader">Detector</span>
        </div>
        <article
          v-for="item in markets"
          :key="item.id"
          :data-selected="selectedId === item.id"
          role="row"
        >
          <div role="cell">
            <button
              class="event-title"
              :aria-expanded="selectedId === item.id"
              @click="selectedId = selectedId === item.id ? '' : item.id"
            >
              {{ item.title }}</button
            ><small
              ><b>{{ item.provider }}</b> · {{ readable(item.topic) }} ·
              {{ item.symbols.join(" / ") }}</small
            >
          </div>
          <div role="cell" class="event-probability">
            <strong>{{ percent(item.latest?.probability) }}</strong
            ><span
              ><i
                :style="{ width: `${(item.latest?.probability ?? 0) * 100}%` }"
              ></i
            ></span>
          </div>
          <div
            role="cell"
            class="event-delta"
            :data-direction="
              item.delta_pp !== null && item.delta_pp > 0 ? 'up' : 'down'
            "
          >
            {{ move(item.delta_pp) }}
          </div>
          <div role="cell" class="quote-quality">
            {{
              item.latest
                ? `${item.latest.spread_pp.toFixed(1)} pp · ${money(item.latest.depth_usd)}`
                : "No two-sided quote"
            }}<small>{{ age(item.latest?.observed_ms) }}</small>
          </div>
          <div role="cell">
            <span class="event-state" :data-state="state(item)">{{
              readable(state(item))
            }}</span>
          </div>
        </article>
        <p v-if="!markets.length" class="radar-empty">
          No contracts match this view.
        </p>
      </div>
      <dialog
        v-if="selected"
        ref="detailDialog"
        class="event-detail"
        aria-label="Selected event"
        @close="selectedId = ''"
        @click="
          (event) => {
            if (event.target === detailDialog) selectedId = '';
          }
        "
      >
        <header>
          <div>
            <span>{{ selected.provider }} · {{ selected.market_id }}</span>
            <h3>{{ selected.title }}</h3>
          </div>
          <button @click="selectedId = ''" aria-label="Close event details">
            ×
          </button>
        </header>
        <div class="detail-body">
          <div class="probability-chart">
            <div>
              <strong>{{ percent(selected.latest?.probability) }}</strong
              ><span>Yes midpoint · observed history</span>
            </div>
            <svg
              v-if="selected.history.length > 1"
              viewBox="0 0 600 145"
              preserveAspectRatio="none"
              role="img"
              aria-label="Observed probability history"
            >
              <path d="M0 70H600" class="midline" />
              <polyline
                v-for="(segment, index) in sparks(selected)"
                :key="index"
                :points="segment"
              />
            </svg>
            <p v-else>
              Recording the first observations. The chart appears after the next
              sample.
            </p>
            <footer>
              <span>0–100% scale · gaps are unobserved</span
              ><span>{{ selected.samples }} recorded samples</span>
            </footer>
          </div>
          <dl>
            <div>
              <dt>Bid / ask</dt>
              <dd>
                {{ percent(selected.latest?.bid) }} /
                {{ percent(selected.latest?.ask) }}
              </dd>
            </div>
            <div>
              <dt>24h volume</dt>
              <dd>
                {{
                  selected.volume_24h.toLocaleString(undefined, {
                    maximumFractionDigits: 0,
                  })
                }}
                {{ selected.volume_unit }}
              </dd>
            </div>
            <div>
              <dt>Market closes</dt>
              <dd>{{ new Date(selected.closes_ms).toLocaleString() }}</dd>
            </div>
            <div>
              <dt>Contract identity</dt>
              <dd>{{ selected.event_id }}</dd>
            </div>
            <div>
              <dt>Source</dt>
              <dd>
                <a
                  v-if="eventUrl(selected.url)"
                  :href="eventUrl(selected.url)"
                  target="_blank"
                  rel="noopener noreferrer"
                  >View market ↗</a
                >
              </dd>
            </div>
          </dl>
        </div>
        <details v-if="selected.resolution" class="resolution-terms">
          <summary>Resolution terms</summary>
          <p>{{ selected.resolution }}</p>
        </details>
        <p class="detail-note">
          Midpoints reflect market prices, not a calibrated Helios forecast.
          Each contract keeps its own identity and resolution terms; related
          markets are not automatically treated as equivalent.
        </p>
      </dialog>
      <section class="signal-journal" aria-label="Event signal journal">
        <header>
          <h3>Signal journal</h3>
          <span>{{ data.transport.subject }}</span>
        </header>
        <div v-if="!data.triggers.length" class="journal-empty">
          <span>↗</span>
          <div>
            <strong>Waiting for a qualifying change</strong>
            <p>
              A signal needs a full
              {{ data.config.lookback_seconds / 60 }}-minute baseline, fresh
              quotes, and the checks above. Startup data does not trigger a
              signal.
            </p>
          </div>
        </div>
        <article v-for="signal in data.triggers" :key="signal.id">
          <span
            class="event-state"
            :data-state="now < signal.expires_ms ? 'triggered' : 'cooldown'"
            >{{ now < signal.expires_ms ? "triggered" : "expired" }}</span
          >
          <div>
            <strong>{{ signal.title }}</strong
            ><small
              >{{ move(signal.delta_pp) }} · {{ signal.symbols.join(" / ") }} ·
              rule v{{ signal.rule_revision }} ·
              {{ age(signal.received_ms) }}</small
            >
          </div>
          <b>{{
            signal.delivered
              ? `Recorded in stream #${signal.stream_sequence}`
              : "Pending acknowledgement"
          }}</b>
        </article>
      </section>
      <details class="news-register">
        <summary>
          News register
          <span
            >{{ data.news.length }} latest recorded headlines · existing 6-feed
            intake</span
          >
        </summary>
        <article v-for="news in data.news" :key="news.news_id">
          <span>{{ news.source }}</span
          ><a
            :href="eventUrl(news.url)"
            target="_blank"
            rel="noopener noreferrer"
            >{{ news.title }}</a
          ><small
            >Published {{ age(news.published_ms) }} · received
            {{ age(news.observed_ms) }}</small
          >
        </article>
      </details>
    </template>
  </section>
</template>
<style scoped>
.event-radar {
  color: #dce4ee;
  padding-bottom: 24px;
}
.radar-intro {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 20px;
  padding: 18px 0 22px;
}
.eyebrow {
  color: #b9d694;
  font-size: 9px;
  letter-spacing: 0.16em;
}
.radar-intro h2 {
  font-size: 24px;
  font-weight: 550;
  letter-spacing: -0.035em;
  margin: 8px 0;
}
.radar-intro p {
  color: #95a5b6;
  font-size: 12px;
  margin: 0;
}
.detector {
  display: flex;
  gap: 12px;
  align-items: center;
  font-size: 11px;
}
.detector > span {
  color: #bba281;
}
.detector > span[data-online="true"] {
  color: #9bc6ad;
}
button {
  font: inherit;
  cursor: pointer;
  color: inherit;
}
.detector button {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 14px;
  border: 1px solid #43513b;
  border-radius: 8px;
  background: #1a241a;
}
.detector button i {
  width: 26px;
  height: 14px;
  border-radius: 9px;
  background: #556070;
  position: relative;
}
.detector button i:after {
  content: "";
  position: absolute;
  left: 2px;
  top: 2px;
  width: 10px;
  height: 10px;
  background: #dce6ea;
  border-radius: 50%;
}
.detector button[aria-checked="true"] i {
  background: #a7ce78;
}
.detector button[aria-checked="true"] i:after {
  left: 14px;
  background: #192315;
}
button:disabled {
  opacity: 0.5;
  cursor: wait;
}
.radar-metrics {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 12px;
}
.radar-metrics > div {
  display: grid;
  gap: 8px;
  padding: 17px 19px;
  border: 1px solid #2e3843;
  border-radius: 11px;
  background: linear-gradient(120deg, #19222a, #121a23);
}
.radar-metrics span,
.radar-metrics small {
  font-size: 10px;
  color: #95a8b9;
}
.radar-metrics strong {
  font-size: 27px;
  font-weight: 550;
  letter-spacing: -0.03em;
}
.radar-metrics strong small {
  font-size: 15px;
}
.event-sources {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 16px;
  padding: 16px 2px;
  font-size: 10px;
  color: #8ea0b3;
}
.event-sources > span {
  display: flex;
  align-items: center;
  gap: 7px;
}
.event-sources b {
  text-transform: capitalize;
  color: #cad6de;
  font-weight: 500;
}
.event-sources i {
  width: 5px;
  height: 5px;
  border-radius: 50%;
  background: #bd8e61;
}
.event-sources [data-ok="true"] i {
  background: #90c7a2;
}
.rule-strip {
  display: flex;
  align-items: center;
  gap: 18px;
  flex-wrap: wrap;
  border: 1px solid #3b4349;
  border-radius: 9px;
  padding: 13px 16px;
  background: #171f27;
  font-size: 11px;
  color: #a1b1bf;
}
.rule-label {
  font-size: 8px;
  letter-spacing: 0.12em;
  color: #b4cb94;
}
.rule-strip strong {
  font-size: 17px;
  color: #e3eacb;
}
.rule-strip small {
  font-size: 11px;
  font-weight: 400;
  color: #b9c4b1;
}
.rule-strip b {
  margin-left: auto;
  font-size: 9px;
  font-weight: 400;
  color: #9aaabd;
}
.radar-tools {
  display: flex;
  align-items: center;
  gap: 14px;
  margin: 24px 0 12px;
}
.radar-tools > div {
  display: flex;
  padding: 3px;
  border: 1px solid #2c3844;
  border-radius: 7px;
  background: #131b23;
}
.radar-tools button {
  padding: 7px 12px;
  border: 0;
  background: none;
  color: #8fa2b6;
  text-transform: capitalize;
  font-size: 11px;
  border-radius: 5px;
}
.radar-tools button[aria-pressed="true"] {
  background: #263329;
  color: #d2e8b9;
}
.radar-tools input {
  flex: 1;
  max-width: 350px;
  min-width: 120px;
  border: 1px solid #2c3844;
  border-radius: 6px;
  background: #0f1821;
  color: #d7e3ef;
  padding: 9px 12px;
  font: inherit;
  font-size: 11px;
}
.radar-tools > span {
  margin-left: auto;
  font-size: 10px;
  color: #8b9daf;
}
.event-table {
  border: 1px solid #2d3845;
  border-radius: 10px;
  overflow: hidden;
}
.event-columns,
.event-table article {
  display: grid;
  grid-template-columns:
    minmax(250px, 2.5fr) minmax(100px, 0.6fr) minmax(125px, 0.8fr)
    minmax(150px, 1fr) 125px;
  gap: 18px;
  align-items: center;
  padding: 14px 18px;
}
.event-columns {
  font-size: 9px;
  text-transform: uppercase;
  color: #8195a8;
  letter-spacing: 0.04em;
  background: #151f29;
}
.event-table article {
  border-top: 1px solid #25303c;
  background: #0e171f;
  font-size: 11px;
}
.event-table article:hover,
.event-table article[data-selected="true"] {
  background: #18232e;
}
.event-title {
  text-align: left;
  background: none;
  border: 0;
  padding: 0;
  color: #d7e3ed;
  font-size: 12px;
  line-height: 1.5;
}
.event-title:hover {
  color: #cee5ad;
}
.event-table small {
  display: block;
  font-size: 9px;
  color: #859aad;
  margin-top: 5px;
}
.event-table small b {
  text-transform: capitalize;
  font-weight: 400;
  color: #abc0d3;
}
.event-probability strong {
  font: 500 17px var(--vp-font-family-mono);
  color: #e4eccf;
}
.event-probability > span {
  display: block;
  width: 75px;
  height: 3px;
  background: #2a3441;
  margin-top: 7px;
}
.event-probability i {
  display: block;
  background: #a3c87d;
  height: 100%;
}
.event-delta {
  font-size: 11px;
  color: #a7b9c9;
}
.event-delta[data-direction="up"] {
  color: #8dc4aa;
}
.quote-quality {
  color: #b3c3d0;
  font-size: 10px;
}
.event-state {
  display: inline-block;
  white-space: nowrap;
  text-transform: capitalize;
  border: 1px solid #344253;
  border-radius: 5px;
  padding: 5px 7px;
  font-size: 9px;
  color: #9eb1c4;
  background: #17222e;
}
.event-state[data-state="watching"] {
  color: #acd4b6;
  background: #18291f;
  border-color: #354a3b;
}
.event-state[data-state="triggered"] {
  color: #e9ecc1;
  background: #3c4324;
  border-color: #717c42;
}
.event-state[data-state="quality_blocked"],
.event-state[data-state="stale"] {
  color: #dfba86;
  border-color: #594637;
  background: #2c221b;
}
.event-detail {
  width: min(850px, calc(100vw - 40px));
  max-height: 85dvh;
  overflow: auto;
  color: #dce4ee;
  margin: auto;
  box-sizing: border-box;
  border: 1px solid #40516a;
  border-radius: 11px;
  background: #121d29;
  padding: 24px;
}
.event-detail::backdrop {
  background: #050b13bb;
  backdrop-filter: blur(4px);
}
.resolution-terms {
  margin-top: 14px;
  font-size: 11px;
  color: #9cb0c3;
}
.resolution-terms summary {
  cursor: pointer;
}
.resolution-terms p {
  white-space: pre-wrap;
  line-height: 1.7;
  max-height: 180px;
  overflow: auto;
}
.event-detail header {
  display: flex;
  justify-content: space-between;
  gap: 20px;
}
.event-detail header span {
  font-size: 9px;
  color: #8aa9c7;
}
.event-detail h3 {
  font-size: 17px;
  margin: 8px 0 18px;
  letter-spacing: -0.02em;
}
.event-detail header button {
  border: 0;
  background: none;
  font-size: 22px;
  align-self: flex-start;
}
.detail-body {
  display: grid;
  grid-template-columns: 1.6fr 1fr;
  gap: 24px;
}
.probability-chart {
  padding: 15px;
  background: #0b151f;
  border: 1px solid #2d3e4f;
  border-radius: 8px;
}
.probability-chart > div {
  display: flex;
  align-items: baseline;
  gap: 14px;
}
.probability-chart strong {
  font-size: 23px;
  font-weight: 500;
}
.probability-chart span,
.probability-chart p {
  font-size: 10px;
  color: #96aabd;
}
.probability-chart svg {
  display: block;
  width: 100%;
  height: 145px;
  margin-top: 10px;
  overflow: visible;
}
.probability-chart polyline {
  fill: none;
  stroke: #b2d289;
  stroke-width: 2;
  vector-effect: non-scaling-stroke;
}
.midline {
  stroke: #334352;
  stroke-dasharray: 4 4;
  stroke-width: 1;
}
.probability-chart footer {
  display: flex;
  justify-content: space-between;
  margin-top: 8px;
}
.detail-body dl {
  margin: 0;
  display: grid;
  align-content: start;
}
.detail-body dl > div {
  display: flex;
  justify-content: space-between;
  gap: 16px;
  border-bottom: 1px solid #293847;
  padding: 10px 0;
  font-size: 11px;
}
.detail-body dt {
  color: #8fa7bc;
}
.detail-body dd {
  margin: 0;
  max-width: 65%;
  overflow-wrap: anywhere;
  text-align: right;
}
.event-radar a {
  color: #b7d7f7;
  text-decoration: none;
}
.detail-note {
  font-size: 10px;
  line-height: 1.6;
  color: #8fa4b9;
  margin: 16px 0 0;
}
.signal-journal {
  border: 1px solid #2e3a46;
  border-radius: 10px;
  margin-top: 20px;
  background: #101a23;
}
.signal-journal > header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 14px 18px;
  border-bottom: 1px solid #2a3642;
}
.signal-journal h3 {
  font-size: 13px;
  font-weight: 500;
  margin: 0;
}
.signal-journal header > span {
  font: 9px var(--vp-font-family-mono);
  color: #7f98b0;
}
.journal-empty {
  display: flex;
  gap: 17px;
  padding: 25px 20px;
  align-items: center;
}
.journal-empty > span {
  display: grid;
  place-items: center;
  width: 34px;
  height: 34px;
  border-radius: 9px;
  border: 1px solid #354557;
  color: #acc998;
}
.journal-empty strong {
  font-size: 12px;
  font-weight: 500;
}
.journal-empty p {
  font-size: 11px;
  line-height: 1.6;
  color: #8ea2b5;
  margin: 6px 0 0;
}
.signal-journal article {
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 13px 18px;
  border-bottom: 1px solid #263440;
  font-size: 11px;
}
.signal-journal article > div {
  flex: 1;
}
.signal-journal article small {
  display: block;
  margin-top: 5px;
  color: #95aabd;
  font-size: 10px;
}
.signal-journal article > b {
  font-size: 9px;
  font-weight: 400;
  color: #a4bbcf;
}
.news-register {
  margin-top: 20px;
  border-top: 1px solid #2c3946;
}
.news-register summary {
  padding: 16px 0;
  font-size: 12px;
  cursor: pointer;
}
.news-register summary > span {
  color: #829aad;
  margin-left: 10px;
  font-size: 10px;
}
.news-register article {
  display: grid;
  grid-template-columns: 100px 1fr 240px;
  gap: 15px;
  align-items: center;
  padding: 12px 0;
  border-top: 1px solid #24323e;
  font-size: 11px;
}
.news-register article > span,
.news-register small {
  font-size: 9px;
  color: #859db2;
}
.radar-empty {
  padding: 35px;
  color: #97adbf;
  font-size: 12px;
}
.radar-error {
  background: #30251e;
  color: #e3ba98;
  border: 1px solid #584231;
  border-radius: 7px;
  padding: 12px;
  font-size: 12px;
}
.radar-error button {
  background: none;
  border: 0;
  text-decoration: underline;
}
:focus-visible {
  outline: 2px solid #accc85;
  outline-offset: 3px;
}
@media (max-width: 1100px) {
  .event-columns,
  .event-table article {
    grid-template-columns: minmax(200px, 2fr) 90px 120px 110px;
    gap: 12px;
  }
  .quote-quality,
  .event-columns > span:nth-child(4) {
    display: none;
  }
  .radar-intro {
    align-items: flex-start;
  }
  .detector {
    flex-direction: column;
    align-items: flex-end;
  }
  .rule-strip b {
    margin-left: 0;
  }
  .radar-metrics > div {
    padding: 13px;
  }
  .detail-body {
    grid-template-columns: 1fr;
  }
  .news-register article {
    grid-template-columns: 80px 1fr;
  }
  .news-register small {
    grid-column: 2;
  }
}
@media (max-width: 700px) {
  .radar-intro {
    flex-wrap: wrap;
  }
  .radar-intro h2 {
    font-size: 22px;
  }
  .detector {
    flex-direction: row;
    align-items: center;
  }
  .radar-metrics {
    grid-template-columns: 1fr 1fr;
  }
  .radar-tools {
    flex-wrap: wrap;
  }
  .radar-tools input {
    max-width: none;
    flex-basis: 100%;
    order: 3;
  }
  .radar-tools > span {
    display: none;
  }
  .event-columns {
    display: none;
  }
  .event-table article {
    grid-template-columns: 1fr 100px;
    gap: 12px;
  }
  .event-table article > div:first-child {
    grid-column: 1/-1;
  }
  .event-delta {
    grid-column: 1;
    grid-row: 3;
  }
  .event-table article > div:last-child {
    grid-column: 2;
    grid-row: 3;
  }
  .event-probability {
    display: flex;
    align-items: center;
    gap: 15px;
    grid-column: 1/-1;
  }
  .event-probability > span {
    width: 100%;
    max-width: 190px;
  }
  .event-state {
    white-space: normal;
  }
  .rule-strip {
    gap: 12px;
    font-size: 10px;
  }
  .rule-strip .rule-label {
    flex-basis: 100%;
  }
  .signal-journal header {
    gap: 8px;
    flex-wrap: wrap;
  }
  .signal-journal article {
    flex-wrap: wrap;
  }
  .signal-journal article > b {
    flex-basis: 100%;
  }
  .event-detail {
    padding: 14px;
  }
  .news-register summary > span {
    display: block;
    margin: 6px 0 0;
  }
}
</style>
