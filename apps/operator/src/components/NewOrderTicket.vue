<script setup lang="ts">
import { computed, ref, watch } from "vue";
import type {
  CommandAuthority,
  CommandPort,
  CommandReceipt,
  OrderRequest,
} from "../operations/command-port";
import type { OperationsSnapshot } from "../operations/operations-port";

const props = defineProps<{
  snapshot: OperationsSnapshot;
  stale: boolean;
  authority: CommandAuthority;
  port: CommandPort;
  initialInstrument?: string;
}>();
const emit = defineEmits<{ authority: [authority: CommandAuthority] }>();

type TicketPhase = "edit" | "review" | "receipt";
type TimeInForce = OrderRequest["timeInForce"];

const phase = ref<TicketPhase>("edit");
const instrument = ref(props.initialInstrument ?? props.snapshot.positions[0]?.instrument ?? props.snapshot.signals[0]?.instrument ?? "");
const side = ref<OrderRequest["side"]>("buy");
const orderType = ref<OrderRequest["orderType"]>("limit");
const quantity = ref("");
const limitPrice = ref("");
const timeInForce = ref<TimeInForce>("day");
const strategyId = ref("");
const reason = ref("");
const confirmation = ref("");
const failure = ref("");
const busy = ref(false);
const receipt = ref<CommandReceipt>();

const instruments = computed(() => Array.from(new Set([
  ...props.snapshot.positions.map((position) => position.instrument),
  ...props.snapshot.signals.map((signal) => signal.instrument),
])).sort());
const normalizedInstrument = computed(() => instrument.value.trim().toUpperCase());
const quantityMicros = computed(() => decimalToMicros(quantity.value));
const priceMicros = computed(() => orderType.value === "market" ? undefined : decimalToMicros(limitPrice.value));
const referencePriceMicros = computed(() => {
  if (priceMicros.value) return priceMicros.value;
  return props.snapshot.positions.find((position) => position.instrument === normalizedInstrument.value)?.markPriceMicros;
});
const estimatedNotionalMicros = computed(() => {
  if (!quantityMicros.value || !referencePriceMicros.value) return undefined;
  return ((BigInt(quantityMicros.value) * BigInt(referencePriceMicros.value)) / 1_000_000n).toString();
});
const confirmationPhrase = computed(() => `SUBMIT ${normalizedInstrument.value}`);
const authorityLabel = computed(() => {
  if (props.authority.state === "authenticated") return props.authority.operator ?? "Command ready";
  if (props.authority.state === "expired") return "Session expired";
  return "Read only";
});
const reviewedOrder = computed<OrderRequest | undefined>(() => {
  if (!quantityMicros.value || (orderType.value === "limit" && !priceMicros.value)) return undefined;
  return {
    instrument: normalizedInstrument.value,
    side: side.value,
    quantityMicros: quantityMicros.value,
    orderType: orderType.value,
    ...(priceMicros.value ? { limitPriceMicros: priceMicros.value } : {}),
    timeInForce: timeInForce.value,
    ...(strategyId.value ? { strategyId: strategyId.value } : {}),
  };
});
const canSubmit = computed(() =>
  props.authority.state === "authenticated" &&
  !props.stale &&
  !busy.value &&
  reason.value.trim().length >= 12 &&
  confirmation.value === confirmationPhrase.value,
);

function decimalToMicros(value: string): string | undefined {
  const match = value.trim().match(/^([0-9]+)(?:\.([0-9]{0,6}))?$/);
  if (!match) return undefined;
  const micros = BigInt(match[1]) * 1_000_000n + BigInt((match[2] ?? "").padEnd(6, "0"));
  return micros > 0n ? micros.toString() : undefined;
}

function formatDecimal(micros: string | undefined, maximumFractionDigits = 6): string {
  if (!micros) return "Market";
  const value = BigInt(micros);
  const whole = value / 1_000_000n;
  const fraction = (value % 1_000_000n).toString().padStart(6, "0").slice(0, maximumFractionDigits).replace(/0+$/, "");
  return `${new Intl.NumberFormat("en-US", { maximumFractionDigits: 0 }).format(whole)}${fraction ? `.${fraction}` : ""}`;
}

function money(micros: string | undefined): string {
  if (!micros) return "Not available";
  const value = BigInt(micros);
  const roundedCents = (value + 5_000n) / 10_000n;
  const dollars = roundedCents / 100n;
  const cents = roundedCents % 100n;
  const whole = new Intl.NumberFormat("en-US", { maximumFractionDigits: 0 }).format(dollars);
  return `$${whole}.${cents.toString().padStart(2, "0")}`;
}

function validateDraft(): string | undefined {
  if (!/^[A-Z0-9][A-Z0-9._/-]{0,31}$/.test(normalizedInstrument.value)) return "Enter a valid instrument.";
  if (!quantityMicros.value) return "Enter a positive quantity with at most six decimal places.";
  if (orderType.value === "limit" && !priceMicros.value) return "Enter a positive limit price with at most six decimal places.";
  return undefined;
}

function review(): void {
  failure.value = validateDraft() ?? "";
  if (failure.value) return;
  instrument.value = normalizedInstrument.value;
  reason.value = "";
  confirmation.value = "";
  receipt.value = undefined;
  phase.value = "review";
}

function edit(): void {
  phase.value = "edit";
  failure.value = "";
}

function reset(): void {
  quantity.value = "";
  reason.value = "";
  confirmation.value = "";
  receipt.value = undefined;
  failure.value = "";
  phase.value = "edit";
}

async function submit(): Promise<void> {
  const order = reviewedOrder.value;
  if (!order || !canSubmit.value) return;
  busy.value = true;
  failure.value = "";
  try {
    receipt.value = await props.port.execute({
      action: "submit_order",
      targetId: props.snapshot.context.accountId,
      reason: reason.value.trim(),
      confirmation: confirmation.value,
      order,
    }, props.snapshot.sequence);
    phase.value = "receipt";
  } catch (error) {
    failure.value = error instanceof Error ? error.message : "Order request failed.";
    emit("authority", await props.port.describe());
  } finally {
    busy.value = false;
  }
}

watch(orderType, (next) => {
  if (next === "market" && timeInForce.value === "good_till_canceled") timeInForce.value = "day";
});
watch(
  () => props.initialInstrument,
  (next) => {
    if (!next) return;
    instrument.value = next;
    reset();
  },
);
</script>

<template>
  <section class="order-ticket" aria-labelledby="new-order-heading">
    <header>
      <div><h2 id="new-order-heading">New order</h2><span :data-state="authority.state">{{ authorityLabel }}</span></div>
      <ol aria-label="Order progress"><li :aria-current="phase === 'edit' ? 'step' : undefined">Draft</li><li :aria-current="phase === 'review' ? 'step' : undefined">Review</li><li :aria-current="phase === 'receipt' ? 'step' : undefined">Receipt</li></ol>
    </header>

    <form v-if="phase === 'edit'" class="ticket-form" @submit.prevent="review">
      <label class="instrument-field"><span>Instrument</span><input v-model="instrument" list="order-instruments" inputmode="text" autocomplete="off" spellcheck="false"><datalist id="order-instruments"><option v-for="item in instruments" :key="item" :value="item" /></datalist></label>
      <fieldset class="side-field"><legend>Side</legend><div><button type="button" :aria-pressed="side === 'buy'" data-side="buy" @click="side = 'buy'">Buy</button><button type="button" :aria-pressed="side === 'sell'" data-side="sell" @click="side = 'sell'">Sell</button></div></fieldset>
      <label><span>Quantity</span><input v-model="quantity" inputmode="decimal" autocomplete="off" placeholder="0.00"></label>
      <label><span>Order type</span><select v-model="orderType"><option value="limit">Limit</option><option value="market">Market</option></select></label>
      <label v-if="orderType === 'limit'"><span>Limit price</span><input v-model="limitPrice" inputmode="decimal" autocomplete="off" placeholder="0.00"></label>
      <label><span>Time in force</span><select v-model="timeInForce"><option value="day">Day</option><option value="good_till_canceled">Good till canceled</option><option value="immediate_or_cancel">Immediate or cancel</option><option value="fill_or_kill">Fill or kill</option></select></label>
      <label class="strategy-field"><span>Strategy attribution</span><select v-model="strategyId"><option value="">Unattributed</option><option v-for="strategy in snapshot.strategies" :key="strategy.id" :value="strategy.id">{{ strategy.name }}</option></select></label>
      <p v-if="failure" class="ticket-error" role="alert">{{ failure }}</p>
      <footer><span>Estimated notional <strong>{{ money(estimatedNotionalMicros) }}</strong></span><button type="submit">Review order</button></footer>
    </form>

    <div v-else-if="phase === 'review' && reviewedOrder" class="ticket-review">
      <div class="order-summary" :data-side="reviewedOrder.side"><span>{{ reviewedOrder.side }}</span><strong>{{ formatDecimal(reviewedOrder.quantityMicros) }} {{ reviewedOrder.instrument }}</strong><b>{{ reviewedOrder.orderType === 'limit' ? `@ ${formatDecimal(reviewedOrder.limitPriceMicros, 4)}` : '@ market' }}</b></div>
      <dl><div><dt>Estimated notional</dt><dd>{{ money(estimatedNotionalMicros) }}</dd></div><div><dt>Time in force</dt><dd>{{ reviewedOrder.timeInForce.replaceAll('_', ' ') }}</dd></div><div><dt>Snapshot</dt><dd>{{ snapshot.sequence.toLocaleString() }}</dd></div><div><dt>Strategy</dt><dd>{{ strategyId || "Unattributed" }}</dd></div></dl>
      <label><span>Operational reason</span><textarea v-model="reason" rows="2" placeholder="Why this order is being submitted"></textarea><small>Minimum 12 characters</small></label>
      <label><span>Type {{ confirmationPhrase }}</span><input v-model="confirmation" autocomplete="off" spellcheck="false"></label>
      <p v-if="authority.state !== 'authenticated'" class="ticket-lock"><svg viewBox="0 0 18 18" aria-hidden="true"><rect x="4.5" y="8" width="9" height="7"/><path d="M6.5 8V5.8a2.5 2.5 0 0 1 5 0V8"/></svg><span>Command service required</span></p>
      <p v-else-if="stale" class="ticket-lock">Refresh the operations stream before submitting.</p>
      <p v-if="failure" class="ticket-error" role="alert">{{ failure }}</p>
      <footer><button type="button" class="secondary" @click="edit">Edit</button><button type="button" :disabled="!canSubmit" @click="submit">{{ busy ? "Submitting" : "Submit order" }}</button></footer>
    </div>

    <div v-else-if="receipt" class="ticket-receipt" aria-live="polite">
      <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m5 12 4 4L19 6" /></svg><h3>{{ receipt.status }}</h3><p>{{ receipt.message }}</p><dl><div><dt>Command</dt><dd>{{ receipt.commandId }}</dd></div><div><dt>Sequence</dt><dd>{{ receipt.expectedSequence.toLocaleString() }}</dd></div></dl><button type="button" @click="reset">New ticket</button>
    </div>
  </section>
</template>

<style scoped>
.order-ticket{min-width:0;border:1px solid var(--atlas-rule);background:var(--operator-black)}.order-ticket>header{display:flex;min-height:54px;justify-content:space-between;gap:14px;align-items:center;padding:0 14px;border-bottom:1px solid var(--atlas-rule)}.order-ticket>header>div{display:flex;gap:10px;align-items:baseline}.order-ticket h2{margin:0;font-size:16px;letter-spacing:-.015em}.order-ticket header span{color:var(--atlas-axis);font:8px var(--vp-font-family-mono);text-transform:uppercase}.order-ticket header span[data-state=authenticated]{color:var(--atlas-green-ink)}.order-ticket ol{display:flex;margin:0;padding:0;list-style:none}.order-ticket ol li{padding:5px 7px;color:var(--atlas-axis);font:7px var(--vp-font-family-mono);text-transform:uppercase;border-bottom:1px solid transparent}.order-ticket ol li[aria-current=step]{color:var(--atlas-blue);border-color:var(--atlas-blue)}
.ticket-form{display:grid;grid-template-columns:1.3fr .7fr;gap:13px 12px;padding:15px}.ticket-form label,.ticket-review label{display:grid;gap:6px;min-width:0}.ticket-form label>span,.ticket-review label>span,.side-field legend{color:var(--atlas-axis);font:8px var(--vp-font-family-mono);letter-spacing:.035em;text-transform:uppercase}.ticket-form input,.ticket-form select,.ticket-review input,.ticket-review textarea{width:100%;min-height:39px;padding:0 10px;color:var(--atlas-ink);font:10px var(--vp-font-family-mono);border:1px solid var(--atlas-rule);border-radius:0;background:var(--atlas-surface-alt);outline:none}.ticket-review textarea{padding:9px 10px;line-height:1.4;resize:vertical}.ticket-form input:focus,.ticket-form select:focus,.ticket-review input:focus,.ticket-review textarea:focus{border-color:var(--atlas-blue);box-shadow:inset 0 0 0 1px var(--atlas-blue)}.ticket-form input::placeholder,.ticket-review textarea::placeholder{color:var(--atlas-axis)}.side-field{min-width:0;margin:0;padding:0;border:0}.side-field legend{margin-bottom:6px}.side-field>div{display:grid;grid-template-columns:1fr 1fr}.side-field button{min-height:39px;color:var(--atlas-muted);font:9px var(--vp-font-family-mono);text-transform:uppercase;border:1px solid var(--atlas-rule);background:var(--atlas-surface-alt);cursor:pointer}.side-field button+button{border-left:0}.side-field button[data-side=buy][aria-pressed=true]{color:var(--atlas-green-ink);background:color-mix(in srgb,var(--atlas-green) 9%,var(--atlas-surface-alt))}.side-field button[data-side=sell][aria-pressed=true]{color:var(--atlas-oxide);background:color-mix(in srgb,var(--atlas-oxide) 9%,var(--atlas-surface-alt))}.strategy-field{grid-column:1/-1}.ticket-form footer,.ticket-review footer{display:flex;grid-column:1/-1;justify-content:space-between;gap:12px;align-items:center;padding-top:3px}.ticket-form footer>span{color:var(--atlas-axis);font:8px var(--vp-font-family-mono);text-transform:uppercase}.ticket-form footer strong{margin-left:6px;color:var(--atlas-ink);font-size:10px}.ticket-form footer button,.ticket-review footer button,.ticket-receipt button{min-height:38px;padding:0 13px;color:var(--operator-black);font:650 8px var(--vp-font-family-mono);text-transform:uppercase;border:1px solid var(--atlas-blue);background:var(--atlas-blue);cursor:pointer}.ticket-form footer button:hover,.ticket-review footer button:hover,.ticket-receipt button:hover{background:var(--atlas-green);border-color:var(--atlas-green)}
.ticket-review{display:grid;gap:14px;padding:15px}.order-summary{display:grid;grid-template-columns:auto 1fr auto;gap:9px;align-items:baseline;padding:12px;border:1px solid var(--atlas-rule);background:var(--atlas-surface-alt)}.order-summary span{color:var(--atlas-green-ink);font:8px var(--vp-font-family-mono);text-transform:uppercase}.order-summary[data-side=sell] span{color:var(--atlas-oxide)}.order-summary strong{font:12px var(--vp-font-family-mono)}.order-summary b{color:var(--atlas-muted);font:10px var(--vp-font-family-mono)}.ticket-review dl{display:grid;grid-template-columns:1fr 1fr;margin:0;border-top:1px solid var(--atlas-rule);border-left:1px solid var(--atlas-rule)}.ticket-review dl>div{padding:9px;border-right:1px solid var(--atlas-rule);border-bottom:1px solid var(--atlas-rule)}.ticket-review dt{color:var(--atlas-axis);font:7px var(--vp-font-family-mono);text-transform:uppercase}.ticket-review dd{overflow:hidden;margin:5px 0 0;color:var(--atlas-ink);font:9px var(--vp-font-family-mono);text-overflow:ellipsis;white-space:nowrap}.ticket-review label small{color:var(--atlas-axis);font-size:8px}.ticket-lock{display:flex;gap:8px;align-items:center;margin:0;color:var(--atlas-muted);font:8px var(--vp-font-family-mono);text-transform:uppercase}.ticket-lock svg{width:16px;fill:none;stroke:var(--atlas-axis);stroke-width:1.2}.ticket-review footer{padding-top:0}.ticket-review footer .secondary{color:var(--atlas-muted);border-color:var(--atlas-rule);background:transparent}.ticket-review footer button:disabled{color:var(--atlas-axis);border-color:var(--atlas-rule);background:var(--atlas-surface-alt);cursor:not-allowed}.ticket-error{grid-column:1/-1;margin:0;color:var(--atlas-oxide);font-size:10px}
.ticket-receipt{display:grid;min-height:330px;place-content:center;justify-items:center;padding:30px;text-align:center}.ticket-receipt>svg{width:34px;fill:none;stroke:var(--atlas-green);stroke-width:1.5}.ticket-receipt h3{margin:12px 0 0;font:18px var(--vp-font-family-mono);text-transform:capitalize}.ticket-receipt p{max-width:42ch;margin:9px 0 0;color:var(--atlas-muted);font-size:11px}.ticket-receipt dl{display:grid;gap:7px;width:min(100%,340px);margin:20px 0}.ticket-receipt dl>div{display:grid;grid-template-columns:80px minmax(0,1fr);gap:10px;text-align:left}.ticket-receipt dt{color:var(--atlas-axis);font:8px var(--vp-font-family-mono);text-transform:uppercase}.ticket-receipt dd{overflow:hidden;margin:0;color:var(--atlas-ink);font:8px var(--vp-font-family-mono);text-overflow:ellipsis;white-space:nowrap}
button:focus-visible,input:focus-visible,select:focus-visible,textarea:focus-visible{outline:2px solid var(--atlas-blue);outline-offset:-2px}@media(max-width:520px){.order-ticket>header{align-items:flex-start;flex-direction:column;padding:12px}.ticket-form{grid-template-columns:1fr}.strategy-field{grid-column:auto}.ticket-form footer{align-items:flex-start;flex-direction:column}.ticket-form footer button{width:100%;min-height:44px}.ticket-review dl{grid-template-columns:1fr}.ticket-review footer button{min-height:44px}.order-summary{grid-template-columns:auto 1fr}.order-summary b{grid-column:2}}@media(prefers-reduced-motion:reduce){*{scroll-behavior:auto!important}}
</style>
