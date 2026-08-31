<script setup lang="ts">
import { computed, ref, shallowRef } from "vue";
import {
  type CommandAction,
  type CommandAuthority,
  type CommandPort,
  type CommandReceipt,
} from "../operations/command-port";
import type { OperationsSnapshot } from "../operations/operations-port";

const props = defineProps<{
  snapshot: OperationsSnapshot;
  stale: boolean;
  authority: CommandAuthority;
  port: CommandPort;
}>();
const emit = defineEmits<{ authority: [authority: CommandAuthority] }>();
type ControlAction = Exclude<CommandAction, "submit_order">;

const commandDraft = ref<{
  action: ControlAction;
  targetId: string;
  label: string;
  confirmationPhrase: string;
}>();
const commandReason = ref("");
const commandConfirmation = ref("");
const commandFailure = ref("");
const commandReceipt = shallowRef<CommandReceipt>();
const commandBusy = ref(false);
const canCommand = computed(
  () =>
    !props.stale &&
    props.authority.state === "authenticated" &&
    !commandBusy.value,
);
const canEmergencyCommand = computed(
  () => props.authority.state === "authenticated" && !commandBusy.value,
);
const authorityLabel = computed(() => {
  if (props.authority.state === "authenticated") return props.authority.operator ?? "Command ready";
  if (props.authority.state === "expired") return "Session expired";
  return "Read only";
});

function prepareCommand(
  action: ControlAction,
  targetId: string,
  label: string,
  confirmationPhrase: string,
): void {
  commandDraft.value = { action, targetId, label, confirmationPhrase };
  commandReason.value = "";
  commandConfirmation.value = "";
  commandFailure.value = "";
  commandReceipt.value = undefined;
}

function dismissCommand(): void {
  commandDraft.value = undefined;
  commandReason.value = "";
  commandConfirmation.value = "";
  commandFailure.value = "";
}

async function issueCommand(): Promise<void> {
  const draft = commandDraft.value;
  if (!draft) return;
  if (commandConfirmation.value !== draft.confirmationPhrase) {
    commandFailure.value = "Typed confirmation does not match";
    return;
  }
  commandBusy.value = true;
  commandFailure.value = "";
  try {
    commandReceipt.value = await props.port.execute(
      {
        action: draft.action,
        targetId: draft.targetId,
        reason: commandReason.value.trim(),
        confirmation: commandConfirmation.value,
      },
      props.snapshot.sequence,
    );
    commandDraft.value = undefined;
  } catch (error) {
    commandFailure.value = error instanceof Error ? error.message : "Command request failed";
    emit("authority", await props.port.describe());
  } finally {
    commandBusy.value = false;
  }
}

</script>

<template>
  <main class="command-workspace">
    <section class="control-plane" aria-labelledby="control-plane-heading">
      <header>
        <div>
          <h1 id="control-plane-heading">Strategy control</h1>
        </div>
        <div class="command-authority" :data-state="authority.state">
          <span aria-hidden="true"></span>
          <div>
            <strong>{{ authorityLabel }}</strong>
            <small>Command channel</small>
          </div>
        </div>
      </header>

      <div class="strategy-register">
        <article v-for="strategy in snapshot.strategies" :key="strategy.id" :data-state="strategy.state">
          <div>
            <span>{{ strategy.state }}</span>
            <strong>{{ strategy.name }}</strong>
            <small>{{ strategy.id }} · generation {{ strategy.generation }}</small>
          </div>
          <p>{{ strategy.detail }}</p>
          <button
            :disabled="!canCommand || strategy.state === 'blocked'"
            @click="prepareCommand(
              strategy.state === 'paused' ? 'resume_strategy' : 'pause_strategy',
              strategy.id,
              `${strategy.state === 'paused' ? 'Resume' : 'Pause'} ${strategy.name}`,
              `${strategy.state === 'paused' ? 'RESUME' : 'PAUSE'} ${strategy.id.toUpperCase()}`,
            )"
          >
            {{ strategy.state === "paused" ? "Resume strategy" : "Pause strategy" }}
          </button>
        </article>
      </div>

      <span class="scroll-cue" aria-hidden="true">Scroll stages →</span>
      <div class="stage-scroll" tabindex="0" aria-label="Processing stages. Scroll horizontally for the full path.">
        <ol class="stage-path" :style="{ '--stage-count': snapshot.stages.length }">
          <li v-for="stage in snapshot.stages" :key="stage.id" :data-state="stage.state">
            <div class="stage-node">
              <span>{{ stage.kind }}</span>
              <strong>{{ stage.name }}</strong>
              <small>{{ stage.state }} · {{ stage.lagMs }}ms</small>
              <code>{{ stage.checkpoint }}</code>
              <p>{{ stage.detail }}</p>
            </div>
            <button
              :disabled="!canCommand || !stage.canPauseBefore"
              @click="prepareCommand(
                'pause_before_stage',
                stage.id,
                `Pause before ${stage.name}`,
                `PAUSE BEFORE ${stage.id.toUpperCase()}`,
              )"
            >
              Hold
            </button>
          </li>
        </ol>
      </div>

      <div class="intervention-register">
        <section>
          <header><strong>Open position interventions</strong><span>{{ snapshot.positions.length }}</span></header>
          <button
            v-for="position in snapshot.positions"
            :key="`${position.instrument}:${position.strategy}`"
            :disabled="!canCommand"
            @click="prepareCommand(
              'flatten_position',
              `${position.instrument}:${position.strategy}`,
              `Flatten ${position.instrument} for ${position.strategy}`,
              `FLATTEN ${position.instrument}`,
            )"
          >
            <strong>{{ position.instrument }}</strong><span>{{ position.strategy }}</span><b>Flatten</b>
          </button>
        </section>
        <section>
          <header><strong>Active order interventions</strong><span>{{ snapshot.orders.length }}</span></header>
          <button
            v-for="order in snapshot.orders"
            :key="order.clientOrderId"
            :disabled="!canCommand"
            @click="prepareCommand(
              'cancel_order',
              order.clientOrderId,
              `Cancel ${order.instrument} order`,
              `CANCEL ${order.instrument}`,
            )"
          >
            <strong>{{ order.instrument }}</strong><span>{{ order.strategy }}</span><b>Cancel</b>
          </button>
        </section>
      </div>

      <form v-if="commandDraft" class="command-review" @submit.prevent="issueCommand">
        <div>
          <span>Command review</span>
          <h2>{{ commandDraft.label }}</h2>
          <p>Expected sequence {{ snapshot.sequence.toLocaleString() }}</p>
        </div>
        <label>
          Operational reason
          <textarea v-model="commandReason" minlength="12" required placeholder="State the incident, evidence, or operator intent"></textarea>
        </label>
        <label>
          Type <code>{{ commandDraft.confirmationPhrase }}</code>
          <input v-model="commandConfirmation" required autocomplete="off" spellcheck="false" />
        </label>
        <p v-if="commandFailure" class="command-failure" role="alert">{{ commandFailure }}</p>
        <div class="command-review-actions">
          <button type="button" @click="dismissCommand">Cancel review</button>
          <button
            type="submit"
            :disabled="commandBusy || commandReason.trim().length < 12 || commandConfirmation !== commandDraft.confirmationPhrase"
          >
            {{ commandBusy ? "Submitting" : "Issue command" }}
          </button>
        </div>
      </form>

      <div v-if="commandReceipt" class="command-receipt" :data-status="commandReceipt.status" role="status">
        <strong>{{ commandReceipt.status }} · {{ commandReceipt.action.replaceAll("_", " ") }}</strong>
        <span>{{ commandReceipt.message }}</span>
        <code>{{ commandReceipt.commandId }} · seq {{ commandReceipt.expectedSequence }}</code>
      </div>

      <footer class="emergency-strip">
        <div>
          <strong>Emergency stop</strong>
          <span>Stops new order admission. Positions remain open.</span>
        </div>
        <button
          :disabled="!canEmergencyCommand || snapshot.risk.killSwitchActive"
          @click="prepareCommand(
            'activate_kill_switch',
            'system',
            'Activate the global kill switch',
            'ACTIVATE KILL SWITCH',
          )"
        >
          {{ snapshot.risk.killSwitchActive ? "Kill switch active" : "Activate kill switch" }}
        </button>
      </footer>
    </section>
  </main>
</template>

<style scoped>
.command-workspace { max-width: 1920px; min-height: calc(100vh - 70px); margin: 0 auto; color: var(--atlas-ink); background: var(--atlas-ground); }
.command-workspace * { box-sizing: border-box; }
button:focus-visible,
.stage-scroll:focus-visible,
input:focus-visible,
textarea:focus-visible { outline: 2px solid var(--atlas-oxide); outline-offset: -2px; }
.control-plane { border-right: 1px solid var(--atlas-rule); border-bottom: 1px solid var(--atlas-rule); border-left: 1px solid var(--atlas-rule); }
.control-plane > header { display: flex; justify-content: space-between; gap: 24px; align-items: center; min-height: 72px; padding: 14px 18px; border-bottom: 1px solid var(--atlas-rule); }
h1 { margin: 0; font-size: 20px; line-height: 1.2; letter-spacing: -.02em; }
.command-authority { display: flex; gap: 9px; align-items: center; min-width: 230px; padding: 8px 10px; border: 1px solid var(--atlas-rule); }
.command-authority > span { width: 7px; height: 7px; flex: 0 0 auto; border-radius: 50%; background: var(--atlas-axis); }
.command-authority[data-state="authenticated"] > span { background: var(--atlas-green); }
.command-authority[data-state="expired"] > span { background: var(--atlas-oxide); }
.command-authority div { display: grid; gap: 2px; min-width: 0; }
.command-authority strong { font: 9px var(--vp-font-family-mono); letter-spacing: .04em; text-transform: uppercase; }
.command-authority small { overflow: hidden; max-width: 320px; color: var(--atlas-muted); font-size: 10px; text-overflow: ellipsis; white-space: nowrap; }
.strategy-register { display: grid; grid-template-columns: repeat(auto-fit, minmax(280px, 1fr)); border-bottom: 1px solid var(--atlas-rule); }
.strategy-register article { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 8px 14px; min-width: 0; padding: 14px 16px; border-right: 1px solid var(--atlas-rule); }
.strategy-register article:last-child { border-right: 0; }
.strategy-register article > div { display: grid; grid-template-columns: auto minmax(0, 1fr); gap: 3px 9px; align-items: baseline; min-width: 0; }
.strategy-register article > div > span { color: var(--atlas-green-ink); font: 8px var(--vp-font-family-mono); letter-spacing: .04em; text-transform: uppercase; }
.strategy-register article[data-state="blocked"] > div > span { color: var(--atlas-oxide); }
.strategy-register article[data-state="paused"] > div > span { color: var(--atlas-blue); }
.strategy-register article strong { overflow: hidden; font-size: 13px; text-overflow: ellipsis; white-space: nowrap; }
.strategy-register article small { grid-column: 1 / -1; color: var(--atlas-axis); font: 8px var(--vp-font-family-mono); }
.strategy-register article p { grid-column: 1 / -1; margin: 0; color: var(--atlas-muted); font-size: 10px; }
button { color: var(--atlas-blue); font: 8px var(--vp-font-family-mono); letter-spacing: .03em; text-transform: uppercase; border: 1px solid var(--atlas-rule); border-radius: 0; background: transparent; cursor: pointer; }
button:hover { color: var(--atlas-ink); border-color: var(--atlas-blue); background: var(--atlas-blue-soft); }
button:disabled { color: var(--atlas-axis); border-color: var(--atlas-rule-soft); background: transparent; cursor: not-allowed; }
.strategy-register button { align-self: start; padding: 6px 8px; }
.scroll-cue { display: none; }
.stage-scroll { overflow-x: auto; scrollbar-color: var(--atlas-blue) var(--atlas-blue-soft); }
.stage-path { display: grid; grid-auto-flow: column; grid-auto-columns: minmax(190px, 1fr); min-width: max(100%, calc(var(--stage-count, 1) * 190px)); margin: 0; padding: 0; list-style: none; }
.stage-path li { position: relative; display: grid; grid-template-rows: 1fr auto; min-width: 0; border-right: 1px solid var(--atlas-rule); }
.stage-path li:last-child { border-right: 0; }
.stage-path li:not(:last-child)::after { position: absolute; z-index: 2; top: 36px; right: -5px; width: 9px; height: 9px; border: 1px solid var(--atlas-blue); background: var(--atlas-ground); content: ""; transform: rotate(45deg); }
.stage-node { display: grid; align-content: start; min-height: 140px; padding: 13px 15px; }
.stage-node > span { color: var(--atlas-blue); font: 8px var(--vp-font-family-mono); letter-spacing: .05em; text-transform: uppercase; }
.stage-path li[data-state="blocked"] .stage-node > span,
.stage-path li[data-state="blocked"] .stage-node > small { color: var(--atlas-oxide); }
.stage-path li[data-state="paused"] .stage-node > span,
.stage-path li[data-state="paused"] .stage-node > small { color: var(--atlas-axis); }
.stage-node strong { margin-top: 7px; font-size: 13px; }
.stage-node small { margin-top: 3px; color: var(--atlas-green-ink); font: 8px var(--vp-font-family-mono); text-transform: uppercase; }
.stage-node code { margin-top: 10px; color: var(--atlas-blue); font: 9px var(--vp-font-family-mono); }
.stage-node p { margin: 5px 0 0; color: var(--atlas-muted); font-size: 10px; line-height: 1.35; }
.stage-path button { width: 100%; min-height: 32px; border-right: 0; border-bottom: 0; border-left: 0; }
.intervention-register { display: grid; grid-template-columns: 1fr 1fr; border-top: 1px solid var(--atlas-rule); }
.intervention-register > section:first-child { border-right: 1px solid var(--atlas-rule); }
.intervention-register section > header { display: flex; justify-content: space-between; padding: 10px 13px; color: var(--atlas-muted); font: 9px var(--vp-font-family-mono); text-transform: uppercase; border-bottom: 1px solid var(--atlas-rule); }
.intervention-register section > button { display: grid; grid-template-columns: 70px 1fr auto; gap: 9px; align-items: center; width: 100%; min-height: 38px; padding: 0 12px; text-align: left; border: 0; border-bottom: 1px solid var(--atlas-rule-soft); }
.intervention-register button strong { color: var(--atlas-ink); }
.intervention-register button span { overflow: hidden; color: var(--atlas-muted); text-overflow: ellipsis; white-space: nowrap; }
.intervention-register button b { color: var(--atlas-oxide); font-weight: 500; }
.command-review { display: grid; grid-template-columns: 1.2fr 1fr 1fr; gap: 14px 18px; padding: 16px 18px; border-top: 1px solid var(--atlas-oxide); border-bottom: 1px solid var(--atlas-rule); background: color-mix(in srgb, var(--atlas-oxide) 5%, var(--atlas-ground)); }
.command-review > div:first-child > span { color: var(--atlas-oxide); font: 8px var(--vp-font-family-mono); letter-spacing: .06em; text-transform: uppercase; }
.command-review h2 { margin: 5px 0 0; font-size: 16px; letter-spacing: -.015em; }
.command-review p { margin: 5px 0 0; color: var(--atlas-muted); font-size: 10px; line-height: 1.4; }
.command-review label { display: grid; gap: 6px; align-content: start; color: var(--atlas-muted); font: 8px var(--vp-font-family-mono); letter-spacing: .03em; text-transform: uppercase; }
.command-review label code { color: var(--atlas-oxide); }
.command-review input,
.command-review textarea { width: 100%; min-height: 34px; padding: 8px 9px; color: var(--atlas-ink); font: 10px var(--vp-font-family-mono); text-transform: none; border: 1px solid var(--atlas-rule); border-radius: 0; background: #05090d; resize: vertical; }
.command-review textarea { min-height: 58px; }
.command-failure { grid-column: 1 / -1; color: var(--atlas-oxide) !important; }
.command-review-actions { grid-column: 1 / -1; display: flex; justify-content: flex-end; gap: 8px; }
.command-review-actions button { padding: 8px 10px; color: var(--atlas-muted); }
.command-review-actions button[type="submit"] { color: #05090d; border-color: var(--atlas-oxide); background: var(--atlas-oxide); }
.command-review-actions button:disabled { opacity: .45; }
.command-receipt { display: grid; grid-template-columns: auto 1fr auto; gap: 12px; align-items: center; padding: 10px 16px; color: var(--atlas-muted); border-bottom: 1px solid var(--atlas-rule); }
.command-receipt strong { color: var(--atlas-green-ink); font: 9px var(--vp-font-family-mono); text-transform: uppercase; }
.command-receipt[data-status="rejected"] strong { color: var(--atlas-oxide); }
.command-receipt span { font-size: 10px; }
.command-receipt code { color: var(--atlas-axis); font-size: 8px; }
.emergency-strip { display: flex; justify-content: space-between; gap: 18px; align-items: center; min-height: 58px; padding: 10px 16px; background: #05090d; }
.emergency-strip div { display: grid; gap: 2px; }
.emergency-strip strong { font-size: 11px; }
.emergency-strip span { color: var(--atlas-muted); font-size: 10px; }
.emergency-strip button { padding: 6px 8px; color: var(--atlas-oxide); border-color: var(--atlas-oxide); }
.emergency-strip button:not(:disabled):hover { color: #05090d; background: var(--atlas-oxide); }
@media (max-width: 820px) {
  .control-plane > header { align-items: stretch; flex-direction: column; }
  .command-authority { min-width: 0; }
  .strategy-register { grid-template-columns: 1fr; }
  .strategy-register article { border-right: 0; border-bottom: 1px solid var(--atlas-rule); }
  .command-review { grid-template-columns: 1fr; }
  .command-review-actions,
  .command-failure { grid-column: 1; }
}
@media (max-width: 520px) {
  .control-plane > header { padding: 14px 12px; }
  .strategy-register article { grid-template-columns: 1fr; padding: 12px; }
  .strategy-register button { width: 100%; }
  .scroll-cue { display: block; padding: 5px 12px; color: var(--atlas-blue); font: 8px var(--vp-font-family-mono); text-align: right; text-transform: uppercase; border-bottom: 1px solid var(--atlas-rule-soft); }
  .intervention-register { grid-template-columns: 1fr; }
  .intervention-register > section:first-child { border-right: 0; border-bottom: 1px solid var(--atlas-rule); }
  .command-review { padding: 13px 12px; }
  .command-review-actions { display: grid; grid-template-columns: 1fr; }
  .command-receipt { grid-template-columns: 1fr; }
  .emergency-strip { align-items: stretch; flex-direction: column; }
  .emergency-strip button { width: 100%; }
}
</style>
