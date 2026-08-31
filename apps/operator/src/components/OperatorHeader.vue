<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
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
  view: WorkspaceView;
}>();

const emit = defineEmits<{
  actAlert: [alert: AlertView];
  freeze: [];
  openAlert: [alert: AlertView];
  selectView: [view: WorkspaceView];
}>();

const header = ref<HTMLElement>();
const menu = ref<ShellMenu>();
const viewerName = computed(() => props.commandAuthority.operator ?? "Guest observer");
const viewerRole = computed(() => props.commandAuthority.state === "authenticated" ? "Command operator" : "Public demo viewer");
const viewerInitials = computed(() => viewerName.value.split(/\s+/).slice(0, 2).map((part) => part[0]?.toUpperCase()).join("") || "GO");
const criticalCount = computed(() => props.activeAlerts.filter((alert) => alert.severity === "critical").length);
const freezeLabel = computed(() => props.connection === "paused" ? "Resume live" : props.connection === "error" ? "Retry" : "Freeze view");
const canFreeze = computed(() => !["connecting", "snapshot", "reconnecting"].includes(props.connection));

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
          <path class="mark-orbit" d="M5.5 18.5C1.8 12.2 6.3 4.2 13.6 4.2c5 0 9.1 4 9.1 9.1 0 7.6-8.7 12-15 7.4" />
          <path d="M14 8.5v2M14 17.5v2M8.5 14h2M17.5 14h2" />
          <rect x="12" y="12" width="4" height="4" />
          <path d="m8.5 8.5 1.4 1.4m8.2 8.2 1.4 1.4m0-11-1.4 1.4m-8.2 8.2-1.4 1.4" />
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
          <span><strong>{{ hasSnapshot ? context.organizationName : "Connecting" }}</strong><small>{{ hasSnapshot ? `${context.workspaceName} / ${context.accountName}` : "Operations context" }}</small></span>
          <svg viewBox="0 0 16 16" aria-hidden="true"><path d="m4.5 6 3.5 3.5L11.5 6" /></svg>
        </button>
        <section v-if="menu === 'context'" id="context-menu" class="shell-menu context-menu" aria-label="Current operations context">
          <header><strong>{{ context.organizationName }}</strong><span>{{ dataClass }} data</span></header>
          <dl>
            <div><dt>Workspace</dt><dd>{{ context.workspaceName }}</dd><small>{{ context.workspaceId }}</small></div>
            <div><dt>Account</dt><dd>{{ context.accountName }}</dd><small>{{ context.accountId }}</small></div>
            <div><dt>Organization</dt><dd>{{ context.organizationId }}</dd></div>
          </dl>
        </section>
      </div>
    </div>

    <nav aria-label="Workspace views">
      <button :aria-current="view === 'operations' ? 'page' : undefined" @click="selectView('operations')">Operations</button>
      <button :aria-current="view === 'control' ? 'page' : undefined" :disabled="!hasSnapshot" @click="selectView('control')">Control</button>
      <button :aria-current="view === 'explorer' ? 'page' : undefined" :disabled="!hasSnapshot" @click="selectView('explorer')">Explore</button>
    </nav>

    <div class="session-state">
      <span class="status-chip" :data-mode="hasSnapshot ? mode : 'pending'">{{ hasSnapshot ? mode : "pending" }}</span>
      <span class="live-state" :data-state="connection"><i aria-hidden="true"></i>{{ connectionLabel }}</span>
      <button
        class="freeze-button"
        :disabled="!canFreeze"
        :title="connection === 'paused' ? 'Resume live updates in this browser' : connection === 'error' ? 'Retry the operations connection' : 'Freeze updates in this browser'"
        @click="emit('freeze')"
      >{{ freezeLabel }}</button>

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
          :aria-expanded="menu === 'operator'"
          aria-controls="operator-menu"
          @click="toggleMenu('operator')"
        ><span>{{ viewerInitials }}</span><strong>{{ viewerName }}</strong><svg viewBox="0 0 16 16" aria-hidden="true"><path d="m4.5 6 3.5 3.5L11.5 6" /></svg></button>
        <section v-if="menu === 'operator'" id="operator-menu" class="shell-menu operator-menu" aria-label="Operator session">
          <header><span class="operator-avatar">{{ viewerInitials }}</span><div><strong>{{ viewerName }}</strong><small>{{ viewerRole }}</small></div></header>
          <dl>
            <div><dt>Access</dt><dd>{{ commandLabel }}</dd></div>
            <div><dt>Account</dt><dd>{{ context.accountName }}</dd></div>
            <div><dt>Audit identity</dt><dd>{{ commandAuthority.operator ?? "Anonymous" }}</dd></div>
          </dl>
          <p v-if="commandAuthority.state !== 'authenticated'">Shared guest session. Commands stay unavailable.</p>
        </section>
      </div>
    </div>
  </header>
</template>

<style scoped>
.app-header{position:sticky;z-index:30;top:0;display:grid;grid-template-columns:minmax(330px,1fr) auto minmax(360px,1fr);min-height:64px;border-bottom:1px solid var(--atlas-rule);background:color-mix(in srgb,var(--atlas-ground) 96%,transparent);backdrop-filter:blur(12px)}button{color:inherit}.identity-cluster,.product-identity,.context-trigger,.session-state,.app-header nav,.operator-trigger{display:flex;align-items:center}.identity-cluster{min-width:0}.product-identity{gap:9px;align-self:stretch;padding:0 15px;border:0;border-right:1px solid var(--atlas-rule-soft);background:transparent;cursor:pointer}.product-identity>span{display:grid;grid-template-columns:auto auto;gap:5px;align-items:baseline}.product-identity strong{font-size:16px;letter-spacing:-.02em}.product-identity small{color:var(--atlas-axis);font:8px var(--vp-font-family-mono);letter-spacing:.06em}.helios-mark{width:27px;height:27px;overflow:visible;fill:none;stroke:var(--atlas-green);stroke-width:1.25;stroke-linecap:square;stroke-linejoin:miter}.helios-mark rect{fill:var(--atlas-green);stroke:none}.mark-orbit{stroke:var(--atlas-blue);transform-origin:center;animation:orbit-mark 12s linear infinite}.context-control,.menu-anchor{position:relative}.context-control{min-width:0}.context-trigger{gap:9px;min-width:0;max-width:265px;padding:0 13px;text-align:left;border:0;background:transparent;cursor:pointer}.context-trigger>span{display:grid;min-width:0;gap:3px}.context-trigger strong,.context-trigger small{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.context-trigger strong{font-size:11px;font-weight:620}.context-trigger small{color:var(--atlas-muted);font:8px var(--vp-font-family-mono)}.context-trigger svg,.operator-trigger svg{width:14px;flex:0 0 auto;fill:none;stroke:var(--atlas-axis);stroke-width:1.4}.context-trigger:hover,.operator-trigger:hover{background:var(--atlas-surface-alt)}.context-trigger:disabled{opacity:.55;cursor:not-allowed}
.app-header nav{align-self:stretch}.app-header nav button{position:relative;height:100%;min-width:78px;padding:0 14px;color:var(--atlas-muted);font:600 10px var(--vp-font-family-mono);letter-spacing:.02em;text-transform:uppercase;border:0;background:transparent;cursor:pointer}.app-header nav button:hover{color:var(--atlas-ink);background:var(--atlas-surface-alt)}.app-header nav button[aria-current=page]{color:var(--atlas-blue);background:var(--atlas-surface-strong)}.app-header nav button[aria-current=page]::after{position:absolute;right:0;bottom:0;left:0;height:2px;content:"";background:var(--atlas-blue)}.app-header nav button:disabled{opacity:.42;cursor:not-allowed}
.session-state{justify-content:flex-end;gap:7px;padding:0 14px}.status-chip,.live-state{color:var(--atlas-muted);font:9px var(--vp-font-family-mono);letter-spacing:.035em;text-transform:uppercase;white-space:nowrap}.status-chip{padding:5px 7px;border:1px solid var(--atlas-rule)}.status-chip[data-mode=live]{color:var(--atlas-green-ink);border-color:color-mix(in srgb,var(--atlas-green) 42%,var(--atlas-rule))}.live-state{display:flex;gap:6px;align-items:center}.live-state i{width:7px;height:7px;border-radius:50%;background:var(--atlas-axis)}.live-state[data-state=streaming] i{background:var(--atlas-green);animation:live-pulse 2.4s cubic-bezier(.16,1,.3,1) infinite}.live-state[data-state=reconnecting] i,.live-state[data-state=error] i{background:var(--atlas-oxide)}.freeze-button{min-height:31px;padding:0 9px;color:var(--atlas-blue);font:9px var(--vp-font-family-mono);text-transform:uppercase;border:1px solid var(--atlas-rule);background:transparent;cursor:pointer}.freeze-button:hover{color:var(--atlas-ink);border-color:var(--atlas-blue);background:var(--atlas-blue-soft)}.freeze-button:disabled{color:var(--atlas-axis);cursor:not-allowed}.icon-button{position:relative;display:grid;width:35px;height:35px;place-items:center;border:1px solid transparent;background:transparent;cursor:pointer}.icon-button:hover,.icon-button[aria-expanded=true]{border-color:var(--atlas-rule);background:var(--atlas-surface-alt)}.icon-button svg{width:19px;fill:none;stroke:var(--atlas-muted);stroke-width:1.4;stroke-linecap:square}.alert-button>span{position:absolute;top:1px;right:0;display:grid;min-width:15px;height:15px;padding:0 3px;place-items:center;color:var(--operator-black);font:700 7px var(--vp-font-family-mono);border-radius:50%;background:var(--atlas-blue)}.alert-button>span[data-critical=true]{background:var(--atlas-oxide)}.operator-trigger{gap:7px;min-height:35px;padding:0 4px 0 2px;border:1px solid transparent;background:transparent;cursor:pointer}.operator-trigger>span,.operator-avatar{display:grid;width:27px;height:27px;place-items:center;color:var(--atlas-green-ink);font:700 8px var(--vp-font-family-mono);border:1px solid color-mix(in srgb,var(--atlas-green) 45%,var(--atlas-rule));border-radius:50%;background:var(--atlas-surface-strong)}.operator-trigger strong{max-width:96px;overflow:hidden;font-size:10px;text-overflow:ellipsis;white-space:nowrap}
.shell-menu{position:absolute;z-index:40;top:calc(100% + 14px);border:1px solid var(--atlas-rule);background:var(--operator-black)}.shell-menu::before{position:absolute;top:-15px;right:0;left:0;height:14px;content:""}.shell-menu>header{display:flex;justify-content:space-between;gap:12px;align-items:center;min-height:44px;padding:0 13px;border-bottom:1px solid var(--atlas-rule)}.shell-menu>header>strong{font-size:11px}.shell-menu>header>span{color:var(--atlas-axis);font:8px var(--vp-font-family-mono);text-transform:uppercase}.context-menu{left:0;width:320px}.context-menu dl,.operator-menu dl{margin:0}.context-menu dl>div,.operator-menu dl>div{display:grid;grid-template-columns:88px minmax(0,1fr);gap:4px 10px;padding:10px 13px;border-bottom:1px solid var(--atlas-rule-soft)}.context-menu dt,.operator-menu dt{grid-row:1/3;color:var(--atlas-axis);font:8px var(--vp-font-family-mono);text-transform:uppercase}.context-menu dd,.operator-menu dd{overflow:hidden;margin:0;color:var(--atlas-ink);font:9px var(--vp-font-family-mono);text-overflow:ellipsis;white-space:nowrap}.context-menu small{color:var(--atlas-axis);font:8px var(--vp-font-family-mono)}.alerts-menu{right:-48px;width:min(390px,calc(100vw - 24px))}.alert-menu-list article{display:grid;grid-template-columns:minmax(0,1fr) auto;border-bottom:1px solid var(--atlas-rule-soft)}.alert-copy{display:grid;grid-template-columns:8px minmax(0,1fr);gap:9px;min-height:63px;align-items:start;padding:12px;text-align:left;border:0;background:transparent;cursor:pointer}.alert-copy:hover{background:var(--atlas-surface-strong)}.alert-copy i{width:7px;height:7px;margin-top:3px;border-radius:50%;background:var(--atlas-blue)}.alert-menu-list article[data-severity=critical] .alert-copy i,.alert-menu-list article[data-severity=warning] .alert-copy i{background:var(--atlas-oxide)}.alert-copy>span{display:grid;gap:5px;min-width:0}.alert-copy strong{overflow:hidden;font-size:10px;text-overflow:ellipsis;white-space:nowrap}.alert-copy small{color:var(--atlas-axis);font:8px var(--vp-font-family-mono);text-transform:uppercase}.alert-action{align-self:stretch;min-width:84px;padding:0 10px;color:var(--atlas-blue);font:8px var(--vp-font-family-mono);text-transform:uppercase;border:0;border-left:1px solid var(--atlas-rule-soft);background:transparent;cursor:pointer}.alert-action:hover{color:var(--atlas-ink);background:var(--atlas-blue-soft)}.alert-menu-list>p{display:grid;min-height:80px;place-items:center;margin:0;color:var(--atlas-axis);font:9px var(--vp-font-family-mono);text-transform:uppercase}.menu-footer{width:100%;min-height:38px;color:var(--atlas-blue);font:8px var(--vp-font-family-mono);text-transform:uppercase;border:0;background:var(--atlas-surface-alt);cursor:pointer}.menu-footer:hover{color:var(--atlas-ink)}.operator-menu{right:0;width:290px}.operator-menu>header{justify-content:flex-start;min-height:62px}.operator-menu>header>div{display:grid;gap:4px}.operator-menu>header strong{font-size:11px}.operator-menu>header small{color:var(--atlas-muted);font:8px var(--vp-font-family-mono)}.operator-menu>p{margin:0;padding:11px 13px;color:var(--atlas-muted);font-size:10px;line-height:1.4}
button:focus-visible{outline:2px solid var(--atlas-blue);outline-offset:-2px}@keyframes orbit-mark{to{transform:rotate(360deg)}}@keyframes live-pulse{0%,100%{box-shadow:0 0 0 0 color-mix(in srgb,var(--atlas-green) 0%,transparent)}40%{box-shadow:0 0 0 5px color-mix(in srgb,var(--atlas-green) 13%,transparent)}}
@media(max-width:1180px){.app-header{grid-template-columns:minmax(0,1fr) auto}.app-header nav{order:3;grid-column:1/-1;height:40px;border-top:1px solid var(--atlas-rule)}.app-header nav button{flex:1}.session-state{min-height:64px}.status-chip{display:none}.context-trigger{max-width:230px}}
@media(max-width:720px){.app-header{position:static;display:flex;flex-wrap:wrap}.identity-cluster{width:100%;min-height:56px;border-bottom:1px solid var(--atlas-rule)}.product-identity{padding:0 12px}.context-control{flex:1}.context-trigger{max-width:calc(100vw - 108px)}.session-state{order:2;display:grid;grid-template-columns:1fr auto auto auto;width:100%;min-height:auto;padding:8px 12px}.operator-trigger strong,.operator-trigger>svg{display:none}.app-header nav{order:3;width:100%;overflow-x:auto}.app-header nav button{min-width:92px}.context-menu{left:auto;right:0;width:min(320px,calc(100vw - 24px))}.alerts-menu{right:-45px}.freeze-button{min-width:94px}}
@media(max-width:440px){.live-state{font-size:0}.live-state i{width:8px;height:8px}.freeze-button{min-width:78px;padding:0 6px}.session-state{gap:4px}.alerts-menu{right:-42px}.alert-action{min-width:75px}.context-trigger small{max-width:180px}}
@media(prefers-reduced-motion:reduce){.mark-orbit,.live-state[data-state=streaming] i{animation:none}}
</style>
