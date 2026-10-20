<script setup lang="ts">
import { computed } from "vue";
const props = defineProps<{ name: string }>();
const paths: Record<string, string> = {
  overview: "M3 17l5-5 4 3 8-10M14 5h6v6M3 4v16h18",
  positions: "M12 3 2 8l10 5 10-5-10-5ZM3 12l9 5 9-5M3 16l9 5 9-5",
  orders: "M7 3h10l3 3v15H4V3h3ZM8 8h8M8 12h8M8 16h5",
  forecasts: "M3 12h4l3-8 4 16 3-8h4",
  activity: "M4 5h16M4 12h10M4 19h13M19 10v4M17 12h4",
  sources: "M12 3a4 4 0 0 1 4 4v1h2a4 4 0 0 1 0 8h-2M12 21a4 4 0 0 1-4-4v-1H6a4 4 0 0 1 0-8h2M12 8v8M8 12h8",
  control: "M12 3 4 6v6c0 5 8 9 8 9s8-4 8-9V6l-8-3ZM8 12l3 3 5-6",
  explorer: "M3 3h7v7H3V3ZM14 3h7v7h-7V3ZM3 14h7v7H3v-7ZM14 14h7v7h-7v-7Z",
  refresh: "M20 7V3l-3 3a8 8 0 1 0 3 9M20 7h-5",
  arrow: "M6 18 18 6M6 6h12v12",
  search: "M16 16l5 5M10 18a8 8 0 1 0 0-16 8 8 0 0 0 0 16Z",
};
const path = computed(() => paths[props.name] ?? paths.overview);
</script>
<template><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false"><path :d="path" /></svg></template>
<style scoped>svg{width:20px;height:20px;flex-shrink:0}</style>
