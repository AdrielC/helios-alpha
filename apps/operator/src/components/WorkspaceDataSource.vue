<script setup lang="ts">
import OperatorIcon from "./OperatorIcon.vue";
import type { OperationsSnapshot } from "../operations/operations-port";
defineProps<{ snapshot: OperationsSnapshot; connection: string; description: string }>();
</script>

<template>
  <section class="workspace-data-source" aria-label="Workspace data source">
    <OperatorIcon name="sources" />
    <div><strong>{{ snapshot.context.accountName }} <span>{{ snapshot.mode }} ledger</span></strong><p>{{ description }}</p></div>
    <details><summary>{{ connection === 'Feed connected' ? 'Event stream connected' : connection }}</summary><p>{{ snapshot.provider }} · Snapshot updated <time :datetime="snapshot.observedAt">{{ new Date(snapshot.observedAt).toLocaleString() }}</time></p></details>
  </section>
</template>

<style scoped>
.workspace-data-source { display: flex; gap: 12px; align-items: flex-start; padding: 16px 28px; border-bottom: 1px solid var(--ui-border); background: #101820; }
.workspace-data-source > svg { color: var(--ui-accent); margin-top: 2px; width: 18px; height: 18px; }
.workspace-data-source > div { flex: 1; min-width: 0; }
strong { font-size: 12px; font-weight: 550; }
strong span { color: var(--ui-subtle); font-weight: 400; margin-left: 8px; text-transform: capitalize; }
p { margin: 6px 0 0; font-size: 11px; color: var(--ui-muted); line-height: 1.6; }
details { max-width: 270px; color: var(--ui-subtle); font-size: 11px; }
summary { cursor: pointer; line-height: 1.7; }
@media (max-width: 720px) { .workspace-data-source { padding: 16px 18px; flex-wrap: wrap; }.workspace-data-source > div { flex-basis: calc(100% - 32px); }details { margin-left: 30px; } }
</style>
