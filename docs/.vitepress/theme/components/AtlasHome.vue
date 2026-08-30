<script setup lang="ts">
import {
  computed,
  nextTick,
  onBeforeUnmount,
  onMounted,
  ref,
  watch,
} from "vue";
import PosteriorObservatory from "./PosteriorObservatory.vue";

type Stage = {
  key: string;
  index: string;
  label: string;
  method: string;
  detail: string;
  state: string;
};

const stages: Stage[] = [
  {
    key: "source",
    index: "01",
    label: "Admit",
    method: "step(event)",
    detail: "Accept the observation only after its availability time, while retaining its source offset.",
    state: "available_at ≤ 09:39:42Z · offset 184,512",
  },
  {
    key: "reorder",
    index: "02",
    label: "Reorder",
    method: "watermark(t)",
    detail: "Hold bounded disorder, release in event-time order, and reject late or overflowing input explicitly.",
    state: "pending 7 / 4,096 · watermark 09:39:42Z",
  },
  {
    key: "bucket",
    index: "03",
    label: "10m bucket",
    method: "reduce(value)",
    detail: "Place the event in [09:30, 09:40). Finalize only when the watermark closes that interval.",
    state: "open [09:30, 09:40) · n 511",
  },
  {
    key: "moments",
    index: "04",
    label: "Moments",
    method: "try_push(x)",
    detail: "Update count, mean, and M2 in constant space. Merge partitions through a fixed tree.",
    state: "n 512 · μ 0.18 · σ 1.07",
  },
  {
    key: "signal",
    index: "05",
    label: "Decide",
    method: "emit(output)",
    detail: "Apply injected research policy to the closed summary. The result is a candidate, not an order.",
    state: "candidate true · authority none",
  },
  {
    key: "checkpoint",
    index: "06",
    label: "Checkpoint",
    method: "write(offset)",
    detail: "Bind operator state to its source offset, watermark, snapshot version, and pipeline fingerprint.",
    state: "snapshot v1 · fingerprint compatible",
  },
];

const activeIndex = ref(0);
const isPlaying = ref(true);
const isVisible = ref(true);
const pageHidden = ref(false);
const reducedMotion = ref(false);
const replayPass = ref(1);
const replayPhase = ref<"streaming" | "paused" | "restoring" | "restored">(
  "streaming",
);
const pipelineRail = ref<HTMLElement | null>(null);
const pulseX = ref(0);
const pulseY = ref(0);

let replayTimer: number | undefined;
let restartTimer: number | undefined;
let intersectionObserver: IntersectionObserver | undefined;
let resizeObserver: ResizeObserver | undefined;
let motionPreference: MediaQueryList | undefined;

const activeStage = computed(() => stages[activeIndex.value]);
const pulseStyle = computed(() => ({
  transform: `translate3d(${pulseX.value}px, ${pulseY.value}px, 0)`,
}));
const replayStatus = computed(() => {
  if (replayPhase.value === "restoring") {
    return "Checkpoint loaded · validating fingerprint";
  }
  if (replayPhase.value === "restored") {
    return "State restored · source resumes after offset";
  }
  if (!isPlaying.value) {
    return "Replay paused · inspect any operator";
  }
  return `Synthetic replay · pass ${String(replayPass.value).padStart(2, "0")}`;
});
const replayOffset = computed(() =>
  (184_512 + (replayPass.value - 1) * stages.length + activeIndex.value).toLocaleString(
    "en-US",
  ),
);
const checkpointState = computed(() => {
  if (replayPhase.value === "restoring") return "v1 · validating";
  if (replayPass.value > 1) return "v1 · restored";
  if (activeIndex.value === stages.length - 1) return "v1 · captured";
  return "v1 · ready";
});
const resumeState = computed(() =>
  replayPass.value > 1 || activeIndex.value === stages.length - 1
    ? "compatible"
    : "pending",
);

function updatePulsePosition() {
  void nextTick(() => {
    const rail = pipelineRail.value;
    const node = rail?.querySelectorAll<HTMLElement>(".stage-node")[activeIndex.value];
    if (!rail || !node) return;
    const railRect = rail.getBoundingClientRect();
    const nodeRect = node.getBoundingClientRect();
    pulseX.value = nodeRect.left - railRect.left + nodeRect.width / 2 - 5;
    pulseY.value = nodeRect.top - railRect.top + nodeRect.height / 2 - 5;
  });
}

function advanceReplay() {
  replayPhase.value = "streaming";
  if (activeIndex.value === stages.length - 1) {
    replayPass.value += 1;
    activeIndex.value = 0;
    return;
  }
  activeIndex.value += 1;
}

function cancelPendingRestore() {
  if (restartTimer) {
    window.clearTimeout(restartTimer);
    restartTimer = undefined;
  }
}

function selectStage(index: number) {
  cancelPendingRestore();
  isPlaying.value = false;
  replayPhase.value = "paused";
  activeIndex.value = index;
}

function toggleReplay() {
  cancelPendingRestore();
  isPlaying.value = !isPlaying.value;
  replayPhase.value = isPlaying.value ? "streaming" : "paused";
}

function stepReplay() {
  cancelPendingRestore();
  isPlaying.value = false;
  advanceReplay();
  replayPhase.value = "paused";
}

function restartFromCheckpoint() {
  cancelPendingRestore();
  isPlaying.value = false;
  replayPhase.value = "restoring";
  activeIndex.value = stages.length - 1;

  const finishRestore = () => {
    restartTimer = undefined;
    replayPass.value += 1;
    activeIndex.value = 0;
    replayPhase.value = "restored";
  };

  if (reducedMotion.value) {
    finishRestore();
  } else {
    restartTimer = window.setTimeout(finishRestore, 620);
  }
}

function handleVisibilityChange() {
  pageHidden.value = document.hidden;
}

function handleMotionPreference(event: MediaQueryListEvent | MediaQueryList) {
  reducedMotion.value = event.matches;
  if (event.matches) {
    isPlaying.value = false;
    replayPhase.value = "paused";
  }
}

watch(activeIndex, updatePulsePosition, { flush: "post" });

onMounted(() => {
  pageHidden.value = document.hidden;
  motionPreference = window.matchMedia("(prefers-reduced-motion: reduce)");
  handleMotionPreference(motionPreference);
  motionPreference.addEventListener("change", handleMotionPreference);
  document.addEventListener("visibilitychange", handleVisibilityChange);
  window.addEventListener("resize", updatePulsePosition, { passive: true });

  if (pipelineRail.value) {
    intersectionObserver = new IntersectionObserver(
      ([entry]) => {
        isVisible.value = entry.isIntersecting;
      },
      { threshold: 0.15 },
    );
    intersectionObserver.observe(pipelineRail.value);

    resizeObserver = new ResizeObserver(updatePulsePosition);
    resizeObserver.observe(pipelineRail.value);
  }

  replayTimer = window.setInterval(() => {
    if (
      isPlaying.value &&
      isVisible.value &&
      !pageHidden.value &&
      !reducedMotion.value
    ) {
      advanceReplay();
    }
  }, 1_350);
  updatePulsePosition();
});

onBeforeUnmount(() => {
  if (replayTimer) window.clearInterval(replayTimer);
  if (restartTimer) window.clearTimeout(restartTimer);
  intersectionObserver?.disconnect();
  resizeObserver?.disconnect();
  motionPreference?.removeEventListener("change", handleMotionPreference);
  document.removeEventListener("visibilitychange", handleVisibilityChange);
  window.removeEventListener("resize", updatePulsePosition);
});
</script>

<template>
  <main class="atlas" aria-labelledby="atlas-title">
    <section class="atlas-intro atlas-plate" aria-labelledby="atlas-title">
      <div class="atlas-intro-copy">
        <h1 id="atlas-title">Build event pipelines that survive disorder and restarts.</h1>
        <p class="plate-meta">Annotated event atlas · causal Rust runtime</p>
        <p>
          Compose typed state machines for admission, watermarks, online inference,
          and exact resume. Domain policy stays injected. Order authority stays out.
        </p>
        <a class="atlas-primary" href="./guide/compose-a-strategy">
          Build the 10-minute pipeline <span aria-hidden="true">→</span>
        </a>
      </div>

      <dl class="atlas-spec" aria-label="System specification">
        <div>
          <dt>Ingress</dt>
          <dd><strong>Timed&lt;T&gt;</strong><span>event + availability</span></dd>
        </div>
        <div>
          <dt>Ordering</dt>
          <dd><strong>bounded</strong><span>watermark released</span></dd>
        </div>
        <div>
          <dt>State</dt>
          <dd><strong>online</strong><span>mergeable + typed</span></dd>
        </div>
        <div>
          <dt>Resume</dt>
          <dd><strong>exact</strong><span>offset + snapshot</span></dd>
        </div>
      </dl>
      <span class="registration registration-east" aria-hidden="true"></span>
    </section>

    <section class="pipeline atlas-plate" aria-labelledby="pipeline-title">
      <div class="section-heading">
        <div>
          <h2 id="pipeline-title">One observation crosses six explicit state owners.</h2>
          <p class="plate-meta">Causal trace · synthetic values</p>
        </div>
        <p class="section-note">
          Run the trace, advance one stage, or select an operator to inspect it.
        </p>
      </div>

      <div ref="pipelineRail" class="pipeline-rail" aria-label="Streaming pipeline stages">
        <button
          v-for="(stage, index) in stages"
          :key="stage.key"
          class="pipeline-stage"
          :class="{
            'is-active': activeIndex === index,
            'is-complete': activeIndex > index,
          }"
          type="button"
          :aria-pressed="activeIndex === index"
          @click="selectStage(index)"
        >
          <span class="stage-index">{{ stage.index }}</span>
          <span class="stage-node" aria-hidden="true">
            <svg v-if="stage.key === 'source'" class="stage-icon" viewBox="0 0 20 20">
              <ellipse cx="10" cy="5" rx="5.5" ry="2.5" />
              <path d="M4.5 5v5c0 1.4 2.5 2.5 5.5 2.5s5.5-1.1 5.5-2.5V5" />
              <path d="M4.5 10v5c0 1.4 2.5 2.5 5.5 2.5s5.5-1.1 5.5-2.5v-5" />
            </svg>
            <svg v-else-if="stage.key === 'reorder'" class="stage-icon" viewBox="0 0 20 20">
              <path d="M4 5h10.5M4 10h8M4 15h5.5" />
              <path d="m13 12 3 3-3 3M16 15H9.5" />
            </svg>
            <svg v-else-if="stage.key === 'bucket'" class="stage-icon" viewBox="0 0 20 20">
              <rect x="3.5" y="3.5" width="13" height="13" rx="1" />
              <path d="M3.5 8h13M8 8v8.5M12 8v8.5" />
            </svg>
            <svg v-else-if="stage.key === 'moments'" class="stage-icon" viewBox="0 0 20 20">
              <path d="M3 16.5h14M3.5 16V3.5" />
              <polyline points="5,13 8,10 10.5,12 14.5,6 17,8" />
            </svg>
            <svg v-else-if="stage.key === 'signal'" class="stage-icon" viewBox="0 0 20 20">
              <circle cx="10" cy="10" r="5.5" />
              <circle cx="10" cy="10" r="1.5" />
              <path d="M10 1.5v3M18.5 10h-3M10 18.5v-3M1.5 10h3" />
            </svg>
            <svg v-else class="stage-icon" viewBox="0 0 20 20">
              <path d="M5 2.5h7l3 3V17.5H5z" />
              <path d="M12 2.5v3h3M7.5 11l1.7 1.8 3.6-4" />
            </svg>
          </span>
          <span class="stage-label">{{ stage.label }}</span>
          <span class="stage-method">{{ stage.method }}</span>
          <span v-if="index < stages.length - 1" class="stage-connector" aria-hidden="true"></span>
        </button>
        <span
          class="event-pulse"
          :class="{ 'is-restoring': replayPhase === 'restoring' }"
          :style="pulseStyle"
          aria-hidden="true"
        ></span>
      </div>

      <div class="pipeline-console">
        <div class="replay-status" role="status" :aria-live="isPlaying ? 'off' : 'polite'">
          <span class="replay-lamp" :class="{ 'is-live': isPlaying }" aria-hidden="true"></span>
          <span>{{ replayStatus }}</span>
          <b>{{ activeIndex + 1 }} / {{ stages.length }}</b>
        </div>
        <div class="replay-controls" aria-label="Synthetic pipeline replay controls">
          <button type="button" class="pipeline-control" @click="toggleReplay">
            <svg viewBox="0 0 16 16" aria-hidden="true">
              <path v-if="isPlaying" d="M5 3v10M11 3v10" />
              <path v-else d="m5 3 8 5-8 5z" />
            </svg>
            {{ isPlaying ? "Pause trace" : "Play trace" }}
          </button>
          <button type="button" class="pipeline-control" @click="stepReplay">
            <svg viewBox="0 0 16 16" aria-hidden="true">
              <path d="m3 3 7 5-7 5zM12 3v10" />
            </svg>
            Advance one stage
          </button>
          <button type="button" class="pipeline-control" @click="restartFromCheckpoint">
            <svg viewBox="0 0 16 16" aria-hidden="true">
              <path d="M3.3 5.3A5.5 5.5 0 1 1 2.7 10M3 2v3.7h3.7" />
            </svg>
            Restore checkpoint
          </button>
        </div>
      </div>

      <div class="stage-readout" :aria-live="isPlaying ? 'off' : 'polite'">
        <span class="readout-label">Inspecting · {{ activeStage.index }}</span>
        <strong>{{ activeStage.label }}</strong>
        <p>{{ activeStage.detail }}</p>
        <code>{{ activeStage.state }}</code>
      </div>
      <span class="registration registration-west" aria-hidden="true"></span>
    </section>

    <section
      class="evidence-grid atlas-plate"
      :class="{ 'is-signal-ready': activeIndex >= 4 }"
      aria-labelledby="evidence-title"
    >
      <div class="plot-panel">
        <div class="plot-header">
          <div>
            <h2 id="evidence-title">Synthetic event study aligned at t = 0</h2>
            <p class="plate-meta">Illustrative response · not estimated alpha</p>
          </div>
          <dl class="plot-legend" aria-label="Plot legend">
            <div><dt class="legend-line"></dt><dd>Mean response</dd></div>
            <div><dt class="legend-band"></dt><dd>95% interval</dd></div>
            <div><dt class="legend-event"></dt><dd>Event at t = 0</dd></div>
          </dl>
        </div>
        <p class="plot-pan-note">Swipe horizontally to inspect the full time axis.</p>

        <svg
          class="event-plot"
          viewBox="0 0 900 350"
          role="img"
          aria-labelledby="plot-title plot-description"
        >
          <title id="plot-title">Synthetic response aligned around an event</title>
          <desc id="plot-description">
            A synthetic pre-event baseline spikes at time zero, decays below baseline,
            and gradually recovers. It demonstrates alignment and state transitions.
          </desc>
          <defs>
            <pattern id="minor-grid" width="35" height="44" patternUnits="userSpaceOnUse">
              <path class="plot-grid-path" d="M 35 0 L 0 0 0 44" fill="none" stroke-width="1" />
            </pattern>
          </defs>
          <rect x="70" y="28" width="790" height="264" fill="url(#minor-grid)" />
          <line x1="70" y1="160" x2="860" y2="160" class="axis baseline" />
          <line x1="70" y1="292" x2="860" y2="292" class="axis" />
          <line x1="70" y1="28" x2="70" y2="292" class="axis" />

          <path
            class="confidence-band"
            d="M70 151 L102 147 L134 153 L166 149 L198 157 L230 151 L262 145 L294 150 L326 143 L358 151 L390 139 L418 126 L438 80 L454 65 L470 77 L488 103 L510 143 L534 181 L560 205 L590 219 L622 225 L654 218 L686 204 L718 190 L750 179 L782 174 L820 177 L860 181 L860 229 L820 225 L782 231 L750 240 L718 254 L686 267 L654 276 L622 278 L590 270 L560 255 L534 232 L510 199 L488 162 L470 128 L454 117 L438 131 L418 171 L390 183 L358 191 L326 183 L294 190 L262 185 L230 191 L198 197 L166 189 L134 193 L102 187 L70 191 Z"
          />
          <polyline
            class="response-line"
            pathLength="1"
            points="70,170 102,166 134,173 166,169 198,177 230,171 262,165 294,170 326,162 358,171 390,160 418,149 438,105 454,91 470,102 488,128 510,169 534,207 560,230 590,244 622,250 654,243 686,229 718,216 750,204 782,198 820,201 860,205"
          />
          <g class="sample-marks" aria-hidden="true">
            <circle cx="102" cy="166" r="2" /><circle cx="134" cy="173" r="2" />
            <circle cx="166" cy="169" r="2" /><circle cx="198" cy="177" r="2" />
            <circle cx="230" cy="171" r="2" /><circle cx="262" cy="165" r="2" />
            <circle cx="294" cy="170" r="2" /><circle cx="326" cy="162" r="2" />
            <circle cx="358" cy="171" r="2" /><circle cx="390" cy="160" r="2" />
            <circle cx="418" cy="149" r="2" /><circle cx="438" cy="105" r="2" />
            <circle cx="470" cy="102" r="2" /><circle cx="488" cy="128" r="2" />
            <circle cx="510" cy="169" r="2" /><circle cx="534" cy="207" r="2" />
            <circle cx="560" cy="230" r="2" /><circle cx="590" cy="244" r="2" />
            <circle cx="622" cy="250" r="2" /><circle cx="654" cy="243" r="2" />
            <circle cx="686" cy="229" r="2" /><circle cx="718" cy="216" r="2" />
            <circle cx="750" cy="204" r="2" /><circle cx="782" cy="198" r="2" />
            <circle cx="820" cy="201" r="2" /><circle cx="860" cy="205" r="2" />
          </g>
          <line x1="454" y1="28" x2="454" y2="292" class="event-line" />
          <circle cx="454" cy="91" r="7" class="event-dot" />
          <circle cx="718" cy="216" r="6" class="recovery-dot" />

          <g class="plot-labels">
            <text x="70" y="320">−60m</text>
            <text x="192" y="320">−40m</text>
            <text x="326" y="320">−20m</text>
            <text x="444" y="320" class="event-text">t = 0</text>
            <text x="565" y="320">+20m</text>
            <text x="697" y="320">+40m</text>
            <text x="829" y="320">+60m</text>
            <text x="38" y="35">+3σ</text>
            <text x="45" y="165">0</text>
            <text x="38" y="290">−3σ</text>
          </g>

          <g class="plot-callout">
            <line x1="210" y1="108" x2="240" y2="168" />
            <circle cx="240" cy="168" r="3" />
            <text x="112" y="86">PRE-EVENT BASELINE</text>
            <text x="112" y="104" class="callout-copy">State exists before any emission.</text>
          </g>
          <g class="plot-callout callout-event">
            <line x1="520" y1="70" x2="470" y2="102" />
            <text x="520" y="48">IMMEDIATE UPDATE</text>
            <text x="520" y="66" class="callout-copy">Availability gates computation.</text>
          </g>
          <g class="plot-callout">
            <line x1="736" y1="176" x2="718" y2="211" />
            <text x="738" y="150">RECOVERY PATH</text>
            <text x="738" y="170" class="callout-copy">Candidate state remains replayable.</text>
          </g>
        </svg>

        <dl class="plot-stats" aria-label="Synthetic plot summary">
          <div><dt>Demo events</dt><dd>512</dd></div>
          <div><dt>Pre mean</dt><dd>0.02σ</dd></div>
          <div><dt>Peak</dt><dd>2.74σ</dd></div>
          <div><dt>Recovery</dt><dd>43m</dd></div>
          <div><dt>Data</dt><dd>Synthetic</dd></div>
        </dl>
      </div>

      <aside class="evidence-ledger" aria-label="Annotations and restart state">
        <section class="ledger-section">
          <div class="ledger-tabs" aria-hidden="true">
            <span class="is-active">Annotations</span>
            <span>Restart state</span>
          </div>
          <ol class="annotation-list">
            <li>
              <span>01</span>
              Availability is checked before the event enters feature state.
            </li>
            <li>
              <span>02</span>
              Late arrival and overflow are explicit outcomes, not silent drops.
            </li>
            <li>
              <span>03</span>
              Variance uses Welford updates and a fixed Chan merge tree.
            </li>
            <li>
              <span>04</span>
              Research policy is injected at the composition boundary.
            </li>
          </ol>
        </section>

        <section class="ledger-section checkpoint-section">
          <p class="ledger-label">Synthetic runtime trace</p>
          <dl class="checkpoint-list">
            <div><dt>Offset</dt><dd>{{ replayOffset }}</dd></div>
            <div><dt>Watermark</dt><dd>09:40:00Z</dd></div>
            <div><dt>Snapshot</dt><dd>{{ checkpointState }}</dd></div>
            <div><dt>Fingerprint</dt><dd>7a2c…9e1b</dd></div>
            <div>
              <dt>Resume</dt>
              <dd :class="{ verified: resumeState === 'compatible' }">{{ resumeState }}</dd>
            </div>
          </dl>
          <a href="./concepts/checkpoints">Inspect the restart contract →</a>
        </section>
      </aside>
      <span class="registration registration-south" aria-hidden="true"></span>
    </section>

    <section class="evidence-strip" aria-label="Evidence status">
      <strong>Illustrative series</strong>
      <span>The plot explains alignment and state. It is not an empirical result.</span>
      <span class="evidence-boundary">
        Mechanics <b>tested</b> · Alpha <b>unproven</b>
      </span>
    </section>

    <PosteriorObservatory />

    <section class="atlas-section premise" aria-labelledby="premise-title">
      <div class="section-number" aria-hidden="true">01</div>
      <div class="section-copy">
        <h2 id="premise-title">Define what the strategy can know before writing code.</h2>
        <p class="plate-meta">Research contract</p>
      </div>
      <dl class="research-contract" aria-label="Example research contract">
        <div>
          <dt>Event</dt>
          <dd>A timestamped observation with a documented detection rule.</dd>
        </div>
        <div>
          <dt>Available at</dt>
          <dd>The first instant the strategy could have observed it.</dd>
        </div>
        <div>
          <dt>Decision</dt>
          <dd>A candidate emitted only from a watermark-closed summary.</dd>
        </div>
        <div>
          <dt>Falsifier</dt>
          <dd>Pre-registered controls, costs, and out-of-sample failure criteria.</dd>
        </div>
      </dl>
      <span class="registration registration-east registration-center" aria-hidden="true"></span>
    </section>

    <section class="composition-section atlas-section" aria-labelledby="composition-title">
      <div class="section-number" aria-hidden="true">02</div>
      <div class="section-copy">
        <h2 id="composition-title">Own only the state the research question requires.</h2>
        <p class="plate-meta">Static policy injection</p>
        <p>
          Reducers and decision rules are ordinary Rust values. Ordering, flushing,
          and snapshots remain generic, statically dispatched, and allocation-aware.
        </p>
        <a href="./guide/compose-a-strategy">Build the 10-minute pipeline →</a>
      </div>
      <div class="code-plate" aria-label="Rust composition example">
        <div class="code-plate-bar">
          <span>pipeline.rs</span>
          <span>injected reducer · explicit watermark</span>
        </div>
        <pre><code><span class="code-keyword">let</span> pipeline = OrderedBucketPipeline::try_new(
    <span class="code-number">4_096</span>,
    SecondWallBucket { width_sec: <span class="code-number">600</span> },
    F64MomentsReducer::new(project_value),
)?;

pipeline.flush(
    &amp;<span class="code-keyword">mut</span> state,
    FlushReason::Watermark(watermark),
    &amp;<span class="code-keyword">mut</span> emit,
);</code></pre>
      </div>
    </section>

    <section class="crate-section atlas-section" aria-labelledby="crate-title">
      <div class="section-number" aria-hidden="true">03</div>
      <div class="section-copy">
        <h2 id="crate-title">Follow the evidence in research order.</h2>
        <p class="plate-meta">Four-part walkthrough</p>
      </div>
      <div class="crate-list">
        <a href="./research/rare-events">
          <span>Step 01 · define</span>
          <strong>Specify the event and its evidence</strong>
          <p>Separate event time, availability, alignment, exposure, and the falsifier.</p>
        </a>
        <a href="./concepts/event-time">
          <span>Step 02 · constrain</span>
          <strong>Make time semantics causal</strong>
          <p>Gate availability, bound disorder, advance watermarks, and close buckets.</p>
        </a>
        <a href="./guide/compose-a-strategy">
          <span>Step 03 · compose</span>
          <strong>Build the typed operator chain</strong>
          <p>Inject the projection, accumulate stable moments, and emit a candidate.</p>
        </a>
        <a href="./guide/restart-a-pipeline">
          <span>Step 04 · prove</span>
          <strong>Replay, restore, and compare</strong>
          <p>Require identical output across incremental, batch, and checkpointed runs.</p>
        </a>
      </div>
    </section>

    <section class="boundary-section atlas-section" aria-labelledby="boundary-title">
      <div class="section-number" aria-hidden="true">04</div>
      <div class="section-copy">
        <h2 id="boundary-title">Stop at the capital boundary.</h2>
        <p class="plate-meta">Capital boundary</p>
      </div>
      <div class="boundary-columns">
        <div>
          <h3>Mechanics you can test now</h3>
          <ul>
            <li>Deterministic replay and checkpoint-resume equivalence</li>
            <li>Bounded ordering with typed failure outcomes</li>
            <li>Numerically stable, mergeable online statistics</li>
            <li>Static composition without per-step allocation by default</li>
          </ul>
        </div>
        <div>
          <h3>Evidence required before capital</h3>
          <ul>
            <li>Atomic source, checkpoint, and sink coordination</li>
            <li>Venue-grade calendars, broker integration, and risk limits</li>
            <li>Transaction-cost, slippage, and capacity modeling</li>
            <li>Operational observability, incident response, and deployment proof</li>
          </ul>
        </div>
      </div>
      <a class="atlas-primary secondary" href="./operations/production-readiness">
        Audit the production gap <span aria-hidden="true">→</span>
      </a>
      <span class="registration registration-west registration-bottom" aria-hidden="true"></span>
    </section>
  </main>
</template>
