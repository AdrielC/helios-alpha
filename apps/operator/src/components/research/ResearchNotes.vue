<script setup lang="ts">
import { formatCell, formatTime, fresh, type RendererProps } from "../../operations/research-views";
defineProps<RendererProps>();
</script>

<template>
  <div class="notes">
    <article v-for="row in section.rows" :key="row.id">
      <dl><div v-for="field in section.fields" :key="field.key">
        <dt>{{ field.label }}</dt>
        <dd>{{ formatCell(row.values[field.key], field) }}</dd>
      </div></dl>
      <small v-if="section.max_age_ms !== null">
        {{ online && fresh(row.observed_ms, section.max_age_ms, now) ? 'Current' : 'Dated observation' }}
        <time v-if="row.observed_ms !== null"> · {{ formatTime(row.observed_ms) }}</time>
      </small>
    </article>
  </div>
</template>

<style scoped>
.notes { display: grid; gap: 16px; }
article + article { border-top: 1px solid #283632; padding-top: 16px; }
dl { display: grid; gap: 8px; margin: 0; }
dt { color: #a9bdaa; font-size: 9px; letter-spacing: .5px; text-transform: uppercase; margin-bottom: 4px; }
dd { margin: 0; font-size: 11px; line-height: 1.65; color: #c9d4ce; overflow-wrap: anywhere; white-space: pre-line; }
small { color: #c1b18f; font-size: 10px; }
</style>
