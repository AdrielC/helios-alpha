<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";

type ObservatoryPhase = {
  key: string;
  label: string;
  detail: string;
};

type FrequencyLane = {
  id: string;
  frequency: string;
  horizon: string;
  observations: number;
  priorMean: number;
  posteriorMean: number;
  low: number;
  high: number;
  draw: number | null;
  status: "eligible" | "rejected" | "selected";
  constraint: string;
  points: number[];
};

const phases: ObservatoryPhase[] = [
  {
    key: "observe",
    label: "Observe",
    detail: "Update sufficient statistics from closed, causally admitted outcomes.",
  },
  {
    key: "pool",
    label: "Pool",
    detail: "Share strength across related frequencies without treating them as independent tests.",
  },
  {
    key: "constrain",
    label: "Constrain",
    detail: "Reject infeasible candidates before they can enter the selection set.",
  },
  {
    key: "draw",
    label: "Draw",
    detail: "Regenerate one posterior utility sample for each eligible arm from an explicit replay key.",
  },
  {
    key: "select",
    label: "Select",
    detail: "Choose the largest eligible draw and emit a research candidate, not an order.",
  },
];

const lanes: FrequencyLane[] = [
  {
    id: "one-minute",
    frequency: "1 minute",
    horizon: "micro response",
    observations: 148,
    priorMean: 0.31,
    posteriorMean: 0.18,
    low: 0.04,
    high: 0.31,
    draw: null,
    status: "rejected",
    constraint: "turnover gate",
    points: [-0.04, 0.08, 0.14, 0.19, 0.22, 0.26, 0.34],
  },
  {
    id: "ten-minute",
    frequency: "10 minutes",
    horizon: "primary response",
    observations: 64,
    priorMean: 0.31,
    posteriorMean: 0.42,
    low: 0.08,
    high: 0.76,
    draw: 0.61,
    status: "selected",
    constraint: "highest eligible draw",
    points: [0.03, 0.16, 0.27, 0.39, 0.47, 0.63, 0.81],
  },
  {
    id: "one-hour",
    frequency: "1 hour",
    horizon: "decay response",
    observations: 27,
    priorMean: 0.31,
    posteriorMean: 0.29,
    low: -0.12,
    high: 0.69,
    draw: 0.37,
    status: "eligible",
    constraint: "all gates passed",
    points: [-0.16, -0.02, 0.12, 0.25, 0.38, 0.56, 0.74],
  },
];

const activePhase = ref(0);
const isPlaying = ref(true);
const isVisible = ref(true);
const pageHidden = ref(false);
const reducedMotion = ref(false);
const decisionPass = ref(17);
const observatory = ref<HTMLElement | null>(null);

let phaseTimer: number | undefined;
let intersectionObserver: IntersectionObserver | undefined;
let motionPreference: MediaQueryList | undefined;

const phase = computed(() => phases[activePhase.value]);
const phaseClass = computed(() => `phase-${phase.value.key}`);
const phaseStatus = computed(
  () => `${phase.value.label}: ${phase.value.detail}`,
);
const decisionId = computed(() => `decision/2026-08-29/${decisionPass.value}`);
const selectedLane = computed(() =>
  activePhase.value === phases.length - 1 ? "10 minutes" : "pending",
);

function scaleX(value: number) {
  const minimum = -0.4;
  const maximum = 1;
  return 34 + ((value - minimum) / (maximum - minimum)) * 382;
}

function densityPath(lane: FrequencyLane) {
  const standardDeviation = Math.max((lane.high - lane.low) / 3.29, 0.08);
  const points: string[] = [];
  for (let index = 0; index <= 36; index += 1) {
    const value = -0.4 + (index / 36) * 1.4;
    const z = (value - lane.posteriorMean) / standardDeviation;
    const density = Math.exp(-0.5 * z * z);
    points.push(`${scaleX(value).toFixed(1)},${(77 - density * 53).toFixed(1)}`);
  }
  return `M34,78 L${points.join(" L")} L416,78 Z`;
}

function formatEffect(value: number) {
  const sign = value >= 0 ? "+" : "−";
  return `${sign}${Math.abs(value).toFixed(2)}%`;
}

function laneStatus(lane: FrequencyLane) {
  if (activePhase.value < 2) return "Pending";
  if (lane.status === "rejected") return "Rejected";
  if (activePhase.value < phases.length - 1) return "Eligible";
  return lane.status === "selected" ? "Selected" : "Eligible";
}

function laneConstraint(lane: FrequencyLane) {
  if (activePhase.value < 2) return "awaiting feasibility";
  if (lane.status === "selected" && activePhase.value < phases.length - 1) {
    return "all gates passed";
  }
  return lane.constraint;
}

function advancePhase() {
  if (activePhase.value === phases.length - 1) {
    activePhase.value = 0;
    decisionPass.value += 1;
  } else {
    activePhase.value += 1;
  }
}

function selectPhase(index: number) {
  isPlaying.value = false;
  activePhase.value = index;
}

function togglePlaying() {
  isPlaying.value = !isPlaying.value;
}

function replayDecision() {
  isPlaying.value = false;
  activePhase.value = 0;
  decisionPass.value += 1;
}

function handleVisibilityChange() {
  pageHidden.value = document.hidden;
}

function handleMotionPreference(event: MediaQueryListEvent | MediaQueryList) {
  reducedMotion.value = event.matches;
  if (event.matches) {
    isPlaying.value = false;
    activePhase.value = phases.length - 1;
  }
}

onMounted(() => {
  pageHidden.value = document.hidden;
  motionPreference = window.matchMedia("(prefers-reduced-motion: reduce)");
  handleMotionPreference(motionPreference);
  motionPreference.addEventListener("change", handleMotionPreference);
  document.addEventListener("visibilitychange", handleVisibilityChange);

  if (observatory.value) {
    intersectionObserver = new IntersectionObserver(
      ([entry]) => {
        isVisible.value = entry.isIntersecting;
      },
      { threshold: 0.12 },
    );
    intersectionObserver.observe(observatory.value);
  }

  phaseTimer = window.setInterval(() => {
    if (
      isPlaying.value &&
      isVisible.value &&
      !pageHidden.value &&
      !reducedMotion.value
    ) {
      advancePhase();
    }
  }, 1_650);
});

onBeforeUnmount(() => {
  if (phaseTimer) window.clearInterval(phaseTimer);
  intersectionObserver?.disconnect();
  motionPreference?.removeEventListener("change", handleMotionPreference);
  document.removeEventListener("visibilitychange", handleVisibilityChange);
});
</script>

<template>
  <section
    ref="observatory"
    class="posterior-observatory atlas-plate"
    :class="phaseClass"
    aria-labelledby="posterior-title"
  >
    <div class="posterior-heading">
      <div>
        <h2 id="posterior-title">Watch uncertainty become a constrained decision.</h2>
        <p>
          Three related horizons update, partially pool, pass explicit
          feasibility gates, and draw only when eligible. Every value below is synthetic.
        </p>
      </div>
      <a href="./guide/build-a-thompson-portfolio">
        Build the portfolio <span aria-hidden="true">→</span>
      </a>
    </div>

    <div class="posterior-sequence" aria-label="Posterior decision phases">
      <button
        v-for="(item, index) in phases"
        :key="item.key"
        type="button"
        :class="{
          'is-active': activePhase === index,
          'is-complete': activePhase > index,
        }"
        :aria-pressed="activePhase === index"
        @click="selectPhase(index)"
      >
        <span>{{ String(index + 1).padStart(2, "0") }}</span>
        <strong>{{ item.label }}</strong>
      </button>
      <div class="posterior-sweep" aria-hidden="true"></div>
    </div>

    <div class="posterior-console">
      <div class="posterior-lanes-scroll" tabindex="0" aria-label="Posterior frequency lanes">
        <div class="posterior-lanes">
          <article
            v-for="lane in lanes"
            :key="lane.id"
            class="posterior-lane"
            :class="`is-${lane.status}`"
          >
            <header>
              <span>{{ lane.frequency }}</span>
              <small>{{ lane.horizon }}</small>
              <b>n {{ lane.observations }}</b>
            </header>

            <div class="density-frame">
              <svg
                viewBox="0 0 450 112"
                role="img"
                :aria-labelledby="`${lane.id}-title ${lane.id}-description`"
              >
                <title :id="`${lane.id}-title`">
                  {{ lane.frequency }} synthetic effect posterior
                </title>
                <desc :id="`${lane.id}-description`">
                  Posterior mean {{ formatEffect(lane.posteriorMean) }}, interval
                  {{ formatEffect(lane.low) }} to {{ formatEffect(lane.high) }}.
                  <template v-if="lane.draw !== null">
                    Sampled utility {{ formatEffect(lane.draw) }}.
                  </template>
                  <template v-else>
                    This arm is rejected before sampling.
                  </template>
                </desc>
                <line x1="34" y1="78" x2="416" y2="78" class="density-axis" />
                <line :x1="scaleX(0)" y1="16" :x2="scaleX(0)" y2="84" class="density-zero" />
                <g class="observation-marks" aria-hidden="true">
                  <circle
                    v-for="(point, index) in lane.points"
                    :key="index"
                    :cx="scaleX(point)"
                    :cy="92 + (index % 2) * 7"
                    r="2.6"
                    :style="{ '--mark-index': index }"
                  />
                </g>
                <path :d="densityPath(lane)" class="density-shape" />
                <line
                  :x1="scaleX(lane.low)"
                  y1="82"
                  :x2="scaleX(lane.high)"
                  y2="82"
                  class="credible-line"
                />
                <path
                  :d="`M${scaleX(lane.low)},77v10 M${scaleX(lane.high)},77v10`"
                  class="credible-cap"
                />
                <g v-if="lane.draw !== null" class="sample-needle" aria-hidden="true">
                  <line :x1="scaleX(lane.draw)" y1="10" :x2="scaleX(lane.draw)" y2="78" />
                  <circle :cx="scaleX(lane.draw)" cy="10" r="4" />
                </g>
                <g class="density-labels" aria-hidden="true">
                  <text x="31" y="108">−0.40%</text>
                  <text :x="scaleX(0) - 12" y="108">0</text>
                  <text x="383" y="108">+1.00%</text>
                </g>
              </svg>
              <div class="posterior-measures">
                <span>
                  posterior μ
                  <b>{{ formatEffect(lane.posteriorMean) }}</b>
                </span>
                <span>
                  90% interval
                  <b>{{ formatEffect(lane.low) }} to {{ formatEffect(lane.high) }}</b>
                </span>
                <span class="draw-measure">
                  keyed draw
                  <b>{{ lane.draw === null ? "not sampled" : formatEffect(lane.draw) }}</b>
                </span>
              </div>
            </div>

            <div class="constraint-result" :class="`is-${lane.status}`">
              <span>{{ laneStatus(lane) }}</span>
              <b>{{ laneConstraint(lane) }}</b>
            </div>
          </article>
        </div>
      </div>
      <p class="posterior-pan-note">Swipe horizontally to compare all frequency lanes.</p>

      <aside class="decision-ledger" aria-label="Synthetic posterior decision ledger">
        <div class="decision-state" role="status" :aria-live="isPlaying ? 'off' : 'polite'">
          <span class="decision-lamp" :class="{ 'is-playing': isPlaying }" aria-hidden="true"></span>
          <p>{{ phaseStatus }}</p>
          <b>{{ activePhase + 1 }} / {{ phases.length }}</b>
        </div>
        <dl>
          <div><dt>Strategy fingerprint</dt><dd>9bd3…5a70</dd></div>
          <div><dt>Decision ID</dt><dd>{{ decisionId }}</dd></div>
          <div><dt>Sampler</dt><dd>ChaCha8 · v2</dd></div>
          <div><dt>Selected</dt><dd class="selected-value">{{ selectedLane }}</dd></div>
          <div><dt>Authority</dt><dd>research candidate</dd></div>
        </dl>
        <p>
          A posterior sample is an uncertainty-aware decision input. It is not a
          forecast guarantee or permission to trade.
        </p>
      </aside>
    </div>

    <div class="posterior-controls" aria-label="Posterior animation controls">
      <button type="button" @click="togglePlaying">
        <svg viewBox="0 0 16 16" aria-hidden="true">
          <path v-if="isPlaying" d="M5 3v10M11 3v10" />
          <path v-else d="m5 3 8 5-8 5z" />
        </svg>
        {{ isPlaying ? "Pause observatory" : "Play observatory" }}
      </button>
      <button type="button" @click="advancePhase">
        <svg viewBox="0 0 16 16" aria-hidden="true">
          <path d="m3 3 7 5-7 5zM12 3v10" />
        </svg>
        Advance one phase
      </button>
      <button type="button" @click="replayDecision">
        <svg viewBox="0 0 16 16" aria-hidden="true">
          <path d="M3.3 5.3A5.5 5.5 0 1 1 2.7 10M3 2v3.7h3.7" />
        </svg>
        Replay keyed decision
      </button>
    </div>
    <span class="registration registration-west" aria-hidden="true"></span>
  </section>
</template>
