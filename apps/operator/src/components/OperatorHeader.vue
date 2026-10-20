<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import type { OperatorProfile } from "../operations/operator-profile";
import type { CommandAuthority } from "../operations/command-port";
import type {
  AlertView,
  FeedMode,
  OperationsContext,
} from "../operations/operations-port";

type WorkspaceView = "operations" | "alerts" | "control" | "explorer";
type ConnectionState = "connecting" | "streaming" | "reconnecting" | "snapshot" | "paused" | "error";
type ShellMenu = "context" | "alerts" | "operator";

const props = defineProps<{
  profile: OperatorProfile;
  brokerAccount?: string;
  activeAlerts: readonly AlertView[];
  commandAuthority: CommandAuthority;
  commandLabel: string;
  connection: ConnectionState;
  connectionLabel: string;
  context: OperationsContext;
  dataClass: "synthetic" | "observed";
  hasSnapshot: boolean;
  mode: FeedMode;
  observedAt: string;
}>();

const emit = defineEmits<{
  actAlert: [alert: AlertView];
  openAlert: [alert: AlertView];
  selectView: [view: WorkspaceView];
}>();

const header = ref<HTMLElement>();
const menu = ref<ShellMenu>();
const viewerName = computed(() => props.profile.display_name);
const viewerRole = computed(() => props.profile.role === "owner" ? "Owner" : "Operator");
const viewerInitials = computed(() => viewerName.value.split(/\s+/).slice(0, 2).map((part) => part[0]?.toUpperCase()).join("") || "GO");
const criticalCount = computed(() => props.activeAlerts.filter((alert) => alert.severity === "critical").length);

function toggleMenu(next: ShellMenu): void {
  menu.value = menu.value === next ? undefined : next;
}

function closeMenu(): void {
  menu.value = undefined;
}

function selectView(next: WorkspaceView): void {
  emit("selectView", next);
  closeMenu();
}

function openAlert(alert: AlertView): void {
  emit("openAlert", alert);
  closeMenu();
}

function actOnAlert(alert: AlertView): void {
  emit("actAlert", alert);
  closeMenu();
}

function actionLabel(alert: AlertView): string {
  if (alert.id === "runtime-stale-view") return "Reconnect";
  const kind = alert.relatedEntity?.kind;
  if (kind === "source") return "Open source";
  if (["strategy", "stage", "control", "account"].includes(kind ?? "")) return "Open control";
  if (kind === "signal") return "Open signal";
  if (kind === "order") return "Open order";
  if (kind === "position") return "Open position";
  return "Inspect";
}

function relativeTime(timestamp: string): string {
  const delta = Math.max(0, new Date(props.observedAt).getTime() - new Date(timestamp).getTime());
  if (delta < 60_000) return `${Math.floor(delta / 1_000)}s`;
  if (delta < 3_600_000) return `${Math.floor(delta / 60_000)}m`;
  return `${Math.floor(delta / 3_600_000)}h`;
}

function handleDocumentPointer(event: PointerEvent): void {
  if (header.value && event.target instanceof Node && !header.value.contains(event.target)) closeMenu();
}

function handleDocumentKey(event: KeyboardEvent): void {
  if (event.key === "Escape") closeMenu();
}

onMounted(() => {
  document.addEventListener("pointerdown", handleDocumentPointer);
  document.addEventListener("keydown", handleDocumentKey);
});
onBeforeUnmount(() => {
  document.removeEventListener("pointerdown", handleDocumentPointer);
  document.removeEventListener("keydown", handleDocumentKey);
});
</script>

<template>
  <header ref="header" class="app-header">
    <div class="identity-cluster">
      <button class="product-identity" aria-label="Open operations" @click="selectView('operations')">
        <svg class="helios-mark" viewBox="0 0 28 28" aria-hidden="true">
          <circle cx="14" cy="14" r="3.2" />
          <path class="mark-orbit" d="M3.8 14c0-5.6 4.6-10.2 10.2-10.2S24.2 8.4 24.2 14 19.6 24.2 14 24.2 3.8 19.6 3.8 14Z" />
          <path class="mark-arc" d="M5.8 7.5c4.7 3.1 11.8 4.4 16.7 1.2M5.5 20.1c4-3.7 11.8-5.5 17-2.1" />
          <path d="M14 1.8v3M14 23.2v3M1.8 14h3M23.2 14h3" />
          <circle class="mark-node" cx="22.5" cy="8.7" r="1.15" />
        </svg>
        <span><strong>Helios</strong><small>OMS</small></span>
      </button>

      <div class="context-control">
        <button
          class="context-trigger"
          :aria-expanded="menu === 'context'"
          aria-controls="context-menu"
          :disabled="!hasSnapshot"
          @click="toggleMenu('context')"
        >
          <span><strong>{{ brokerAccount ? "Robinhood portfolio" : hasSnapshot ? context.organizationName : "Connecting" }}</strong><small>{{ brokerAccount ? `${brokerAccount} / Account monitoring` : hasSnapshot ? `${context.workspaceName} / ${context.accountName}` : "Operations context" }}</small></span>
          <svg viewBox="0 0 16 16" aria-hidden="true"><path d="m4.5 6 3.5 3.5L11.5 6" /></svg>
        </button>
        <section v-if="menu === 'context'" id="context-menu" class="shell-menu context-menu" aria-label="Current operations context">
          <header><strong>{{ brokerAccount ? "Robinhood" : context.organizationName }}</strong><span>{{ brokerAccount ? "Broker observations" : `${dataClass} data` }}</span></header><p v-if="brokerAccount" class="broker-context">Live account {{ brokerAccount }}. The strategy engine has separate paper and live execution status below.</p>
          <dl v-if="!brokerAccount">
            <div><dt>Workspace</dt><dd>{{ context.workspaceName }}</dd><small>{{ context.workspaceId }}</small></div>
            <div><dt>Account</dt><dd>{{ context.accountName }}</dd><small>{{ context.accountId }}</small></div>
            <div><dt>Organization</dt><dd>{{ context.organizationId }}</dd></div>
          </dl>
        </section>
      </div>
    </div>

    <div class="session-state">
      <span class="status-chip" :data-mode="hasSnapshot ? mode : 'pending'">{{ brokerAccount ? "Account monitor" : hasSnapshot ? `${mode} engine` : "pending" }}</span>
      <span class="live-state" :data-state="connection"><i aria-hidden="true"></i>{{ connectionLabel }}</span>

      <div class="menu-anchor">
        <button
          class="icon-button alert-button"
          :aria-expanded="menu === 'alerts'"
          :aria-label="`${activeAlerts.length} active alerts`"
          aria-controls="alert-menu"
          @click="toggleMenu('alerts')"
        >
          <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M5.5 8.2a4.5 4.5 0 0 1 9 0c0 4 1.8 4.7 1.8 4.7H3.7s1.8-.7 1.8-4.7ZM8.2 15.2a2 2 0 0 0 3.6 0" /></svg>
          <span v-if="activeAlerts.length" :data-critical="criticalCount > 0">{{ activeAlerts.length }}</span>
        </button>
        <section v-if="menu === 'alerts'" id="alert-menu" class="shell-menu alerts-menu" aria-label="Active alerts">
          <header><strong>Alerts</strong><span>{{ activeAlerts.length }} active</span></header>
          <div class="alert-menu-list">
            <article v-for="alert in activeAlerts.slice(0, 4)" :key="alert.id" :data-severity="alert.severity">
              <button class="alert-copy" @click="openAlert(alert)">
                <i aria-hidden="true"></i><span><strong>{{ alert.title }}</strong><small>{{ alert.category }} / {{ relativeTime(alert.updatedAt) }}</small></span>
              </button>
              <button class="alert-action" @click="actOnAlert(alert)">{{ actionLabel(alert) }}</button>
            </article>
            <p v-if="activeAlerts.length === 0">No active alerts</p>
          </div>
          <button class="menu-footer" @click="selectView('alerts')">View alert center</button>
        </section>
      </div>

      <div class="menu-anchor">
        <button
          class="operator-trigger"
          :aria-label="`${viewerName}, ${viewerRole}`"
          :aria-expanded="menu === 'operator'"
          aria-controls="operator-menu"
          @click="toggleMenu('operator')"
        ><span>{{ viewerInitials }}</span><strong>{{ viewerName }}</strong><svg viewBox="0 0 16 16" aria-hidden="true"><path d="m4.5 6 3.5 3.5L11.5 6" /></svg></button>
        <section v-if="menu === 'operator'" id="operator-menu" class="shell-menu operator-menu" aria-label="Operator session">
          <header><span class="operator-avatar">{{ viewerInitials }}</span><div><strong>{{ viewerName }}</strong><small>{{ viewerRole }}</small></div></header>
          <dl>
            <div><dt>Role</dt><dd>{{ viewerRole }}</dd></div><div><dt>Execution</dt><dd>{{ commandAuthority.state === 'authenticated' ? 'Connected' : 'Not connected' }}</dd></div>
            <div><dt>Account</dt><dd>{{ brokerAccount ?? context.accountName }}</dd></div>
            <div v-if="commandAuthority.operator"><dt>Command identity</dt><dd>{{ commandAuthority.operator }}</dd></div>
          </dl>
        </section>
      </div>
    </div>
  </header>
</template>

<style scoped>
.app-header{z-index:30;position:relative;display:flex;align-items:stretch;justify-content:space-between;min-height:64px;border-bottom:1px solid var(--atlas-rule-soft);background:#10161e;color:var(--atlas-ink)}
button{color:inherit;cursor:pointer}
.identity-cluster,.product-identity,.context-trigger,.session-state,.operator-trigger{display:flex;align-items:center}
.identity-cluster{min-width:0}.product-identity{gap:12px;padding:0 24px;border:0;border-right:1px solid var(--atlas-rule-soft);background:transparent}
.product-identity>span{display:flex;align-items:baseline;gap:7px}.product-identity strong{font-size:21px;font-weight:650;letter-spacing:-.7px}.product-identity small{color:var(--ui-subtle);font:10px var(--vp-font-family-base);letter-spacing:.08em}
.helios-mark{width:30px;height:30px;fill:none;stroke:var(--ui-accent);stroke-width:1.2;stroke-linecap:round;stroke-linejoin:round}.helios-mark>circle:first-child{fill:var(--ui-accent);stroke:none}.mark-orbit{stroke:var(--ui-accent)}.mark-arc{stroke:var(--ui-accent);opacity:.55}.mark-node{fill:var(--ui-accent);stroke:none}
.context-control,.menu-anchor{position:relative}.context-control{min-width:0}.context-trigger{gap:14px;min-height:44px;max-width:280px;margin:0 12px;padding:7px 12px;text-align:left;border:0;border-radius:7px;background:transparent}.context-trigger>span{display:grid;min-width:0;gap:5px}.context-trigger strong,.context-trigger small{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.context-trigger strong{font-size:13px;font-weight:550}.context-trigger small{color:var(--ui-subtle);font-size:11px}.context-trigger svg,.operator-trigger svg{width:14px;flex:0 0 auto;fill:none;stroke:#98a7ba;stroke-width:1.5}.context-trigger:hover,.operator-trigger:hover{background:var(--ui-raised)}.context-trigger:disabled{opacity:.55;cursor:default}
.session-state{gap:20px;padding:0 24px}.status-chip{font-size:10px;font-weight:500;letter-spacing:.07em;text-transform:uppercase;color:var(--ui-subtle);border-right:1px solid var(--ui-border);padding-right:20px;white-space:nowrap}.status-chip[data-mode=live]{color:var(--atlas-green-ink)}.live-state{display:flex;align-items:center;gap:8px;color:#b0bdcb;font-size:11px;white-space:nowrap}.live-state i{width:6px;height:6px;border-radius:50%;background:var(--ui-subtle)}.live-state[data-state=streaming] i{background:var(--atlas-green);box-shadow:0 0 0 4px #67d9b210}.live-state[data-state=reconnecting] i,.live-state[data-state=error] i{background:var(--atlas-oxide)}
.icon-button{position:relative;display:grid;place-items:center;width:38px;height:38px;border:1px solid var(--ui-border);border-radius:8px;background:transparent}.icon-button:hover,.icon-button[aria-expanded=true]{background:var(--ui-raised)}.icon-button svg{width:19px;fill:none;stroke:#a3afbe;stroke-width:1.5;stroke-linecap:round}.alert-button>span{position:absolute;top:-4px;right:-4px;display:grid;place-items:center;min-width:16px;height:16px;padding:0 3px;border-radius:50%;background:var(--ui-accent);color:var(--ui-accent-ink);font-size:9px;font-weight:650;border:2px solid #10161e}.alert-button>span[data-critical=true]{background:#f2ab83}
.operator-trigger{gap:10px;min-height:42px;padding:4px 7px;border:0;border-radius:8px;background:transparent}.operator-trigger>span,.operator-avatar{display:grid;place-items:center;width:32px;height:32px;border-radius:10px;background:#243328;color:#c8e8b5;font-size:11px;font-weight:650;border:1px solid #42563c}.operator-trigger strong{max-width:130px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-size:12px;font-weight:550}
.shell-menu{position:absolute;z-index:40;top:calc(100% + 12px);border:1px solid #364456;border-radius:12px;background:#151e2a;box-shadow:0 16px 45px #0008;overflow:hidden}.shell-menu>header{display:flex;justify-content:space-between;align-items:center;gap:12px;padding:16px 18px;border-bottom:1px solid var(--ui-border)}.shell-menu>header>strong{font-size:14px}.shell-menu>header>span{font-size:11px;color:var(--ui-subtle)}.context-menu{left:12px;width:320px}.broker-context{padding:0 18px 6px;font-size:13px;line-height:1.6;color:var(--ui-muted)}.context-menu dl,.operator-menu dl{margin:0}.context-menu dl>div,.operator-menu dl>div{display:grid;grid-template-columns:88px minmax(0,1fr);gap:5px 10px;padding:12px 18px;border-bottom:1px solid var(--ui-border)}.context-menu dt,.operator-menu dt{grid-row:1/3;color:var(--ui-subtle);font-size:12px}.context-menu dd,.operator-menu dd{overflow:hidden;margin:0;font-size:13px;text-overflow:ellipsis;white-space:nowrap}.context-menu small{font-size:11px;color:var(--ui-subtle)}
.alerts-menu{right:-45px;width:min(400px,calc(100vw - 24px))}.alert-menu-list article{display:grid;grid-template-columns:minmax(0,1fr) auto;border-bottom:1px solid var(--ui-border)}.alert-copy{display:grid;grid-template-columns:7px minmax(0,1fr);gap:10px;padding:15px;text-align:left;border:0;background:transparent}.alert-copy:hover{background:var(--ui-raised)}.alert-copy i{width:6px;height:6px;margin-top:5px;border-radius:50%;background:var(--ui-accent)}.alert-menu-list article[data-severity=critical] .alert-copy i,.alert-menu-list article[data-severity=warning] .alert-copy i{background:var(--atlas-oxide)}.alert-copy>span{display:grid;gap:6px;min-width:0}.alert-copy strong{font-size:12px;font-weight:500;line-height:1.5}.alert-copy small{color:var(--ui-subtle);font-size:10px}.alert-action{padding:0 14px;border:0;border-left:1px solid var(--ui-border);background:transparent;color:var(--ui-accent);font-size:11px}.alert-action:hover{background:var(--ui-raised)}.alert-menu-list>p{margin:0;padding:24px;text-align:center;color:var(--ui-muted)}.menu-footer{width:100%;padding:14px;border:0;background:#192330;color:var(--ui-accent);font-size:12px}.operator-menu{right:0;width:290px}.operator-menu>header{justify-content:flex-start}.operator-menu>header>div{display:grid;gap:5px}.operator-menu>header strong{font-size:14px}.operator-menu>header small{color:var(--ui-muted);font-size:11px}
@media(max-width:1100px){.status-chip{display:none}.session-state{gap:12px;padding:0 18px}.product-identity{padding:0 18px}}
@media(max-width:800px){.context-trigger{max-width:185px;margin:0 5px;padding:7px}.context-trigger small{font-size:10px}.product-identity small{display:none}.product-identity{gap:8px;padding:0 14px}.session-state{gap:8px;padding:0 12px}.operator-trigger strong{display:none}}
@media(max-width:560px){.app-header{min-height:60px}.context-control{display:none}.identity-cluster{flex:1}.product-identity{border:0}.product-identity strong{font-size:19px}.helios-mark{width:26px;height:26px}.session-state{gap:10px;padding:0 14px}.operator-trigger{padding:0}.operator-trigger>svg{display:none}.live-state{font-size:10px}.icon-button{width:34px;height:34px}.alerts-menu{right:-50px}}
@media(max-width:360px){.live-state{display:none}}
</style>
