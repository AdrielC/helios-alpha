<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";

type Frame = {
  input: string;
  stage: number;
  state: string;
  effect: string;
  timer: string;
  revision: number;
  frontier: string;
  terminal: string;
};

type Scenario = {
  key: string;
  label: string;
  outcome: string;
  stages: string[];
  frames: Frame[];
};

const scenarios: Scenario[] = [
  {
    key: "incident/041",
    label: "Candidate path",
    outcome: "completed",
    stages: ["Open", "Propagate", "Impact", "Market", "Terminal"],
    frames: [
      {
        input: "Precursor · available 10:00:00",
        stage: 0,
        state: "awaiting propagation",
        effect: "RequestPropagationModel",
        timer: "deadline/1 · 10:00:30",
        revision: 1,
        frontier: "09:59:59",
        terminal: "active",
      },
      {
        input: "Propagation · available 10:00:04",
        stage: 1,
        state: "joint probability 0.480",
        effect: "RequestInfrastructureModel",
        timer: "deadline/1 · 10:00:34",
        revision: 2,
        frontier: "10:00:03",
        terminal: "active",
      },
      {
        input: "Infrastructure · available 10:00:09",
        stage: 2,
        state: "joint probability 0.120",
        effect: "RequestMarketModel",
        timer: "deadline/1 · 10:00:39",
        revision: 3,
        frontier: "10:00:08",
        terminal: "active",
      },
      {
        input: "Market response · available 10:00:12",
        stage: 3,
        state: "candidate · net effect 0.013",
        effect: "Emit(Candidate)",
        timer: "cleared on completion",
        revision: 4,
        frontier: "10:00:11",
        terminal: "active",
      },
      {
        input: "Complete · available 10:00:12",
        stage: 4,
        state: "terminal tombstone retained",
        effect: "Completed",
        timer: "none",
        revision: 4,
        frontier: "10:00:12",
        terminal: "completed",
      },
    ],
  },
  {
    key: "incident/042",
    label: "Deadline path",
    outcome: "expired",
    stages: ["Open", "Await", "Advance", "Expire", "Terminal"],
    frames: [
      {
        input: "Precursor · available 10:01:00",
        stage: 0,
        state: "awaiting propagation",
        effect: "RequestPropagationModel",
        timer: "deadline/1 · 10:01:30",
        revision: 1,
        frontier: "10:00:59",
        terminal: "active",
      },
      {
        input: "No response yet · 10:01:29",
        stage: 1,
        state: "no response admitted",
        effect: "State remains bounded",
        timer: "deadline/1 · 10:01:30",
        revision: 1,
        frontier: "10:01:29",
        terminal: "active",
      },
      {
        input: "Advance frontier · 10:01:30",
        stage: 2,
        state: "deadline becomes causally due",
        effect: "TimerFired(deadline/1)",
        timer: "fired deterministically",
        revision: 2,
        frontier: "10:01:30",
        terminal: "active",
      },
      {
        input: "Timeout transition · 10:01:30",
        stage: 3,
        state: "expired output staged",
        effect: "Emit(Expired) + Complete",
        timer: "none",
        revision: 2,
        frontier: "10:01:30",
        terminal: "active",
      },
      {
        input: "Complete · available 10:01:30",
        stage: 4,
        state: "terminal tombstone retained",
        effect: "Completed",
        timer: "none",
        revision: 2,
        frontier: "10:01:30",
        terminal: "completed",
      },
    ],
  },
  {
    key: "incident/043",
    label: "Correction path",
    outcome: "superseded",
    stages: ["Open", "Correct", "Validate", "Replace", "Terminal"],
    frames: [
      {
        input: "Precursor · available 10:02:00",
        stage: 0,
        state: "awaiting propagation",
        effect: "RequestPropagationModel",
        timer: "deadline/1 · 10:02:30",
        revision: 1,
        frontier: "10:01:59",
        terminal: "active",
      },
      {
        input: "Corrected identity · 10:02:06",
        stage: 1,
        state: "replacement evidence admitted",
        effect: "Prepare Supersede",
        timer: "old deadline still protected",
        revision: 1,
        frontier: "10:02:05",
        terminal: "active",
      },
      {
        input: "Validate incident/043b · 10:02:06",
        stage: 2,
        state: "replacement model state valid",
        effect: "Stage replacement effects",
        timer: "new deadline validated",
        revision: 1,
        frontier: "10:02:05",
        terminal: "active",
      },
      {
        input: "Atomic replacement · 10:02:06",
        stage: 3,
        state: "incident/043b opened at revision 1",
        effect: "Superseded + Opened",
        timer: "replacement deadline active",
        revision: 2,
        frontier: "10:02:06",
        terminal: "active",
      },
      {
        input: "Retain old tombstone · 10:02:06",
        stage: 4,
        state: "incident/043b remains active",
        effect: "Terminal status retained for incident/043",
        timer: "replacement deadline active",
        revision: 2,
        frontier: "10:02:06",
        terminal: "superseded",
      },
    ],
  },
];

const selected = ref(0);
const frameIndex = ref(0);
const playing = ref(true);
const visible = ref(true);
const pageHidden = ref(false);
const reducedMotion = ref(false);
const atlas = ref<HTMLElement | null>(null);

let timer: number | undefined;
let observer: IntersectionObserver | undefined;
let motionPreference: MediaQueryList | undefined;

const scenario = computed(() => scenarios[selected.value]);
const frame = computed(() => scenario.value.frames[frameIndex.value]);
const status = computed(() =>
  playing.value
    ? `Replaying ${scenario.value.key}`
    : `Paused at revision ${frame.value.revision}`,
);

function advance() {
  const next = frameIndex.value + 1;
  frameIndex.value = next >= scenario.value.frames.length ? 0 : next;
}

function chooseScenario(index: number) {
  selected.value = index;
  frameIndex.value = 0;
  playing.value = false;
}

function toggle() {
  playing.value = !playing.value;
}

function step() {
  playing.value = false;
  advance();
}

function reset() {
  playing.value = false;
  frameIndex.value = 0;
}

function onVisibilityChange() {
  pageHidden.value = document.hidden;
}

function onMotionPreference(event: MediaQueryListEvent | MediaQueryList) {
  reducedMotion.value = event.matches;
  if (event.matches) playing.value = false;
}

onMounted(() => {
  pageHidden.value = document.hidden;
  motionPreference = window.matchMedia("(prefers-reduced-motion: reduce)");
  onMotionPreference(motionPreference);
  motionPreference.addEventListener("change", onMotionPreference);
  document.addEventListener("visibilitychange", onVisibilityChange);

  if (atlas.value) {
    observer = new IntersectionObserver(
      ([entry]) => {
        visible.value = entry.isIntersecting;
      },
      { threshold: 0.12 },
    );
    observer.observe(atlas.value);
  }

  timer = window.setInterval(() => {
    if (
      playing.value &&
      visible.value &&
      !pageHidden.value &&
      !reducedMotion.value
    ) {
      advance();
    }
  }, 1_550);
});

onBeforeUnmount(() => {
  if (timer) window.clearInterval(timer);
  observer?.disconnect();
  motionPreference?.removeEventListener("change", onMotionPreference);
  document.removeEventListener("visibilitychange", onVisibilityChange);
});
</script>

<template>
  <section ref="atlas" class="hypothesis-atlas" aria-labelledby="hypothesis-atlas-title">
    <header class="machine-heading">
      <div>
        <h2 id="hypothesis-atlas-title">Watch one key own its causal state.</h2>
        <p>Illustrative values · deterministic lifecycle mechanics</p>
      </div>
      <output aria-live="polite">{{ status }}</output>
    </header>

    <div class="scenario-selector" aria-label="Hypothesis outcomes">
      <button
        v-for="(item, index) in scenarios"
        :key="item.key"
        type="button"
        :class="{ 'is-selected': selected === index }"
        :aria-pressed="selected === index"
        @click="chooseScenario(index)"
      >
        <span>{{ item.key }}</span>
        <b>{{ item.label }}</b>
        <em>{{ item.outcome }}</em>
      </button>
    </div>

    <p class="swipe-instruction">Swipe horizontally to follow the full conditional path.</p>
    <div class="machine-graph-scroll" tabindex="0" aria-label="Conditional state path">
      <ol class="machine-graph">
        <li
          v-for="(stage, index) in scenario.stages"
          :key="stage"
          :class="{
            'is-active': frame.stage === index,
            'is-complete': frame.stage > index,
          }"
        >
          <span class="machine-index">0{{ index + 1 }}</span>
          <span class="machine-node" aria-hidden="true">
            <span></span>
          </span>
          <b>{{ stage }}</b>
          <span v-if="index < scenario.stages.length - 1" class="machine-connector" aria-hidden="true">
            <i></i>
          </span>
        </li>
      </ol>
    </div>

    <div class="machine-readout">
      <div class="input-register">
        <span>Machine input</span>
        <strong>{{ frame.input }}</strong>
        <p>{{ frame.effect }}</p>
      </div>

      <dl>
        <div>
          <dt>Key</dt>
          <dd>{{ scenario.key }}</dd>
        </div>
        <div>
          <dt>Model state</dt>
          <dd>{{ frame.state }}</dd>
        </div>
        <div>
          <dt>Revision</dt>
          <dd>{{ frame.revision }}</dd>
        </div>
        <div>
          <dt>Timer</dt>
          <dd>{{ frame.timer }}</dd>
        </div>
        <div>
          <dt>Frontier</dt>
          <dd>{{ frame.frontier }}</dd>
        </div>
        <div>
          <dt>Lifecycle</dt>
          <dd :class="{ 'is-terminal': frame.terminal !== 'active' }">
            {{ frame.terminal }}
          </dd>
        </div>
      </dl>
    </div>

    <footer class="machine-controls">
      <div>
        <button type="button" @click="toggle">{{ playing ? "Pause trace" : "Play trace" }}</button>
        <button type="button" @click="step">Advance one transition</button>
        <button type="button" @click="reset">Restore opening state</button>
      </div>
      <p>One actor owns mutation · outputs leave the state transition · snapshot follows the revision</p>
    </footer>
  </section>
</template>

<style scoped>
.hypothesis-atlas {
  width: min(920px, calc(100vw - 370px));
  margin: 34px 50% 48px;
  color: var(--atlas-ink);
  background: var(--atlas-ground);
  border: 1px solid var(--atlas-rule);
  transform: translateX(-50%);
}

.machine-heading {
  display: flex;
  gap: 24px;
  align-items: center;
  justify-content: space-between;
  min-height: 84px;
  padding: 16px 22px;
  border-bottom: 1px solid var(--atlas-rule);
}

.machine-heading h2 {
  padding-top: 0;
  margin: 0;
  font-size: clamp(21px, 1.8vw, 28px);
  line-height: 1.08;
  letter-spacing: -0.03em;
  border-top: 0;
}

.machine-heading p,
.machine-heading output,
.swipe-instruction,
.scenario-selector span,
.scenario-selector em,
.machine-index,
.input-register > span,
.machine-readout dt,
.machine-controls p {
  font-family: var(--vp-font-family-mono);
  font-variation-settings: "MONO" 1, "CASL" 0;
}

.machine-heading p {
  margin: 5px 0 0;
  color: var(--atlas-muted);
  font-size: 10px;
  letter-spacing: 0.06em;
  text-transform: uppercase;
}

.machine-heading output {
  flex: 0 0 auto;
  color: var(--atlas-blue);
  font-size: 11px;
}

.scenario-selector {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  border-bottom: 1px solid var(--atlas-rule);
}

.scenario-selector button {
  position: relative;
  display: grid;
  gap: 3px;
  min-width: 0;
  padding: 13px 16px 12px;
  color: var(--atlas-muted);
  text-align: left;
  background: transparent;
  border: 0;
  border-right: 1px solid var(--atlas-rule-soft);
  cursor: pointer;
}

.scenario-selector button:last-child {
  border-right: 0;
}

.scenario-selector button::after {
  position: absolute;
  right: 0;
  bottom: -1px;
  left: 0;
  height: 2px;
  content: "";
  background: var(--atlas-blue);
  transform: scaleX(0);
  transform-origin: left;
  transition: transform 240ms cubic-bezier(0.16, 1, 0.3, 1);
}

.scenario-selector button:hover,
.scenario-selector button:focus-visible,
.scenario-selector button.is-selected {
  color: var(--atlas-ink);
  background: var(--vp-c-bg-soft);
}

.scenario-selector button.is-selected::after {
  transform: scaleX(1);
}

.scenario-selector span,
.scenario-selector em {
  overflow: hidden;
  font-size: 10px;
  font-style: normal;
  letter-spacing: 0.05em;
  text-overflow: ellipsis;
  text-transform: uppercase;
  white-space: nowrap;
}

.scenario-selector b {
  overflow: hidden;
  font-size: 14px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.scenario-selector em {
  color: var(--atlas-blue);
}

.swipe-instruction {
  display: none;
  padding: 8px 14px;
  margin: 0;
  color: var(--atlas-blue);
  font-size: 10px;
  text-transform: uppercase;
  border-bottom: 1px solid var(--atlas-rule-soft);
}

.machine-graph-scroll {
  overflow-x: auto;
  scrollbar-color: var(--atlas-blue) var(--atlas-blue-soft);
}

.machine-graph {
  display: grid;
  grid-template-columns: repeat(5, 1fr);
  min-width: 720px;
  padding: 24px 28px 20px;
  margin: 0;
  list-style: none;
  border-bottom: 1px solid var(--atlas-rule);
}

.machine-graph li {
  position: relative;
  display: grid;
  justify-items: center;
  color: var(--atlas-muted);
}

.machine-index {
  margin-bottom: 5px;
  color: var(--atlas-axis);
  font-size: 10px;
}

.machine-node {
  position: relative;
  z-index: 2;
  display: grid;
  width: 30px;
  height: 30px;
  place-items: center;
  background: var(--atlas-ground);
  border: 2px solid var(--atlas-rule);
  border-radius: 50%;
  transition: color 220ms ease, background 220ms ease, border-color 220ms ease;
}

.machine-node > span {
  width: 6px;
  height: 6px;
  background: currentcolor;
  border-radius: 50%;
}

.machine-graph b {
  margin-top: 7px;
  font-size: 12px;
}

.machine-connector {
  position: absolute;
  z-index: 1;
  top: 25px;
  left: calc(50% + 15px);
  width: calc(100% - 30px);
  height: 2px;
  overflow: hidden;
  background: var(--atlas-rule-soft);
}

.machine-connector i {
  display: block;
  width: 100%;
  height: 100%;
  background: var(--atlas-blue);
  transform: scaleX(0);
  transform-origin: left;
  transition: transform 560ms cubic-bezier(0.16, 1, 0.3, 1);
}

.machine-graph li.is-complete {
  color: var(--atlas-blue);
}

.machine-graph li.is-complete .machine-node {
  color: #fff;
  background: var(--atlas-blue);
  border-color: var(--atlas-blue);
}

.machine-graph li.is-complete .machine-connector i {
  transform: scaleX(1);
}

.machine-graph li.is-active {
  color: var(--atlas-oxide);
}

.machine-graph li.is-active .machine-node {
  color: #fff;
  background: var(--atlas-oxide);
  border-color: var(--atlas-oxide);
}

.machine-graph li.is-active .machine-node::after {
  position: absolute;
  inset: -7px;
  content: "";
  border: 1px solid var(--atlas-oxide);
  border-radius: 50%;
  animation: register-state 1.55s cubic-bezier(0.16, 1, 0.3, 1) infinite;
}

.machine-readout {
  display: grid;
  grid-template-columns: minmax(220px, 0.8fr) minmax(430px, 1.2fr);
}

.input-register {
  padding: 18px 22px;
  background: var(--vp-c-bg-soft);
  border-right: 1px solid var(--atlas-rule);
}

.input-register > span,
.machine-readout dt {
  color: var(--atlas-blue);
  font-size: 10px;
  letter-spacing: 0.06em;
  text-transform: uppercase;
}

.input-register strong {
  display: block;
  margin-top: 8px;
  font-size: 15px;
  line-height: 1.25;
}

.input-register p {
  margin: 7px 0 0;
  color: var(--atlas-muted);
  font-size: 12px;
  line-height: 1.4;
}

.machine-readout dl {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  margin: 0;
}

.machine-readout dl > div {
  min-width: 0;
  padding: 12px 14px;
  border-right: 1px solid var(--atlas-rule-soft);
  border-bottom: 1px solid var(--atlas-rule-soft);
}

.machine-readout dl > div:nth-child(3n) {
  border-right: 0;
}

.machine-readout dl > div:nth-child(n + 4) {
  border-bottom: 0;
}

.machine-readout dd {
  margin: 5px 0 0;
  overflow: hidden;
  font-family: var(--vp-font-family-mono);
  font-size: 11px;
  font-variation-settings: "MONO" 1, "CASL" 0;
  line-height: 1.35;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.machine-readout dd.is-terminal {
  color: #527d14;
}

.machine-controls {
  display: flex;
  gap: 20px;
  align-items: center;
  justify-content: space-between;
  min-height: 54px;
  padding: 10px 14px;
  border-top: 1px solid var(--atlas-rule);
}

.machine-controls > div {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.machine-controls button {
  min-height: 32px;
  padding: 0 10px;
  color: var(--atlas-blue);
  font-family: var(--vp-font-family-mono);
  font-size: 10px;
  font-variation-settings: "MONO" 1, "CASL" 0;
  background: transparent;
  border: 1px solid var(--atlas-rule);
  border-radius: 1px;
  cursor: pointer;
  transition: color 150ms ease, border-color 150ms ease, background 150ms ease;
}

.machine-controls button:hover,
.machine-controls button:focus-visible {
  color: var(--atlas-oxide);
  background: var(--vp-c-bg-soft);
  border-color: var(--atlas-oxide);
}

.scenario-selector button:focus-visible,
.machine-controls button:focus-visible,
.machine-graph-scroll:focus-visible {
  outline: 2px solid var(--atlas-oxide);
  outline-offset: 3px;
}

.machine-controls p {
  max-width: 310px;
  margin: 0;
  color: var(--atlas-muted);
  font-size: 10px;
  line-height: 1.45;
  text-align: right;
  text-transform: uppercase;
}

@keyframes register-state {
  0% {
    opacity: 0.8;
    transform: scale(0.72);
  }
  70%,
  100% {
    opacity: 0;
    transform: scale(1.28);
  }
}

@media (max-width: 1100px) {
  .hypothesis-atlas {
    width: 100%;
    margin-right: 0;
    margin-left: 0;
    transform: none;
  }

  .machine-readout {
    grid-template-columns: 1fr;
  }

  .input-register {
    border-right: 0;
    border-bottom: 1px solid var(--atlas-rule);
  }
}

@media (max-width: 700px) {
  .machine-heading,
  .machine-controls {
    align-items: flex-start;
    flex-direction: column;
  }

  .machine-heading output {
    align-self: flex-start;
  }

  .scenario-selector {
    grid-template-columns: 1fr;
  }

  .scenario-selector button {
    border-right: 0;
    border-bottom: 1px solid var(--atlas-rule-soft);
  }

  .scenario-selector button:last-child {
    border-bottom: 0;
  }

  .swipe-instruction {
    display: block;
  }

  .machine-readout dl {
    grid-template-columns: repeat(2, 1fr);
  }

  .machine-readout dl > div,
  .machine-readout dl > div:nth-child(3n) {
    border-right: 1px solid var(--atlas-rule-soft);
    border-bottom: 1px solid var(--atlas-rule-soft);
  }

  .machine-readout dl > div:nth-child(2n) {
    border-right: 0;
  }

  .machine-readout dl > div:nth-child(n + 5) {
    border-bottom: 0;
  }

  .machine-controls p {
    max-width: none;
    text-align: left;
  }
}

@media (prefers-reduced-motion: reduce) {
  .machine-connector i,
  .scenario-selector button::after {
    transition-duration: 1ms;
  }

  .machine-graph li.is-active .machine-node::after {
    opacity: 0.55;
    animation: none;
    transform: none;
  }
}
</style>
