<script setup lang="ts">
import {
  computed,
  nextTick,
  onBeforeUnmount,
  onMounted,
  ref,
  watch,
} from "vue";
import OperatorIcon from "./OperatorIcon.vue";
import { eventUrl } from "../operations/event-radar-port";
import {
  loadResearchRegistry,
  type ResearchRegistry,
  type RegisteredEvent,
  type RegisteredForecast,
} from "../operations/research-registry-port";
const props = defineProps<{ mode: "events" | "forecasts"; focusId?: string }>();
const emit = defineEmits<{
  openRegistry: [mode: "events" | "forecasts", id: string];
  definitions: [];
}>();
const data = ref<ResearchRegistry | null>(null),
  now = ref(Date.now()),
  error = ref(""),
  loading = ref(true);
const query = ref(""),
  provider = ref("all"),
  lifecycle = ref("all"),
  selectedId = ref("");
const dialog = ref<HTMLDialogElement>();
let timer: ReturnType<typeof setInterval> | undefined,
  busy = false,
  disposed = false,
  ticks = 0;
const isEvents = computed(() => props.mode === "events");
const online = computed(
  () =>
    !!data.value?.online &&
    now.value >= data.value.observed_ms &&
    now.value - data.value.observed_ms <= 15000 &&
    !error.value,
);
const events = computed(() => data.value?.events ?? []);
const forecasts = computed(() => data.value?.forecasts ?? []);
function state(item: RegisteredForecast): string {
  if (["archived", "awaiting_outcome", "invalidated"].includes(item.state))
    return item.state;
  if (!online.value) return "offline";
  if (now.value >= item.closes_ms) return "awaiting_outcome";
  return item.state;
}
const current = computed(
  () => forecasts.value.filter((f) => state(f) === "current").length,
);
const needsAttention = computed(
  () => forecasts.value.filter((f) => state(f) !== "current").length,
);
const filteredEvents = computed(() =>
  events.value
    .filter(
      (e) =>
        matches(
          e.provider,
          `${e.title} ${e.symbols.join(" ")} ${e.contracts.map((c) => c.title).join(" ")}`,
        ) &&
        (lifecycle.value === "all" || e.state === lifecycle.value),
    )
    .sort(
      (a, b) =>
        Number(a.state !== "watching") - Number(b.state !== "watching") ||
        a.closes_ms - b.closes_ms,
    ),
);
const filteredForecasts = computed(() =>
  forecasts.value
    .filter(
      (f) =>
        matches(f.provider, `${f.title} ${f.symbols.join(" ")} ${f.topic}`) &&
        (lifecycle.value === "all" || state(f) === lifecycle.value),
    )
    .sort(
      (a, b) =>
        Number(state(b) === "current") - Number(state(a) === "current") ||
        a.closes_ms - b.closes_ms ||
        b.issued_ms - a.issued_ms,
    ),
);
const selectedEvent = computed(() =>
  isEvents.value
    ? events.value.find((e) => e.id === selectedId.value)
    : undefined,
);
const selectedForecast = computed(() =>
  !isEvents.value
    ? forecasts.value.find((f) => f.id === selectedId.value)
    : undefined,
);
const selected = computed(() => selectedEvent.value ?? selectedForecast.value);
const parentEvent = computed(() =>
  events.value.find((e) => e.id === selectedForecast.value?.event_key),
);
const nextEvent = computed(
  () =>
    [...events.value]
      .filter((e) => e.state === "watching" && e.closes_ms > now.value)
      .sort((a, b) => a.closes_ms - b.closes_ms)[0],
);
const readable = (v: string) => v.replaceAll("_", " ");
const percent = (n: number | null | undefined) =>
  n == null ? "—" : `${(100 * n).toFixed(1)}%`;
const date = (n: number) =>
  new Date(n).toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  });
const exact = (n: number) =>
  new Date(n).toLocaleString(undefined, { timeZoneName: "short" });
const age = (n: number | null) => {
  if (n == null) return "No quote";
  const s = Math.max(0, Math.floor((now.value - n) / 1000));
  return s < 60
    ? `${s}s ago`
    : s < 3600
      ? `${Math.floor(s / 60)}m ago`
      : s < 86400
        ? `${Math.floor(s / 3600)}h ago`
        : `${Math.floor(s / 86400)}d ago`;
};
const until = (n: number) => {
  const m = Math.ceil((n - now.value) / 60000);
  return m <= 0
    ? "Deadline passed"
    : m < 60
      ? `In ${m}m`
      : m < 1440
        ? `In ${Math.floor(m / 60)}h ${m % 60}m`
        : `In ${Math.ceil(m / 1440)}d`;
};
const money = (n: number) =>
  new Intl.NumberFormat("en-US", {
    style: "currency",
    currency: "USD",
    maximumFractionDigits: 0,
  }).format(n);
function matches(source: string, title: string): boolean {
  return (
    (provider.value === "all" || source === provider.value) &&
    title.toLowerCase().includes(query.value.toLowerCase().trim())
  );
}
function coverage(e: RegisteredEvent): number {
  return e.contracts.filter((c) =>
    forecasts.value.some(
      (f) => f.id === c.forecast_id && state(f) === "current",
    ),
  ).length;
}
function change(f: RegisteredForecast): string {
  const prior = f.history.find(
    (h) => h.version < f.version && h.definition_id === f.definition_id,
  );
  if (!prior) return "First version";
  const pp = (f.probability - prior.probability) * 100;
  return `${pp > 0 ? "+" : ""}${pp.toFixed(1)} pp`;
}
function historyPoints(f: RegisteredForecast): string {
  const points = [...f.history]
    .filter((v) => v.definition_id === f.definition_id)
    .reverse();
  const start = points[0]?.issued_ms ?? 0,
    span = Math.max(1, f.issued_ms - start);
  return points
    .map(
      (p) =>
        `${20 + (560 * (p.issued_ms - start)) / span},${112 - 92 * p.probability}`,
    )
    .join(" ");
}
function close(): void {
  dialog.value?.close();
  selectedId.value = "";
}
function openRelated(mode: "events" | "forecasts", id: string): void {
  close();
  emit("openRegistry", mode, id);
}
async function inspect(id: string): Promise<void> {
  selectedId.value = id;
  await nextTick();
  if (selected.value && !dialog.value?.open) dialog.value?.showModal();
}
async function refresh(): Promise<void> {
  if (busy || disposed) return;
  busy = true;
  try {
    const next = await loadResearchRegistry();
    if (disposed) return;
    data.value = next;
    error.value = "";
    if (props.focusId && !selectedId.value && loading.value)
      await inspect(props.focusId);
  } catch {
    if (!disposed)
      error.value =
        "Registry refresh interrupted. Last recorded evidence is retained.";
  } finally {
    busy = false;
    loading.value = false;
  }
}
watch(
  () => props.focusId,
  (id) => {
    if (id) void inspect(id);
  },
);
onMounted(() => {
  void refresh();
  timer = setInterval(() => {
    now.value = Date.now();
    if (++ticks % 5 === 0) void refresh();
  }, 1000);
});
onBeforeUnmount(() => {
  disposed = true;
  if (timer) clearInterval(timer);
  dialog.value?.close();
});
</script>

<template>
  <section
    class="registry"
    :aria-label="isEvents ? 'Event registry' : 'Forecast registry'"
  >
    <header class="registry-heading">
      <div>
        <span class="eyebrow"
          >Research /
          {{ isEvents ? "Event registry" : "Forecast registry" }}</span
        >
        <h1>
          {{ isEvents ? "Events" : "Forecasts"
          }}<span class="live" :data-online="online"
            ><i />{{
              online ? "Updating" : loading ? "Connecting" : "Offline"
            }}</span
          >
        </h1>
        <p>
          {{
            isEvents
              ? "The questions moving markets. Every contract, source and deadline."
              : "Market expectations, captured in time. Every forecast keeps its evidence."
          }}
        </p>
      </div>
      <div class="heading-actions">
        <button v-if="!isEvents" @click="emit('definitions')">
          Research definitions <OperatorIcon name="arrow" /></button
        ><button
          class="icon-button"
          aria-label="Refresh registry"
          :disabled="loading"
          @click="refresh"
        >
          <OperatorIcon name="refresh" />
        </button>
      </div>
    </header>
    <div v-if="error || (!loading && !online)" class="notice" role="status">
      {{ error || "Collector offline. Showing the last saved registry." }}
      <span v-if="data">Updated {{ age(data.observed_ms) }}</span>
    </div>
    <div class="registry-metrics">
      <article>
        <span>{{ isEvents ? "Registered events" : "Issued forecasts" }}</span
        ><strong>{{
          data ? (isEvents ? data.total_events : data.total_forecasts) : "—"
        }}</strong
        ><small>{{
          isEvents
            ? `${data?.total_contracts ?? "—"} tracked contracts`
            : `${data?.retained_versions ?? "—"} retained versions`
        }}</small>
      </article>
      <article>
        <span>{{ isEvents ? "Forecast coverage" : "Current forecasts" }}</span
        ><strong class="lime"
          >{{ data ? current : "—"
          }}<em v-if="isEvents">
            / {{ events.reduce((n, e) => n + e.contracts.length, 0) }}</em
          ></strong
        ><small>{{
          isEvents
            ? "Fresh, quality-checked quotes"
            : "Fresh evidence · quality checks passed"
        }}</small>
      </article>
      <article>
        <span>{{ isEvents ? "Next deadline" : "Needs attention" }}</span
        ><strong :class="{ compact: isEvents }">{{
          isEvents
            ? nextEvent
              ? until(nextEvent.closes_ms)
              : "—"
            : data
              ? needsAttention
              : "—"
        }}</strong
        ><small>{{
          isEvents
            ? nextEvent
              ? date(nextEvent.closes_ms)
              : "No upcoming deadline"
            : "Stale, blocked or awaiting outcome"
        }}</small>
      </article>
      <article>
        <span>{{ isEvents ? "Source venues" : "Forecast method" }}</span
        ><strong class="compact">{{
          isEvents
            ? [...new Set(events.map((e) => e.provider))].length || "—"
            : "Market implied"
        }}</strong
        ><small>{{
          isEvents
            ? "Event identities remain source-specific"
            : "Yes bid / ask midpoint · not model confidence"
        }}</small>
      </article>
    </div>
    <div class="registry-toolbar">
      <label class="search"
        ><OperatorIcon name="search" /><input
          v-model="query"
          :placeholder="
            isEvents
              ? 'Search events, contracts or assets…'
              : 'Search forecasts or assets…'
          "
          :aria-label="isEvents ? 'Search events' : 'Search forecasts'"
      /></label>
      <label class="filter"
        ><span>Source</span
        ><select v-model="provider" aria-label="Registry source">
          <option value="all">All sources</option>
          <option
            v-for="s in [...new Set(events.map((e) => e.provider))]"
            :key="s"
            :value="s"
          >
            {{ readable(s) }}
          </option>
        </select></label
      >
      <label class="filter"
        ><span>Status</span
        ><select v-model="lifecycle" aria-label="Registry status">
          <option value="all">All statuses</option>
          <option
            v-for="s in isEvents
              ? ['watching', 'closed', 'archived']
              : [
                  'current',
                  'quality_blocked',
                  'stale',
                  'offline',
                  'awaiting_outcome',
                  'invalidated',
                  'archived',
                ]"
            :key="s"
            :value="s"
          >
            {{ readable(s) }}
          </option>
        </select></label
      >
      <span class="result-count"
        >{{ isEvents ? filteredEvents.length : filteredForecasts.length }}
        {{ isEvents ? "events" : "forecasts" }}</span
      >
    </div>
    <div v-if="isEvents" class="event-list">
      <div class="list-head event-grid" aria-hidden="true">
        <span>Event / source</span><span>Contracts</span
        ><span>Forecast coverage</span><span>Next deadline</span
        ><span>Status</span><span />
      </div>
      <button
        v-for="e in filteredEvents"
        :key="e.id"
        class="event-grid registry-row"
        @click="inspect(e.id)"
      >
        <span class="identity"
          ><span class="event-symbol"><OperatorIcon name="events" /></span
          ><span
            ><strong>{{ e.title }}</strong
            ><small
              ><b :data-provider="e.provider">{{ e.provider }}</b
              ><span>{{
                e.symbols.join(" · ") || e.topics.map(readable).join(" · ")
              }}</span></small
            ></span
          ></span
        >
        <span class="count"
          ><b>{{ e.contracts.length }}</b
          ><small>contracts</small></span
        >
        <span class="coverage"
          ><span
            ><b>{{ coverage(e) }}</b> / {{ e.contracts.length }} current</span
          ><i
            ><i
              :style="{
                width: `${(100 * coverage(e)) / e.contracts.length}%`,
              }" /></i
        ></span>
        <span class="deadline"
          ><strong>{{ date(e.closes_ms) }}</strong
          ><small>{{ until(e.closes_ms) }}</small></span
        >
        <span class="state" :data-state="e.state">{{ readable(e.state) }}</span
        ><span class="row-arrow">↗</span>
      </button>
    </div>
    <div v-else class="forecast-list">
      <div class="list-head forecast-grid" aria-hidden="true">
        <span>Forecast / source</span><span>Yes probability</span
        ><span>Revision</span><span>Issued</span><span>Status</span><span />
      </div>
      <button
        v-for="f in filteredForecasts"
        :key="f.id"
        class="forecast-grid registry-row"
        @click="inspect(f.id)"
      >
        <span class="identity"
          ><span class="forecast-symbol"><OperatorIcon name="forecasts" /></span
          ><span
            ><strong>{{ f.title }}</strong
            ><small
              ><b :data-provider="f.provider">{{ f.provider }}</b
              ><span
                >{{ f.symbols.join(" · ") }} · {{ date(f.closes_ms) }}</span
              ></small
            ></span
          ></span
        >
        <span class="probability"
          ><strong>{{ percent(f.probability) }}</strong
          ><i><i :style="{ width: `${100 * f.probability}%` }" /></i
        ></span>
        <span class="revision"
          ><strong>v{{ f.version }}</strong
          ><small>{{ change(f) }}</small></span
        >
        <span class="issued"
          ><strong :title="exact(f.issued_ms)">{{ age(f.issued_ms) }}</strong
          ><small>Seen {{ age(f.confirmed_ms) }}</small></span
        >
        <span class="state" :data-state="state(f)">{{
          readable(state(f))
        }}</span
        ><span class="row-arrow">↗</span>
      </button>
    </div>
    <div
      v-if="!(isEvents ? filteredEvents.length : filteredForecasts.length)"
      class="empty"
    >
      <OperatorIcon :name="mode" />
      <h2>
        {{
          loading
            ? "Loading registry"
            : query || provider !== "all" || lifecycle !== "all"
              ? "No matching records"
              : isEvents
                ? "Waiting for event discovery"
                : "Waiting for eligible forecasts"
        }}
      </h2>
      <p>
        {{
          isEvents
            ? "Events appear when a source is discovered by the collector."
            : "A fresh two-sided quote must pass spread and depth checks before a forecast is recorded."
        }}
      </p>
    </div>
    <footer class="registry-footer">
      <span>{{
        isEvents
          ? "Source events → contracts → forecasts"
          : `Versions retained ${data?.retention_days ?? "—"} days · latest version preserved`
      }}</span
      ><span v-if="data"
        >{{ isEvents ? events.length : forecasts.length }} of
        {{ isEvents ? data.total_events : data.total_forecasts }} records ·
        {{ age(data.observed_ms) }}</span
      >
    </footer>
    <Teleport to="body"
      ><dialog
        ref="dialog"
        class="registry-dialog"
        @click.self="close"
        @close="selectedId = ''"
      >
        <article v-if="selected" class="detail">
          <header>
            <div>
              <span class="eyebrow"
                >{{ selected.provider }} /
                {{
                  isEvents ? "Registered event" : "Market-implied forecast"
                }}</span
              >
              <h2>{{ selected.title }}</h2>
            </div>
            <button
              class="close"
              aria-label="Close registry detail"
              @click="close"
            >
              ×
            </button>
          </header>
          <template v-if="selectedEvent">
            <div class="detail-stats">
              <div>
                <span>Contracts</span
                ><strong>{{ selectedEvent.contracts.length }}</strong>
              </div>
              <div>
                <span>Current forecasts</span
                ><strong>{{ coverage(selectedEvent) }}</strong>
              </div>
              <div>
                <span>Registered</span
                ><strong>{{ date(selectedEvent.first_seen_ms) }}</strong>
              </div>
              <div>
                <span>Next deadline</span
                ><strong>{{ date(selectedEvent.closes_ms) }}</strong>
              </div>
            </div>
            <section class="detail-section">
              <h3>Contracts <span>Source-defined outcomes</span></h3>
              <div
                class="contract-row"
                v-for="c in selectedEvent.contracts"
                :key="c.id"
              >
                <div>
                  <strong>{{ c.title }}</strong
                  ><small
                    >{{ date(c.closes_ms) }} · {{ readable(c.state) }} · Quote
                    {{ age(c.observed_ms) }}</small
                  >
                </div>
                <b>{{ percent(c.probability) }}<small>Yes quote</small></b
                ><button
                  v-if="c.forecast_id"
                  @click="openRelated('forecasts', c.forecast_id)"
                >
                  View forecast ↗</button
                ><span v-else class="muted">No eligible forecast</span>
              </div>
            </section>
            <footer class="detail-footer">
              <span>Source event ID</span
              ><code>{{ selectedEvent.source_id }}</code
              ><span
                >Last discovered {{ exact(selectedEvent.last_seen_ms) }}</span
              >
            </footer>
          </template>
          <template v-if="selectedForecast">
            <div class="forecast-hero">
              <div>
                <span>Issued Yes probability</span
                ><strong>{{ percent(selectedForecast.probability) }}</strong
                ><small
                  >Version {{ selectedForecast.version }} ·
                  {{ change(selectedForecast) }}</small
                >
              </div>
              <div class="hero-context">
                <span class="state" :data-state="state(selectedForecast)">{{
                  readable(state(selectedForecast))
                }}</span
                ><span>Deadline {{ exact(selectedForecast.closes_ms) }}</span
                ><span>Outcome unverified · score pending</span>
              </div>
            </div>
            <section class="detail-section history">
              <h3>
                Forecast history
                <span
                  >{{ selectedForecast.history.length }} retained recent
                  versions · probability scale 0–100%</span
                >
              </h3>
              <svg
                v-if="selectedForecast.history.length > 1"
                viewBox="0 0 620 134"
                role="img"
                aria-label="Issued forecast probabilities over time"
              >
                <path d="M20 20H580M20 66H580M20 112H580" class="grid" />
                <polyline :points="historyPoints(selectedForecast)" />
                <text x="590" y="24">100</text>
                <text x="590" y="70">50</text>
                <text x="590" y="116">0</text>
              </svg>
              <p v-else class="muted">
                First recorded version. History begins with observed evidence.
              </p>
              <div class="history-table">
                <table>
                  <thead>
                    <tr>
                      <th>Version</th>
                      <th>Issued</th>
                      <th>Yes probability</th>
                      <th>Definition</th>
                    </tr>
                  </thead>
                  <tbody>
                    <tr v-for="v in selectedForecast.history" :key="v.id">
                      <td>v{{ v.version }}</td>
                      <td :title="exact(v.issued_ms)">
                        {{ date(v.issued_ms) }}
                      </td>
                      <td>{{ percent(v.probability) }}</td>
                      <td>
                        <code :title="v.definition_id">{{
                          v.definition_id.slice(0, 12)
                        }}</code>
                      </td>
                    </tr>
                  </tbody>
                </table>
              </div>
            </section>
            <section class="detail-section">
              <h3>
                Issue-time evidence <span>Immutable quote and policy</span>
              </h3>
              <dl class="evidence">
                <div>
                  <dt>Yes bid / ask</dt>
                  <dd>
                    {{ percent(selectedForecast.bid) }} /
                    {{ percent(selectedForecast.ask) }}
                  </dd>
                </div>
                <div>
                  <dt>Near-touch depth</dt>
                  <dd>{{ money(selectedForecast.depth_usd) }}</dd>
                </div>
                <div>
                  <dt>Source observed</dt>
                  <dd>{{ exact(selectedForecast.observed_ms) }}</dd>
                </div>
                <div>
                  <dt>Issued by Helios</dt>
                  <dd>{{ exact(selectedForecast.issued_ms) }}</dd>
                </div>
                <div>
                  <dt>
                    Quality policy · r{{ selectedForecast.rule_revision }}
                  </dt>
                  <dd>
                    Spread ≤ {{ selectedForecast.policy.max_spread_pp }} pp ·
                    depth ≥ {{ money(selectedForecast.policy.min_depth_usd) }}
                  </dd>
                </div>
                <div>
                  <dt>Latest source quote</dt>
                  <dd>
                    {{
                      percent(
                        online ? selectedForecast.current_probability : null,
                      )
                    }}
                    <small
                      >· {{ age(selectedForecast.current_observed_ms) }}</small
                    >
                  </dd>
                </div>
              </dl>
              <p class="method">
                The forecast is the source market’s Yes bid/ask midpoint. It is
                market expectation, not a Helios return forecast or an exit
                signal. A passed deadline does not verify the outcome.
              </p>
            </section>
            <details class="terms">
              <summary>Contract terms &amp; provenance</summary>
              <p>
                {{
                  selectedForecast.resolution ||
                  "No resolution text supplied by the source."
                }}
              </p>
              <dl>
                <dt>Method</dt>
                <dd>{{ selectedForecast.method }}</dd>
                <dt>Definition SHA-256</dt>
                <dd>
                  <code>{{ selectedForecast.definition_id }}</code>
                </dd>
                <dt>Version ID</dt>
                <dd>
                  <code>{{ selectedForecast.version_id }}</code>
                </dd>
                <dt>Observation sequence</dt>
                <dd>{{ selectedForecast.observation_seq }}</dd>
              </dl>
            </details>
            <footer class="detail-actions">
              <button
                @click="openRelated('events', selectedForecast.event_key)"
              >
                {{ parentEvent ? "View event" : "Event registry" }} ↗</button
              ><a
                v-if="eventUrl(selectedForecast.url)"
                :href="eventUrl(selectedForecast.url)"
                target="_blank"
                rel="noopener noreferrer"
                >Open source ↗</a
              >
            </footer>
          </template>
        </article>
      </dialog></Teleport
    >
  </section>
</template>

<style scoped>
.registry {
  min-height: 100%;
  color: var(--ui-text, #e6eaf0);
  background: var(--atlas-ground, #0c1118);
}
button,
input,
select {
  font: inherit;
}
button,
a,
input,
select,
summary {
  outline-offset: 4px;
}
button:focus-visible,
a:focus-visible,
input:focus-visible,
select:focus-visible,
summary:focus-visible {
  outline: 2px solid #c5f277;
}
.registry-heading {
  display: flex;
  justify-content: space-between;
  gap: 20px;
  align-items: center;
  padding: 30px 32px 24px;
}
.eyebrow {
  font: 10px var(--vp-font-family-mono);
  letter-spacing: 0.12em;
  text-transform: uppercase;
  color: #869496;
}
.registry-heading h1 {
  display: flex;
  align-items: center;
  gap: 18px;
  font-size: 32px;
  letter-spacing: -0.045em;
  line-height: 1.2;
  margin: 10px 0;
  font-weight: 600;
}
.registry-heading p {
  color: #98a3b1;
  font-size: 13px;
  margin: 0;
  line-height: 1.6;
}
.live {
  display: flex;
  align-items: center;
  gap: 7px;
  font-size: 11px;
  font-weight: 400;
  letter-spacing: 0;
  color: #b9a587;
  border: 1px solid #5b4a32;
  border-radius: 20px;
  padding: 5px 9px;
}
.live i {
  width: 5px;
  height: 5px;
  border-radius: 50%;
  background: currentColor;
}
.live[data-online="true"] {
  color: #c5f277;
  border-color: #3e5133;
  background: #b9f27608;
}
.heading-actions {
  display: flex;
  gap: 8px;
  align-items: center;
}
.heading-actions button,
.detail-actions button,
.detail-actions a,
.contract-row button {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  min-height: 36px;
  border: 1px solid #33404d;
  border-radius: 7px;
  background: #18212b;
  color: #c8d3dc;
  font-size: 12px;
  padding: 8px 12px;
  cursor: pointer;
  text-decoration: none;
}
.heading-actions svg {
  width: 14px;
  height: 14px;
}
.heading-actions .icon-button {
  width: 36px;
  padding: 8px;
}
.notice {
  display: flex;
  gap: 12px;
  justify-content: space-between;
  margin: 0 32px 16px;
  padding: 12px 16px;
  border: 1px solid #665136;
  border-radius: 8px;
  font-size: 12px;
  color: #dbc39d;
  background: #291e12;
}
.registry-metrics {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  margin: 0 32px 26px;
  border: 1px solid #29323c;
  border-radius: 12px;
  background: linear-gradient(110deg, #18211e, #141b23 47%);
  overflow: hidden;
}
.registry-metrics article {
  min-width: 0;
  padding: 19px 22px;
  border-right: 1px solid #29323c;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.registry-metrics article:last-child {
  border: 0;
}
.registry-metrics article > span {
  font-size: 11px;
  color: #a1adba;
}
.registry-metrics strong {
  font: 500 29px var(--vp-font-family-mono);
  letter-spacing: -0.05em;
  line-height: 1.3;
}
.registry-metrics strong.compact {
  font: 500 19px var(--vp-font-family-base);
  letter-spacing: -0.02em;
  min-height: 38px;
  display: flex;
  align-items: center;
}
.registry-metrics em {
  font-size: 16px;
  color: #7d8a98;
  font-style: normal;
}
.registry-metrics small {
  font-size: 10px;
  color: #84929f;
  line-height: 1.5;
}
.lime {
  color: #c5f277;
}
.registry-toolbar {
  display: flex;
  gap: 14px;
  align-items: center;
  padding: 0 32px 20px;
}
.search {
  display: flex;
  align-items: center;
  gap: 10px;
  flex: 1;
  min-width: 140px;
  max-width: 480px;
}
.search svg {
  width: 17px;
  height: 17px;
  color: #738394;
}
.search input {
  border: 0;
  background: transparent;
  color: #dae1e8;
  font-size: 12px;
  width: 100%;
  padding: 10px 2px;
  min-width: 0;
}
.search input::placeholder {
  color: #83909f;
}
.filter {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 11px;
  color: #7f8e9e;
}
.filter select {
  background: #141c25;
  border: 1px solid #2b3743;
  color: #bac7d2;
  padding: 7px 8px;
  border-radius: 6px;
  text-transform: capitalize;
  max-width: 170px;
}
.result-count {
  margin-left: auto;
  font: 10px var(--vp-font-family-mono);
  color: #8292a0;
  white-space: nowrap;
}
.event-grid,
.forecast-grid {
  display: grid;
  grid-template-columns: minmax(240px, 1fr) 82px 150px 170px 100px 20px;
  gap: 22px;
  align-items: center;
}
.forecast-grid {
  grid-template-columns: minmax(240px, 1fr) 120px 84px 120px 130px 20px;
}
.list-head {
  padding: 12px 32px;
  color: #7f8e9c;
  font: 9px var(--vp-font-family-mono);
  text-transform: uppercase;
  letter-spacing: 0.06em;
  background: #121923;
  border-top: 1px solid #26313c;
  border-bottom: 1px solid #26313c;
}
.registry-row {
  padding: 20px 32px;
  width: 100%;
  text-align: left;
  border: 0;
  border-bottom: 1px solid #232c36;
  background: transparent;
  color: #dce5eb;
  cursor: pointer;
  transition: background 0.15s;
}
.registry-row:hover {
  background: #182129;
}
.identity {
  display: flex;
  gap: 14px;
  align-items: center;
  min-width: 0;
}
.event-symbol,
.forecast-symbol {
  display: grid;
  place-items: center;
  width: 38px;
  height: 40px;
  border: 1px solid #344231;
  background: #1a261e;
  color: #c5df9e;
  border-radius: 9px;
  flex-shrink: 0;
}
.forecast-symbol {
  background: #1a2232;
  color: #a5bce7;
  border-color: #303c51;
}
.identity > span:last-child {
  display: grid;
  gap: 8px;
  min-width: 0;
}
.identity strong {
  font-size: 13px;
  font-weight: 500;
  line-height: 1.5;
  overflow-wrap: anywhere;
}
.identity small {
  display: flex;
  gap: 9px;
  flex-wrap: wrap;
  align-items: center;
  font-size: 10px;
  color: #8999a8;
}
.identity small b {
  font-weight: 400;
  text-transform: capitalize;
  padding: 2px 6px;
  border-radius: 4px;
  border: 1px solid #334456;
  color: #a4bdde;
  background: #21314b44;
}
.identity small b[data-provider="kalshi"] {
  color: #afd8bc;
  background: #243c2844;
  border-color: #365344;
}
.count {
  display: grid;
  gap: 5px;
}
.count b {
  font: 500 16px var(--vp-font-family-mono);
}
.count small {
  display: none;
}
.coverage {
  display: grid;
  gap: 9px;
  font-size: 11px;
  color: #819183;
}
.coverage b {
  color: #bed69c;
  font-weight: 400;
}
.coverage > i,
.probability > i {
  display: block;
  height: 3px;
  background: #293333;
  max-width: 115px;
  border-radius: 2px;
  overflow: hidden;
}
.coverage > i > i,
.probability > i > i {
  display: block;
  height: 100%;
  background: #bddf8b;
}
.probability {
  display: grid;
  gap: 10px;
}
.probability strong {
  font: 500 20px var(--vp-font-family-mono);
  letter-spacing: -0.04em;
  color: #d1e7b6;
}
.probability > i {
  max-width: 95px;
}
.deadline,
.revision,
.issued {
  display: grid;
  gap: 6px;
  font-size: 11px;
}
.deadline strong,
.revision strong,
.issued strong {
  font-weight: 400;
}
.deadline small,
.revision small,
.issued small {
  color: #7e90a0;
  font-size: 10px;
}
.state {
  justify-self: start;
  padding: 5px 8px;
  border-radius: 5px;
  border: 1px solid #39434f;
  color: #a8b5c2;
  font-size: 10px;
  text-transform: capitalize;
  white-space: nowrap;
  background: #1b252d;
}
.state[data-state="current"],
.state[data-state="watching"] {
  color: #bcd998;
  background: #25351a55;
  border-color: #3d4f32;
}
.state[data-state="quality_blocked"],
.state[data-state="stale"],
.state[data-state="invalidated"] {
  color: #d3b58a;
  background: #3a2c1955;
  border-color: #5f4a30;
}
.row-arrow {
  color: #657788;
  font-size: 17px;
}
.registry-row:hover .row-arrow {
  color: #c5f277;
}
.registry-footer {
  display: flex;
  justify-content: space-between;
  gap: 16px;
  padding: 18px 32px;
  color: #6e8090;
  font-size: 10px;
}
.empty {
  display: grid;
  justify-items: center;
  gap: 14px;
  padding: 55px 22px;
  color: #7f91a1;
  text-align: center;
}
.empty h2 {
  font-size: 17px;
  font-weight: 500;
  color: #bac7d0;
  margin: 0;
}
.empty p {
  max-width: 490px;
  font-size: 12px;
  line-height: 1.7;
  margin: 0;
}
.registry-dialog {
  width: min(880px, calc(100vw - 40px));
  max-height: calc(100dvh - 40px);
  padding: 0;
  border: 1px solid #3b4853;
  border-radius: 16px;
  background: #111820;
  color: #dce5ec;
  box-shadow: 0 35px 100px #000a;
}
.registry-dialog::backdrop {
  background: #03070bc9;
  backdrop-filter: blur(5px);
}
.detail {
  padding: 28px;
}
.detail > header {
  display: flex;
  align-items: flex-start;
  gap: 20px;
  justify-content: space-between;
}
.detail h2 {
  font-size: 24px;
  font-weight: 500;
  line-height: 1.35;
  letter-spacing: -0.035em;
  margin: 10px 0 22px;
  overflow-wrap: anywhere;
}
.close {
  border: 1px solid #303d49;
  background: #19222c;
  color: #9faeba;
  border-radius: 7px;
  font-size: 23px;
  width: 32px;
  height: 32px;
  flex-shrink: 0;
  cursor: pointer;
}
.detail-stats {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 18px;
  padding: 19px;
  background: #19232b;
  border-radius: 9px;
}
.detail-stats > div {
  display: grid;
  gap: 9px;
}
.detail-stats span {
  font-size: 10px;
  color: #90a2b1;
}
.detail-stats strong {
  font-size: 14px;
  font-weight: 500;
}
.detail-section {
  margin: 26px 0;
}
.detail-section h3 {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  align-items: baseline;
  justify-content: space-between;
  font-size: 13px;
  font-weight: 500;
  margin: 0 0 14px;
}
.detail-section h3 span {
  color: #8494a2;
  font-size: 10px;
  font-weight: 400;
}
.contract-row {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 80px 130px;
  align-items: center;
  gap: 14px;
  border-top: 1px solid #28333d;
  padding: 16px 0;
}
.contract-row > div {
  display: grid;
  gap: 8px;
}
.contract-row strong {
  font-size: 13px;
  font-weight: 400;
  line-height: 1.5;
}
.contract-row small {
  font-size: 10px;
  color: #8a9bab;
}
.contract-row > b {
  font: 500 17px var(--vp-font-family-mono);
}
.contract-row > b small {
  display: block;
  margin-top: 5px;
  font: 10px var(--vp-font-family-base);
}
.contract-row button {
  font-size: 11px;
  padding: 7px;
}
.detail-footer {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  color: #8698a5;
  font-size: 10px;
  border-top: 1px solid #28333d;
  padding-top: 16px;
}
.detail-footer code {
  overflow-wrap: anywhere;
}
.forecast-hero {
  display: flex;
  justify-content: space-between;
  gap: 20px;
  align-items: center;
  background: linear-gradient(110deg, #243123, #172225);
  border: 1px solid #3c4c34;
  border-radius: 10px;
  padding: 24px;
}
.forecast-hero > div:first-child {
  display: grid;
  gap: 8px;
}
.forecast-hero span,
.forecast-hero small {
  font-size: 11px;
  color: #9cac9d;
}
.forecast-hero strong {
  font: 500 42px var(--vp-font-family-mono);
  color: #c5f277;
  letter-spacing: -0.06em;
}
.hero-context {
  display: grid;
  justify-items: end;
  gap: 12px;
  text-align: right;
}
.history svg {
  width: 100%;
  max-height: 170px;
  margin-bottom: 15px;
}
.history polyline {
  fill: none;
  stroke: #c5f277;
  stroke-width: 2;
  stroke-linejoin: round;
}
.history .grid {
  fill: none;
  stroke: #2c3942;
  stroke-dasharray: 3 5;
}
.history text {
  fill: #768b9c;
  font: 9px var(--vp-font-family-mono);
}
.history-table {
  max-height: 210px;
  overflow: auto;
  border: 1px solid #2a3742;
  border-radius: 8px;
}
.history table {
  width: 100%;
  border-collapse: collapse;
  text-align: left;
  font-size: 11px;
  white-space: nowrap;
}
.history th {
  font-size: 10px;
  font-weight: 400;
  color: #8fa1b1;
  background: #1a242d;
  position: sticky;
  top: 0;
}
.history th,
.history td {
  padding: 10px 13px;
  border-bottom: 1px solid #28333c;
}
.history td:nth-child(3) {
  color: #c6dfa8;
  font-family: var(--vp-font-family-mono);
}
.history code {
  font-size: 10px;
  color: #8da0ad;
}
.evidence {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 22px;
  margin: 0;
  padding: 18px;
  background: #17212a;
  border-radius: 8px;
}
.evidence dt {
  font-size: 10px;
  color: #8ba0b1;
}
.evidence dd {
  font-size: 12px;
  line-height: 1.6;
  margin: 7px 0 0;
}
.evidence small {
  font-size: 10px;
  color: #93a5b4;
}
.method {
  font-size: 11px;
  line-height: 1.7;
  color: #8da0af;
  margin: 16px 0 0;
}
.terms {
  border-top: 1px solid #2b3741;
  padding: 16px 0;
  font-size: 12px;
}
.terms summary {
  cursor: pointer;
  color: #bdcbd5;
}
.terms p {
  white-space: pre-wrap;
  line-height: 1.7;
  color: #95a8b8;
  font-size: 11px;
}
.terms dl {
  display: grid;
  grid-template-columns: 130px minmax(0, 1fr);
  gap: 12px;
  font-size: 10px;
  color: #8ea4b4;
}
.terms dd {
  margin: 0;
  overflow-wrap: anywhere;
}
.detail-actions {
  display: flex;
  justify-content: space-between;
  gap: 14px;
  border-top: 1px solid #2b3741;
  padding-top: 18px;
}
.muted {
  font-size: 11px;
  color: #7d94a5;
  line-height: 1.6;
}
.detail-actions button {
  color: #c5e5aa;
  border-color: #46583a;
  background: #233221;
}
@media (max-width: 1250px) {
  .event-grid {
    grid-template-columns: minmax(240px, 1fr) 65px 120px 130px 90px 15px;
    gap: 14px;
  }
  .forecast-grid {
    grid-template-columns: minmax(210px, 1fr) 100px 72px 95px 110px 15px;
    gap: 12px;
  }
  .registry-heading,
  .registry-row,
  .list-head {
    padding-left: 24px;
    padding-right: 24px;
  }
  .registry-metrics {
    margin-left: 24px;
    margin-right: 24px;
  }
  .registry-toolbar {
    padding-left: 24px;
    padding-right: 24px;
  }
  .registry-metrics article {
    padding: 18px 16px;
  }
  .identity {
    gap: 10px;
  }
  .event-symbol,
  .forecast-symbol {
    display: none;
  }
  .state {
    font-size: 9px;
    padding: 5px 6px;
  }
}
@media (max-width: 950px) {
  .list-head {
    display: none;
  }
  .event-grid {
    grid-template-columns: minmax(0, 1fr) 125px 100px;
  }
  .event-grid .identity {
    grid-column: 1/3;
  }
  .event-grid .count {
    grid-column: 3;
    grid-row: 1;
    text-align: right;
  }
  .count small {
    display: block;
  }
  .event-grid .coverage {
    grid-column: 1;
    grid-row: 2;
  }
  .event-grid .deadline {
    grid-column: 2;
    grid-row: 2;
  }
  .event-grid .state {
    grid-column: 3;
    grid-row: 2;
    justify-self: end;
  }
  .row-arrow {
    display: none;
  }
  .forecast-grid {
    grid-template-columns: minmax(0, 1fr) 95px 110px;
  }
  .forecast-grid .identity {
    grid-column: 1/3;
  }
  .forecast-grid .probability {
    grid-column: 3;
    grid-row: 1;
    justify-self: end;
  }
  .forecast-grid .state {
    grid-row: 2;
    grid-column: 3;
    justify-self: end;
  }
  .registry-row {
    gap: 18px 14px;
  }
  .registry-metrics {
    grid-template-columns: 1fr 1fr;
  }
  .registry-metrics article:nth-child(2) {
    border-right: 0;
  }
  .registry-metrics article:nth-child(-n + 2) {
    border-bottom: 1px solid #29323c;
  }
  .registry-heading {
    align-items: flex-start;
  }
  .heading-actions button {
    font-size: 11px;
  }
  .registry-toolbar {
    flex-wrap: wrap;
    gap: 12px;
  }
  .search {
    flex-basis: 100%;
    max-width: none;
  }
  .result-count {
    font-size: 9px;
  }
  .registry-footer {
    flex-wrap: wrap;
  }
}
@media (max-width: 600px) {
  .registry-heading {
    padding: 22px 18px;
    gap: 12px;
    flex-wrap: wrap;
  }
  .registry-heading h1 {
    font-size: 28px;
  }
  .registry-heading p {
    font-size: 12px;
  }
  .registry-metrics {
    margin: 0 18px 20px;
  }
  .registry-metrics article {
    padding: 15px 13px;
  }
  .registry-metrics strong {
    font-size: 25px;
  }
  .registry-metrics strong.compact {
    font-size: 16px;
    min-height: 33px;
  }
  .registry-metrics small {
    font-size: 9px;
  }
  .registry-toolbar {
    padding: 0 18px 18px;
  }
  .filter > span {
    display: none;
  }
  .filter select {
    max-width: 150px;
  }
  .registry-row {
    padding: 18px;
    gap: 18px 10px;
  }
  .event-grid {
    grid-template-columns: minmax(0, 1fr) 115px 80px;
  }
  .forecast-grid {
    grid-template-columns: minmax(0, 1fr) 82px 100px;
  }
  .identity strong {
    font-size: 12px;
  }
  .identity small {
    font-size: 9px;
    gap: 6px;
  }
  .coverage {
    font-size: 10px;
  }
  .deadline strong {
    font-size: 10px;
  }
  .registry-footer {
    padding: 18px;
  }
  .registry-dialog {
    width: calc(100vw - 16px);
    max-height: calc(100dvh - 16px);
    border-radius: 12px;
  }
  .detail {
    padding: 19px;
  }
  .detail h2 {
    font-size: 20px;
  }
  .detail-stats {
    grid-template-columns: 1fr 1fr;
    padding: 16px;
  }
  .contract-row {
    grid-template-columns: minmax(0, 1fr) 90px;
  }
  .contract-row > div {
    grid-column: 1/3;
  }
  .contract-row button {
    grid-row: 2;
    grid-column: 1;
    justify-self: start;
  }
  .contract-row > b {
    grid-row: 2;
    grid-column: 2;
  }
  .forecast-hero {
    padding: 18px;
    align-items: flex-start;
    flex-direction: column;
  }
  .hero-context {
    justify-items: start;
    text-align: left;
  }
  .evidence {
    grid-template-columns: 1fr;
    gap: 16px;
  }
  .terms dl {
    grid-template-columns: 1fr;
    gap: 6px;
  }
  .terms dd {
    margin-bottom: 10px;
  }
  .detail-actions {
    flex-wrap: wrap;
  }
  .notice {
    margin: 0 18px 14px;
    flex-direction: column;
  }
  .history h3 span {
    font-size: 9px;
  }
}
@media (prefers-reduced-motion: reduce) {
  .registry-row {
    transition: none;
  }
}
</style>
