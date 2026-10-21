<script setup lang="ts">
import { formatCell, formatTime, fresh, type RendererProps } from "../../operations/research-views";
defineProps<RendererProps>();
</script>

<template>
  <div class="table-scroll" tabindex="0" role="region" :aria-label="section.title">
    <table>
      <thead><tr>
        <th v-for="field in section.fields" :key="field.key" scope="col">{{ field.label }}</th>
        <th v-if="section.max_age_ms !== null" scope="col">Observed</th>
      </tr></thead>
      <tbody><tr v-for="row in section.rows" :key="row.id">
        <td v-for="field in section.fields" :key="field.key" :class="{ numeric: field.format !== 'text' }">
          {{ formatCell(row.values[field.key], field) }}
        </td>
        <td v-if="section.max_age_ms !== null" class="observed">
          <time v-if="row.observed_ms !== null" :datetime="new Date(row.observed_ms).toISOString()">{{ formatTime(row.observed_ms) }}</time>
          <span v-else>Unknown time</span>
          <small :class="{ current: online && fresh(row.observed_ms, section.max_age_ms, now) }">
            {{ online && fresh(row.observed_ms, section.max_age_ms, now) ? 'Current' : 'Dated observation' }}
          </small>
        </td>
      </tr></tbody>
    </table>
  </div>
</template>

<style scoped>
.table-scroll { overflow-x: auto; max-width: 100%; border-radius: 4px; }
.table-scroll:focus-visible { outline: 2px solid #c9f584; outline-offset: 3px; }
table { border-collapse: collapse; width: 100%; text-align: left; font-variant-numeric: tabular-nums; }
th { color: #a5b3ae; font-size: 9px; letter-spacing: .6px; text-transform: uppercase; font-weight: 500; padding: 0 18px 12px 0; white-space: nowrap; }
td { padding: 14px 18px 14px 0; border-top: 1px solid #283632; font-size: 11px; white-space: nowrap; }
td:first-child { color: #dcedc9; font-weight: 600; }
.numeric { color: #e3eae5; }
.observed time, .observed span { display: block; font-size: 10px; color: #acbbb3; }
small { display: block; margin-top: 5px; font-size: 9px; color: #d2b98c; }
small.current { color: #c5e69c; }
</style>
