import { defineConfig } from "vitepress";

export default defineConfig({
  lang: "en-US",
  title: "Helios Alpha",
  description:
    "Restartable streaming primitives for event-driven quantitative research.",
  base: "/helios-alpha/",
  cleanUrls: true,
  appearance: "dark",
  lastUpdated: true,
  head: [
    ["meta", { name: "theme-color", content: "#08111f" }],
    ["meta", { property: "og:type", content: "website" }],
    ["meta", { property: "og:title", content: "Helios Alpha" }],
    [
      "meta",
      {
        property: "og:description",
        content:
          "Turn event hypotheses into causal, inspectable, restartable Rust pipelines.",
      },
    ],
  ],
  themeConfig: {
    logo: false,
    siteTitle: "Helios Alpha",
    search: { provider: "local" },
    nav: [
      { text: "Atlas", link: "/" },
      { text: "Build", link: "/guide/quickstart" },
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
        text: "Researcher walkthrough",
        items: [
          { text: "Start with a question", link: "/guide/quickstart" },
          {
            text: "Trade space weather",
            link: "/guide/space-weather-reference",
          },
          {
            text: "Build a 10-minute signal",
            link: "/guide/compose-a-strategy",
          },
          {
            text: "Build a Thompson portfolio",
            link: "/guide/build-a-thompson-portfolio",
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
          {
            text: "Hypothesis machines",
            link: "/concepts/hypothesis-machines",
          },
          { text: "Event time", link: "/concepts/event-time" },
          {
            text: "Online statistics",
            link: "/concepts/online-statistics",
          },
          {
            text: "Bayesian streams",
            link: "/concepts/bayesian-streams",
          },
          { text: "Checkpoints", link: "/concepts/checkpoints" },
        ],
      },
      {
        text: "Research",
        items: [
          { text: "Rare events", link: "/research/rare-events" },
          {
            text: "Bayesian event portfolios",
            link: "/research/bayesian-event-portfolios",
          },
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
          {
            text: "Capital admission",
            link: "/operations/capital-admission",
          },
          {
            text: "Incident response",
            link: "/operations/incident-response",
          },
          {
            text: "Golem durability",
            link: "/operations/golem-cloud",
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
      message: "Research infrastructure. Candidate signals have no order authority.",
      copyright: "Helios Alpha",
    },
  },
  transformHtml(code) {
    const contract = `<!--
THESIS: A quant researcher can turn one causal event contract into an inspectable and restartable signal pipeline.
OWN-WORLD: Cool-white atlas sheets, ink and cobalt rules, oxide event marks, evidence green, ruled plates, registration crosses, flat depth, Archivo and Recursive type.
STORY: A researcher states what was knowable, traces one observation through six state owners, then follows the same contract into executable Rust and replay proof.
FIRST VIEWPORT: A slim nav and research contract lead to a controllable six-stage trace; an aligned-time plot owns two-thirds below while annotations and restart state occupy the final third; the 10-minute walkthrough is the primary action.
FORM: Annotated Event Atlas, first in the grounded candidate list, seed ed3c4d6d.
FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance
-->`;
    return code.replace("<body>", `<body>\n${contract}`);
  },
});
