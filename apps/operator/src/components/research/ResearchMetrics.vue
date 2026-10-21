<script setup lang="ts">
import { formatCell, formatTime, fresh, type RendererProps } from "../../operations/research-views";
defineProps<RendererProps>();
</script>

<template>
  <div class="metrics">
    <template v-for="row in section.rows" :key="row.id">
      <article v-for="field in section.fields" :key="field.key">
        <small>{{ field.label }}</small>
        <strong>{{ formatCell(row.values[field.key], field) }}</strong>
        <span v-if="section.max_age_ms !== null" class="age">
          {{ online && fresh(row.observed_ms, section.max_age_ms, now) ? 'Current' : 'Dated observation' }}
          <time v-if="row.observed_ms !== null"> · {{ formatTime(row.observed_ms) }}</time>
        </span>
      </article>
    </template>
  </div>
</template>

<style scoped>
.metrics { display: grid; grid-template-columns: repeat(auto-fit, minmax(140px, 1fr)); gap: 24px; }
article { display: grid; gap: 12px; align-content: start; min-width: 0; }
small { color: #a9b7b2; font-size: 10px; letter-spacing: .8px; text-transform: uppercase; }
strong { color: #edf3e8; font-size: clamp(18px, 2vw, 24px); letter-spacing: -.5px; font-variant-numeric: tabular-nums; overflow-wrap: anywhere; }
.age { color: #b8b1a1; font-size: 10px; }
</style>
