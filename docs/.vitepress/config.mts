import { defineConfig } from "vitepress";

export default defineConfig({
  lang: "en-US",
  title: "Helios Alpha",
  description:
    "Restartable streaming primitives for event-driven quantitative research.",
  base: "/helios-alpha/",
  cleanUrls: true,
  appearance: false,
  lastUpdated: true,
  head: [
    ["meta", { name: "theme-color", content: "#e8eef1" }],
    ["meta", { property: "og:type", content: "website" }],
    ["meta", { property: "og:title", content: "Helios Alpha" }],
    [
      "meta",
      {
        property: "og:description",
        content:
          "Compose causality-aware, restartable event pipelines in Rust.",
      },
    ],
  ],
  themeConfig: {
    logo: false,
    siteTitle: "Helios Alpha",
    search: { provider: "local" },
    nav: [
      { text: "Atlas", link: "/" },
      { text: "Start", link: "/guide/quickstart" },
      { text: "Concepts", link: "/concepts/scan-algebra" },
      { text: "Research", link: "/research/rare-events" },
      { text: "Operations", link: "/operations/production-readiness" },
      {
        text: "GitHub",
        link: "https://github.com/AdrielC/helios-alpha",
      },
    ],
    sidebar: [
      {
        text: "Start here",
        items: [
          { text: "Quickstart", link: "/guide/quickstart" },
          {
            text: "Compose a strategy",
            link: "/guide/compose-a-strategy",
          },
          {
            text: "Restart a pipeline",
            link: "/guide/restart-a-pipeline",
          },
        ],
      },
      {
        text: "Core concepts",
        items: [
          { text: "Scan algebra", link: "/concepts/scan-algebra" },
          { text: "Event time", link: "/concepts/event-time" },
          {
            text: "Online statistics",
            link: "/concepts/online-statistics",
          },
          { text: "Checkpoints", link: "/concepts/checkpoints" },
        ],
      },
      {
        text: "Research",
        items: [
          { text: "Rare events", link: "/research/rare-events" },
          { text: "Evidence standard", link: "/research/evidence-standard" },
        ],
      },
      {
        text: "Operate",
        items: [
          {
            text: "Production readiness",
            link: "/operations/production-readiness",
          },
          { text: "Crate map", link: "/reference/crates" },
          { text: "Benchmarks", link: "/reference/benchmarks" },
        ],
      },
    ],
    socialLinks: [
      {
        icon: "github",
        link: "https://github.com/AdrielC/helios-alpha",
      },
    ],
    editLink: {
      pattern:
        "https://github.com/AdrielC/helios-alpha/edit/main/docs/:path",
      text: "Improve this page",
    },
    footer: {
      message: "Research infrastructure. No claim of profitable alpha.",
      copyright: "Helios Alpha",
    },
  },
  transformHtml(code) {
    const contract = `<!--
THESIS: Strategy composition is inspectable evidence, not generic feature-card documentation.
OWN-WORLD: Cool-white atlas sheets, ink and cobalt rules, oxide event marks, evidence green, ruled plates, registration crosses, flat depth, Archivo and Recursive type.
STORY: A researcher traces one event through ordering, buckets, online statistics, signal logic, and a checkpoint, then opens the guide to reproduce it.
FIRST VIEWPORT: A slim nav and specification band lead to a six-stage rail; an aligned-time plot owns two-thirds below while annotations and restart state occupy the final third; the composition guide is the primary action.
FORM: Annotated Event Atlas, first in the grounded candidate list, seed ed3c4d6d.
FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance
-->`;
    return code.replace("<body>", `<body>\n${contract}`);
  },
});
