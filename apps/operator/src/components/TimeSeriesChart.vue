<script setup lang="ts">
import { computed, ref } from "vue";
import type { MetricSeriesView } from "../operations/operations-port";

const props = defineProps<{ series: MetricSeriesView }>();

const width = 760;
const height = 270;
const pad = { top: 18, right: 24, bottom: 36, left: 76 };
const hoveredIndex = ref<number>();

const values = computed(() => [
  ...props.series.points.map((point) => point.value),
  ...props.series.referenceLines.map((line) => line.value),
]);
const bounds = computed(() => {
  const min = Math.min(...values.value);
  const max = Math.max(...values.value);
  const span = Math.max(Math.abs(max - min), Math.abs(max) * 0.05, 1);
  return { min: min - span * 0.12, max: max + span * 0.12 };
});
const plotWidth = width - pad.left - pad.right;
const plotHeight = height - pad.top - pad.bottom;
const x = (index: number): number =>
  pad.left + (index / Math.max(1, props.series.points.length - 1)) * plotWidth;
const y = (value: number): number =>
  pad.top + ((bounds.value.max - value) / (bounds.value.max - bounds.value.min)) * plotHeight;
const linePath = computed(() =>
  props.series.points
    .map((point, index) => `${index === 0 ? "M" : "L"}${x(index).toFixed(2)} ${y(point.value).toFixed(2)}`)
    .join(" "),
);
const areaPath = computed(() => {
  if (props.series.points.length === 0) return "";
  const bottom = pad.top + plotHeight;
  return `${linePath.value} L${x(props.series.points.length - 1)} ${bottom} L${x(0)} ${bottom} Z`;
});
const yTicks = computed(() =>
  Array.from({ length: 5 }, (_, index) => bounds.value.max - ((bounds.value.max - bounds.value.min) * index) / 4),
);
const xTicks = computed(() => {
  if (props.series.points.length === 0) return [];
  const indices = [0, Math.floor((props.series.points.length - 1) / 2), props.series.points.length - 1];
  return [...new Set(indices)].map((index) => ({ index, point: props.series.points[index] }));
});
const activeIndex = computed(() => hoveredIndex.value ?? Math.max(0, props.series.points.length - 1));
const activePoint = computed(() => props.series.points[activeIndex.value]);
const firstPoint = computed(() => props.series.points[0]);
const change = computed(() => (activePoint.value?.value ?? 0) - (firstPoint.value?.value ?? 0));

function formatValue(value: number, compact = false): string {
  if (props.series.unit === "USD") {
    return new Intl.NumberFormat("en-US", {
      style: "currency",
      currency: "USD",
      notation: compact ? "compact" : "standard",
      maximumFractionDigits: compact ? 1 : 0,
    }).format(value);
  }
  if (props.series.unit === "%") return `${value.toFixed(1)}%`;
  if (props.series.unit === "ms") return `${new Intl.NumberFormat("en-US", { maximumFractionDigits: 0 }).format(value)}ms`;
  return new Intl.NumberFormat("en-US", { notation: compact ? "compact" : "standard", maximumFractionDigits: 1 }).format(value);
}

function formatTime(timestamp: string): string {
  return new Intl.DateTimeFormat("en-US", { hour: "numeric", minute: "2-digit", second: "2-digit" }).format(new Date(timestamp));
}

function handlePointer(event: PointerEvent): void {
  const target = event.currentTarget as SVGElement;
  const rect = target.getBoundingClientRect();
  const localX = ((event.clientX - rect.left) / rect.width) * width;
  const ratio = Math.min(1, Math.max(0, (localX - pad.left) / plotWidth));
  hoveredIndex.value = Math.round(ratio * Math.max(0, props.series.points.length - 1));
}

function handleKeyboard(event: KeyboardEvent): void {
  if (!props.series.points.length || !["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
  event.preventDefault();
  if (event.key === "Home") hoveredIndex.value = 0;
  else if (event.key === "End") hoveredIndex.value = props.series.points.length - 1;
  else {
    const step = event.key === "ArrowLeft" ? -1 : 1;
    hoveredIndex.value = Math.min(
      props.series.points.length - 1,
      Math.max(0, (hoveredIndex.value ?? props.series.points.length - 1) + step),
    );
  }
}
</script>

<template>
  <div class="time-series" :data-tone="series.tone">
    <div class="chart-value">
      <strong>{{ activePoint ? formatValue(activePoint.value) : "No data" }}</strong>
      <span v-if="activePoint">{{ formatTime(activePoint.timestamp) }}</span>
      <b v-if="activePoint" :class="{ negative: change < 0 }">{{ change >= 0 ? "+" : "" }}{{ formatValue(change, true) }}</b>
    </div>
    <svg
      :viewBox="`0 0 ${width} ${height}`"
      role="img"
      tabindex="0"
      :aria-label="`${series.label} time series. Current value ${activePoint ? formatValue(activePoint.value) : 'unavailable'}. Use left and right arrows to inspect points.`"
      preserveAspectRatio="none"
      @pointermove="handlePointer"
      @pointerleave="hoveredIndex = undefined"
      @keydown="handleKeyboard"
    >
      <g class="grid">
        <g v-for="tick in yTicks" :key="tick">
          <line :x1="pad.left" :x2="width - pad.right" :y1="y(tick)" :y2="y(tick)" />
          <text :x="pad.left - 12" :y="y(tick) + 3" text-anchor="end">{{ formatValue(tick, true) }}</text>
        </g>
        <g v-for="tick in xTicks" :key="tick.index">
          <line :x1="x(tick.index)" :x2="x(tick.index)" :y1="pad.top" :y2="pad.top + plotHeight" />
          <text :x="x(tick.index)" :y="height - 10" :text-anchor="tick.index === 0 ? 'start' : tick.index === series.points.length - 1 ? 'end' : 'middle'">{{ formatTime(tick.point.timestamp) }}</text>
        </g>
      </g>
      <g v-for="line in series.referenceLines" :key="line.label" class="reference" :data-tone="line.tone">
        <line :x1="pad.left" :x2="width - pad.right" :y1="y(line.value)" :y2="y(line.value)" />
        <text :x="width - pad.right - 4" :y="y(line.value) - 6" text-anchor="end">{{ line.label }}</text>
      </g>
      <path class="area" :d="areaPath" />
      <path class="series-line" :d="linePath" pathLength="1" vector-effect="non-scaling-stroke" />
      <g v-if="activePoint" class="cursor">
        <line :x1="x(activeIndex)" :x2="x(activeIndex)" :y1="pad.top" :y2="pad.top + plotHeight" />
        <circle :cx="x(activeIndex)" :cy="y(activePoint.value)" r="4.5" vector-effect="non-scaling-stroke" />
      </g>
    </svg>
  </div>
</template>

<style scoped>
.time-series { position: relative; min-width: 0; }
.chart-value { position: absolute; z-index: 2; top: 2px; left: 76px; display: flex; gap: 10px; align-items: baseline; pointer-events: none; }
.chart-value strong { color: var(--atlas-ink); font: 650 20px/1 var(--vp-font-family-mono); font-variant-numeric: tabular-nums; letter-spacing: -.02em; }
.chart-value span,
.chart-value b { color: var(--atlas-axis); font: 9px/1 var(--vp-font-family-mono); font-variant-numeric: tabular-nums; text-transform: uppercase; }
.chart-value b { color: var(--atlas-green-ink); }
.chart-value b.negative { color: var(--atlas-oxide); }
svg { display: block; width: 100%; height: 290px; overflow: visible; touch-action: none; }
svg:focus-visible { outline: 1px solid var(--atlas-blue); outline-offset: -1px; }
.grid line { stroke: var(--atlas-rule-soft); stroke-width: 1; vector-effect: non-scaling-stroke; }
.grid text,
.reference text { fill: var(--atlas-axis); font: 8px var(--vp-font-family-mono); font-variant-numeric: tabular-nums; }
.area { fill: color-mix(in srgb, var(--chart-color) 8%, transparent); }
.series-line { fill: none; stroke: var(--chart-color); stroke-width: 1.6; stroke-linecap: round; stroke-linejoin: round; animation: chart-draw 800ms cubic-bezier(.16,1,.3,1) both; }
.time-series[data-tone="cyan"] { --chart-color: var(--atlas-blue); }
.time-series[data-tone="green"] { --chart-color: var(--atlas-green); }
.time-series[data-tone="coral"] { --chart-color: var(--atlas-oxide); }
.reference line { stroke: var(--atlas-axis); stroke-width: 1; stroke-dasharray: 4 5; vector-effect: non-scaling-stroke; }
.reference[data-tone="warning"] line { stroke: var(--atlas-oxide); }
.reference[data-tone="critical"] line { stroke: var(--atlas-oxide); }
.cursor line { stroke: color-mix(in srgb, var(--chart-color) 45%, transparent); stroke-width: 1; vector-effect: non-scaling-stroke; }
.cursor circle { fill: var(--operator-black, #05090d); stroke: var(--chart-color); stroke-width: 1.5; }
@keyframes chart-draw { from { stroke-dasharray: 0 1; } to { stroke-dasharray: 1 0; } }
@media (max-width: 640px) {
  .chart-value { left: 56px; }
  .chart-value strong { font-size: 16px; }
  .chart-value span { display: none; }
  svg { width: 720px; height: 265px; }
  .time-series { overflow-x: auto; scrollbar-color: var(--atlas-blue) var(--atlas-blue-soft); }
}
@media (prefers-reduced-motion: reduce) { .series-line { animation: none; } }
</style>
