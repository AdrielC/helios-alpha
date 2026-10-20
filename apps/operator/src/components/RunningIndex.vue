<script setup lang="ts">
import { computed } from "vue";
import type { StrategyObservation } from "../operations/broker-holdings";
import type { TimeSeriesDescriptor } from "../operations/time-series-port";
import { readable, bookState, bookSeries, ageLabel } from "../operations/running-index";
const props = defineProps<{ strategy?: StrategyObservation; catalog: readonly TimeSeriesDescriptor[]; selected: string; now: number }>();
const emit = defineEmits<{ select: [id: string, series: string[]] }>();
const books = computed(() => props.strategy?.books ?? []);
const running = computed(() => books.value.filter(book => bookState(book, props.strategy, props.now) === 'running'));
</script>
<template>
  <section class="running-index" aria-label="Running index">
    <header><div><span class="index-kicker">Running index</span><h2>Strategies &amp; markets</h2></div><span>{{ running.length }} running · Golem</span></header>
    <div v-if="books.length" class="index-books">
      <article v-for="book in books" :key="book.id" :data-selected="selected === book.id">
        <button class="book-select" :aria-pressed="selected === book.id" :disabled="!bookSeries(book, catalog).length" @click="emit('select', book.id, bookSeries(book, catalog))"><i :data-running="bookState(book, strategy, now) === 'running'"></i><span><strong>{{ readable(book.engine) }}</strong><small>{{ book.id }}</small></span><b>{{ book.mode }}</b><em>{{ bookState(book, strategy, now) }}</em></button>
        <div class="book-markets"><button v-for="symbol in [...new Set([...book.signals.map(s => s.symbol), ...Object.keys(book.quote_age_ms ?? {})])]" :key="symbol" :disabled="!bookSeries(book, catalog, symbol).length" @click="emit('select', book.id, bookSeries(book, catalog, symbol))">{{ symbol }} <span>↗</span></button><small>{{ readable(book.reference_source ?? 'Unknown source') }} · {{ book.feeds?.length ?? 0 }} news feeds</small></div>
        <footer>{{ book.active_news }} active events <span>Heartbeat {{ ageLabel(book.updated_ms, now) }}</span></footer>
      </article>
    </div>
    <p v-else>{{ strategy ? 'No strategy books are registered.' : 'Waiting for Golem strategy status.' }}</p>
  </section>
</template>
<style scoped>
.running-index{margin:0 28px 16px;padding:18px 20px;border:1px solid #394737;border-radius:12px;background:linear-gradient(110deg,#19231e,#151d26 70%)}header{display:flex;align-items:center;justify-content:space-between;gap:12px;margin-bottom:14px}.index-kicker{font-size:9px;letter-spacing:.13em;text-transform:uppercase;color:#aec393}h2{font-size:16px;font-weight:550;margin:6px 0 0}header>span,footer,small,p{font-size:11px;color:var(--ui-muted)}.index-books{display:grid;grid-template-columns:repeat(auto-fit,minmax(min(100%,330px),1fr));gap:12px}article{border:1px solid #344136;border-radius:8px;background:#111a1e;overflow:hidden}article[data-selected=true]{border-color:#687d50}.book-select{border:0;background:transparent;padding:12px 14px;display:flex;align-items:center;gap:10px;text-align:left;width:100%;color:inherit;cursor:pointer}.book-select:disabled{cursor:default}.book-select>span{display:grid;gap:4px}.book-select strong{font-size:13px;font-weight:550}.book-select i{width:6px;height:6px;border-radius:50%;background:#cf9f66}.book-select i[data-running=true]{background:#91c8a5}.book-select b{margin-left:auto;text-transform:uppercase;font-size:9px;font-weight:500;border:1px solid #415148;padding:4px 7px;border-radius:4px;color:#bbc8bd}.book-select em{font:11px var(--vp-font-family-base);text-transform:capitalize;color:#aebeaa}.book-markets{display:flex;align-items:center;gap:8px;flex-wrap:wrap;padding:0 14px 12px}.book-markets button{padding:6px 10px;border:1px solid #35414f;background:#202b37;color:#d6e5f6;border-radius:5px;font-size:11px;cursor:pointer}.book-markets button:disabled{opacity:.5;cursor:default}.book-markets button span{margin-left:12px;color:#8fa3ba}.book-markets small{margin-left:auto}footer{padding:9px 14px;display:flex;justify-content:space-between;gap:10px;border-top:1px solid #293530;font-size:10px}p{margin:0;padding:8px 0}@media(max-width:720px){.running-index{margin:0 16px 14px;padding:14px}.book-markets small{margin-left:0;width:100%}}

.running-index{padding:14px 16px;margin:0 0 12px}.running-index>header{margin-bottom:10px}.running-index>header>div{display:flex;align-items:center;gap:14px}.running-index h2{font-size:13px;margin:0;color:#c2cdbf}.book-select{padding:10px 12px}.book-markets{padding:0 12px 10px}footer{padding:7px 12px}.book-markets small{font-size:10px}@media(min-width:1101px){.index-books article{display:grid;grid-template-columns:minmax(260px,.9fr) minmax(300px,1fr);align-items:center}.book-markets{padding:10px 14px}.index-books footer{grid-column:1/-1}.book-select{padding:12px 14px}}@media(max-width:720px){.running-index{margin:0 0 12px}.running-index>header>div{gap:8px;flex-wrap:wrap}.running-index h2{font-size:12px}.running-index>header>span{font-size:10px}}
</style>
