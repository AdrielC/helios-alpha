import type { Component } from "vue";
import type { SectionKind } from "../../operations/research-views";
import ResearchMetrics from "./ResearchMetrics.vue";
import ResearchTable from "./ResearchTable.vue";
import ResearchNotes from "./ResearchNotes.vue";

// The finite renderer vocabulary is code. Book/domain registrations are data.
// No manifest can import a component, run a script, or grant an order action.
export const researchRenderers: Record<SectionKind, Component> = {
  metrics: ResearchMetrics,
  table: ResearchTable,
  notes: ResearchNotes,
};
