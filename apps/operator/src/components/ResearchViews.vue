<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from "vue";
import { fresh, loadResearchViews, type ResearchView, type ViewRegistry } from "../operations/research-views";
import { researchRenderers } from "./research/renderers";

const registry = ref<ViewRegistry | null>(null);
const error = ref(false);
const now = ref(Date.now());
let poll: ReturnType<typeof setTimeout> | undefined;
let clock: ReturnType<typeof setInterval> | undefined;
let request: AbortController | undefined;
let disposed = false;

function online(book: ResearchView): boolean {
  return !error.value && book.online && fresh(book.observed_ms, book.lease_ms, now.value);
}
async function refresh() {
  request = new AbortController();
  try {
    const data = await loadResearchViews(AbortSignal.any([request.signal, AbortSignal.timeout(5000)]));
    if (!disposed) { registry.value = data; error.value = false; }
  } catch { if (!disposed) error.value = true; }
  finally {
    if (!disposed) poll = setTimeout(() => void refresh(), registry.value?.refresh_ms ?? 5000);
  }
}
onMounted(() => {
  void refresh();
  clock = setInterval(() => { now.value = Date.now(); }, 1000);
});
onBeforeUnmount(() => {
  disposed = true;
  request?.abort();
  clearTimeout(poll);
  clearInterval(clock);
});
</script>

<template>
  <div v-if="error || registry?.rejected_count" class="registry-error" role="status">
    {{ error ? 'Research refresh interrupted. Showing saved observations; retrying automatically.' : 'Some research views could not be validated.' }}
  </div>
  <details v-for="book in registry?.books ?? []" :key="book.id" class="research-book"
    data-testid="research-view" :data-book-id="book.id">
    <summary>
      <span class="book-mark" aria-hidden="true">{{ book.mark || book.title.slice(0, 2) }}</span>
      <span class="book-title"><strong>{{ book.title }}</strong><small>{{ book.subtitle }}</small></span>
      <span class="book-capabilities"><span v-for="capability in book.capabilities" :key="capability">{{ capability }}</span></span>
      <span class="book-state" :class="{ online: online(book) }">{{ online(book) ? 'Capturing' : 'Capture offline' }}</span>
      <span class="book-open" aria-hidden="true">↗</span>
    </summary>
    <div class="book-sections">
      <section v-for="section in book.sections" :key="section.id" class="book-section"
        :class="{ wide: section.fields.length >= 4 }" :aria-label="section.title">
        <header><h2>{{ section.title }}</h2></header>
        <p v-if="section.description" class="section-description">{{ section.description }}</p>
        <component v-if="section.rows.length" :is="researchRenderers[section.kind]"
          :section="section" :online="online(book)" :now="now" />
        <p v-else class="section-empty">{{ section.empty }}</p>
      </section>
    </div>
  </details>
</template>

<style scoped>
.research-book { margin-bottom: 16px; border: 1px solid #34433c; border-radius: 12px; background: linear-gradient(110deg, #18281f, #101b22); color: #e9efea; overflow: hidden; }
summary { display: flex; align-items: center; gap: 16px; padding: 18px 22px; list-style: none; cursor: pointer; }
summary::-webkit-details-marker { display: none; }
summary:focus-visible { outline: 2px solid #c9f584; outline-offset: -4px; border-radius: 10px; }
.book-mark { display: grid; place-items: center; flex-shrink: 0; width: 40px; height: 40px; border: 1px solid #688354; border-radius: 10px; color: #d2f4a4; font-size: 12px; font-weight: 700; }
.book-title { display: grid; gap: 6px; min-width: 0; }
.book-title strong { font-size: 17px; letter-spacing: -.4px; overflow-wrap: anywhere; }
.book-title small { color: #b0bdb5; font-size: 11px; }
.book-capabilities { display: flex; flex-wrap: wrap; gap: 6px 16px; margin-left: auto; color: #aab8ad; font-size: 10px; }
.book-state { flex-shrink: 0; margin-left: auto; padding: 6px 10px; border: 1px solid #525b50; border-radius: 20px; color: #cbb98e; font-size: 9px; letter-spacing: .7px; text-transform: uppercase; }
.book-state.online { color: #d0eda8; border-color: #607a43; }
.book-open { color: #d4eea7; font-size: 18px; transition: transform .2s; }
[open] .book-open { transform: rotate(90deg); }
.book-sections { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 1px; border-top: 1px solid #34433c; background: #2a3932; }
.book-section { min-width: 0; background: #111c20; padding: 24px; }
.book-section.wide { grid-column: 1 / -1; }
header { margin-bottom: 18px; }
h2 { margin: 0; font-size: 13px; font-weight: 650; letter-spacing: -.1px; }
.section-description { margin: -7px 0 20px; color: #a9bbb0; font-size: 11px; line-height: 1.65; max-width: 90ch; }
.section-empty { color: #a4b5a9; font-size: 12px; line-height: 1.8; margin: 0; padding: 6px 0 12px; }
.registry-error { margin-bottom: 12px; padding: 12px 16px; border: 1px solid #594c30; border-radius: 8px; color: #d4bc92; font-size: 11px; }
@media (max-width: 1050px) { .book-capabilities { display: none; } }
@media (max-width: 760px) { .book-sections { grid-template-columns: minmax(0, 1fr); } .book-section { padding: 20px; } }
@media (max-width: 500px) { summary { padding: 14px 12px; gap: 10px; } .book-mark { width: 32px; height: 32px; font-size: 10px; } .book-title strong { font-size: 14px; } .book-title small { font-size: 10px; } .book-state { padding: 5px 7px; font-size: 8px; } .book-open { display: none; } }
@media (prefers-reduced-motion: reduce) { .book-open { transition: none; } }
</style>
