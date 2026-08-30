---
name: Helios Alpha
description: An annotated event atlas for inspectable, restartable research systems.
colors:
  atlas-ground: "#f9f9f9"
  surface-alt: "#f3f5f7"
  surface-soft: "#eef2f6"
  ink: "#14223d"
  muted-ink: "#5d687b"
  axis-muted: "#87909e"
  action-cobalt: "#0b4cb6"
  action-cobalt-deep: "#173f89"
  action-cobalt-dark: "#142f68"
  confidence-wash: "#e4ecf8"
  event-oxide: "#b8535c"
  evidence-green: "#83b844"
  evidence-green-ink: "#527d14"
  rule: "#cdcdcf"
  rule-soft: "#e2e3e5"
  inverse: "#ffffff"
  code-text: "#dbe6f4"
  code-muted: "#aebbd0"
typography:
  statement:
    fontFamily: "Archivo Variable, sans-serif"
    fontSize: "clamp(28px, 3vw, 44px)"
    lineHeight: 1.04
    letterSpacing: "-0.04em"
  thesis:
    fontFamily: "Archivo Variable, sans-serif"
    fontSize: "clamp(21px, 1.8vw, 28px)"
    lineHeight: 1.08
    letterSpacing: "-0.035em"
  section-title:
    fontFamily: "Archivo Variable, sans-serif"
    fontSize: "18px"
    lineHeight: 1.2
    letterSpacing: "-0.02em"
  observatory-statement:
    fontFamily: "Archivo Variable, sans-serif"
    fontSize: "clamp(30px, 3.5vw, 52px)"
    lineHeight: 0.98
    letterSpacing: "-0.04em"
  mobile-statement:
    fontFamily: "Archivo Variable, sans-serif"
    fontSize: "34px"
    lineHeight: 0.98
    letterSpacing: "-0.04em"
  supporting:
    fontFamily: "Archivo Variable, sans-serif"
    fontSize: "14px"
    lineHeight: 1.55
  lane-title:
    fontFamily: "Archivo Variable, sans-serif"
    fontSize: "17px"
    fontWeight: 720
    lineHeight: 1.2
  body:
    fontFamily: "Archivo Variable, sans-serif"
    fontSize: "15px"
    fontWeight: 400
    lineHeight: 1.62
  nav-title:
    fontFamily: "Recursive Variable, monospace"
    fontSize: "15px"
    fontWeight: 650
    letterSpacing: "0.13em"
    fontVariation: '"MONO" 1, "CASL" 0, "wght" 650'
  data:
    fontFamily: "Recursive Variable, monospace"
    fontSize: "13px"
    fontWeight: 580
    fontVariation: '"MONO" 1, "CASL" 0'
  label:
    fontFamily: "Recursive Variable, monospace"
    fontSize: "11px"
    fontWeight: 650
    letterSpacing: "0.06em"
    fontVariation: '"MONO" 1, "CASL" 0'
  compact-label:
    fontFamily: "Recursive Variable, monospace"
    fontSize: "10px"
    fontVariation: '"MONO" 1, "CASL" 0'
  micro-label:
    fontFamily: "Recursive Variable, monospace"
    fontSize: "8px"
    fontVariation: '"MONO" 1, "CASL" 0'
  code:
    fontFamily: "Recursive Variable, monospace"
    fontSize: "clamp(12px, 1.1vw, 15px)"
    lineHeight: 1.72
    fontVariation: '"MONO" 1, "CASL" 0'
rounded:
  square: "0px"
  hairline: "1px"
  circle: "50%"
spacing:
  micro: "4px"
  compact: "8px"
  small: "12px"
  plate: "20px"
  section: "38px"
  atlas-gutter: "42px"
motion:
  posterior-phase: "560ms cubic-bezier(0.16, 1, 0.3, 1)"
  posterior-observe: "380ms to 650ms staggered ease-out"
  posterior-register: "720ms cubic-bezier(0.16, 1, 0.3, 1)"
components:
  nav-title:
    textColor: "{colors.ink}"
    typography: "{typography.nav-title}"
    rounded: "{rounded.square}"
  nav-link:
    textColor: "{colors.ink}"
    typography: "{typography.label}"
    rounded: "{rounded.square}"
    padding: "0 12px"
  primary-action:
    backgroundColor: "transparent"
    textColor: "{colors.action-cobalt}"
    typography: "{typography.label}"
    rounded: "{rounded.hairline}"
    padding: "0 0 1px"
  primary-action-hover:
    backgroundColor: "transparent"
    textColor: "{colors.event-oxide}"
    typography: "{typography.label}"
    rounded: "{rounded.hairline}"
    padding: "0 0 1px"
  stage-node:
    backgroundColor: "{colors.atlas-ground}"
    textColor: "{colors.action-cobalt}"
    rounded: "{rounded.circle}"
    size: "30px"
  stage-node-active:
    backgroundColor: "{colors.action-cobalt}"
    textColor: "{colors.inverse}"
    rounded: "{rounded.circle}"
    size: "30px"
  replay-control:
    backgroundColor: "transparent"
    textColor: "{colors.action-cobalt}"
    typography: "{typography.label}"
    rounded: "{rounded.square}"
    padding: "0 13px"
  posterior-frequency-lane:
    backgroundColor: "{colors.atlas-ground}"
    textColor: "{colors.ink}"
    rounded: "{rounded.square}"
    padding: "10px 14px"
  posterior-selected-lane:
    backgroundColor: "{colors.surface-soft}"
    textColor: "{colors.evidence-green-ink}"
    rounded: "{rounded.square}"
    padding: "10px 14px"
  research-contract-row:
    backgroundColor: "transparent"
    textColor: "{colors.ink}"
    rounded: "{rounded.square}"
    padding: "30px 54px"
  specification-fact:
    backgroundColor: "transparent"
    textColor: "{colors.ink}"
    typography: "{typography.data}"
    rounded: "{rounded.square}"
    padding: "10px 20px"
  ruled-plate:
    backgroundColor: "{colors.atlas-ground}"
    textColor: "{colors.ink}"
    rounded: "{rounded.square}"
    padding: "20px"
  evidence-status:
    backgroundColor: "{colors.atlas-ground}"
    textColor: "{colors.muted-ink}"
    typography: "{typography.label}"
    rounded: "{rounded.square}"
    padding: "7px 42px"
  code-plate:
    backgroundColor: "{colors.ink}"
    textColor: "{colors.code-text}"
    typography: "{typography.code}"
    rounded: "{rounded.square}"
    padding: "24px 26px"
---

# Design System: Helios Alpha

## Overview

**Creative North Star: "The Annotated Event Atlas"**

Helios Alpha presents computation like a contemporary scientific atlas: a continuous cool-white sheet where rules, labels, plots, and state records make the system inspectable. The visual language is rigorous and calm. It favors exact alignment, compact evidence density, and explicit status over ornamental product marketing.

The world pairs editorial hierarchy with the working character of a statistical notebook. Cobalt traces actions and active computation, oxide pins event time and cautions, and green appears only when evidence has been verified. Registration crosses, ruled plates, authored diagrams, and plain state ledgers make provenance visible without implying that synthetic demonstrations are trading results.

The Posterior Observatory extends that world into uncertainty. Related frequency lanes share one ruled frame, posterior densities register from left to right, oxide needles mark keyed samples, and the selected eligible lane receives the only green field. The sequence is explanatory: observe, pool, constrain, draw, select.

The primary reader is a quantitatively literate researcher who understands returns, variance, event studies, and backtests but should not need prior knowledge of Helios crate boundaries. Every page should first answer what is observed, when it becomes knowable, which operator owns state, what can emit, how replay works, and which claim remains unproven.

**Key Characteristics:**

- Continuous cool-white atlas sheets divided by one-pixel rules.
- Dense but readable evidence, aligned to a shared temporal or structural axis.
- Archivo for explanation and Recursive for labels, metrics, and code.
- Cobalt paths, oxide event marks, and scarce evidence green.
- Flat depth, near-square controls, registration crosses, and authored SVG diagrams.

## Colors

The palette reads like technical ink on a cool research sheet, with color assigned by semantic evidence role rather than decoration.

### Primary

- **Action Cobalt:** Carries links, active paths, selected states, plot lines, and navigation emphasis.
- **Deep Cobalt:** Supports darker brand states where the default action color needs more weight.
- **Cobalt Ink:** Provides the darkest member of the action family for constrained UI contexts.

### Secondary

- **Event Oxide:** Marks event time, focus outlines, warnings, unproven state, and the rare interactive color shift. It is not a general accent.

### Tertiary

- **Evidence Green:** Marks verified or compatible state only. Use the darker green ink when colored text must meet contrast requirements.

### Neutral

- **Atlas Ground:** The continuous page sheet and default stage-node interior.
- **Alternate and Soft Surfaces:** Quiet tonal changes for sidebars, boundaries, and confidence regions without creating elevation.
- **Main Ink:** Headlines, body copy, axes, and the dark code plate.
- **Muted Ink:** Supporting explanations, secondary labels, and chart context.
- **Rule and Soft Rule:** Structural dividers, grids, ledgers, and local row separation.
- **Code Text and Code Muted:** High-legibility text roles on the inverted code plate.
- **Inverse:** Text and marks placed on filled cobalt or oxide states.

**The Evidence Color Rule.** Cobalt means action or active computation, oxide means event or caution, and green means verified evidence. Never use these colors as interchangeable decoration.

## Typography

**Display Font:** Archivo Variable (with sans-serif fallback)

**Body Font:** Archivo Variable (with sans-serif fallback)

**Label/Mono Font:** Recursive Variable (with monospace fallback)

**Character:** Archivo gives explanations and large statements an editorial, research-grade voice. Recursive provides the compact instrument panel for method names, state values, navigation, metadata, and code.

### Hierarchy

- **Statement** (44px maximum, 1.04 line height): Large section claims on wide screens, with tight tracking and compact measure.
- **Thesis** (28px maximum, 1.08 line height): The homepage proposition and other concise introductory claims.
- **Section Title** (18px, 1.2 line height): Plate headings and plot titles.
- **Body** (15px, 1.62 line height): Explanations and operational boundaries, generally held to a readable text measure.
- **Data** (13px, Recursive mono axis): Runtime facts, checkpoint values, and concise quantitative readouts.
- **Label** (11px, Recursive mono axis, uppercase): Navigation, metadata, methods, legends, and state labels. Smaller 8px to 10px labels are reserved for dense charts and ledgers.
- **Code** (12px to 15px, 1.72 line height): Rust examples and other executable material.

**The Two-Hand Rule.** Archivo explains the system. Recursive identifies, measures, and executes it.

### Research Language

Copy follows the causal computation, not the package graph. Prefer concrete terms such as `available_at`, decision cut, open bucket, closed summary, candidate signal, source offset, and compatible snapshot. Use "strategy" only for research-owned policy plus its evidence contract. Use "signal" for an emitted research candidate. Never use "trade" or "order" for an output that has not crossed an execution and risk boundary.

Controls name the action they perform: "Play trace," "Advance one stage," and "Restore checkpoint." Evidence labels state whether values are synthetic, mechanically tested, empirically estimated, or unproven.

## Layout

The atlas uses one continuous bounded sheet, up to 1800px wide, with major regions joined edge to edge. One-pixel rules establish the grid. Desktop compositions can use a two-thirds evidence surface with a one-third annotation or state ledger, but the reusable rule is alignment around shared evidence, not a fixed homepage template.

Spacing is compact near data and generous around arguments. Plate interiors use 20px to 42px gutters, while long-form sections use approximately 38px vertical padding. Registration crosses may mark major joins or boundaries, but never float as unrelated decoration.

At 1100px, multi-column regions simplify, the pipeline rail may form three columns, and evidence panels stack. At 760px, the rail becomes a vertical sequence, specification facts use two columns, and plot overflow stays inside its own panel so the page never scrolls horizontally. Time plots and posterior lanes may pan locally when their shared axis needs more width; a visible swipe instruction names that behavior. Controls remain keyboard reachable and their labels remain legible.

**The Shared Axis Rule.** Related evidence must align to the same ruled grid, temporal axis, or state boundary. Do not scatter facts into independent cards.

## Elevation & Depth

The system is flat. It uses no drop shadows and no floating card stack. Depth comes from tonal paper changes, dark inverted code plates, one-pixel borders, line weight, and selective filled states. The event pulse has a single one-pixel outline to remain visible against the path, not to simulate elevation.

**The Flat Evidence Rule.** If a region needs hierarchy, change its rule, tone, or alignment before considering any shadow. Shipping surfaces remain shadow-free.

## Shapes

Major surfaces and code plates are square. Interactive text links and search shells may use a 1px radius, which reads as near-square rather than softened. Circular geometry is reserved for pipeline nodes, data points, status dots, and registration mechanics. Rules are one pixel, plot and path strokes are generally two to three pixels, and selected-state underlines are two pixels.

**The Instrument Shape Rule.** Corners stay square or nearly square. Circles identify nodes and measured points, never decorative badges.

## Components

### Primary Actions

- **Shape:** An inline text action with a one-pixel cobalt underline and a near-square 1px radius.
- **Default:** Cobalt Recursive label text on a transparent ground, usually paired with a right arrow.
- **Hover / Focus:** Text and underline shift to oxide over 160ms. Keyboard focus remains visibly distinct.
- **Secondary:** Uses the same form and hierarchy. Placement and copy, not a filled button, establish secondary priority.

### Navigation

- **Style:** A slim ruled bar. The product title uses 15px Recursive with wide uppercase tracking; links use compact uppercase Recursive labels.
- **State:** Hover and active links turn cobalt. The active destination receives a two-pixel cobalt underline aligned to the bottom rule.
- **Mobile:** Preserve the rule structure and native menu behavior supplied by the documentation shell.

### Ruled Plates and Ledgers

- **Corner Style:** Square.
- **Background:** Atlas ground or a quiet alternate surface.
- **Depth:** No shadow. One-pixel rule boundaries and aligned rows provide structure.
- **Internal Padding:** Compact ledger rows use 5px to 12px; major plate interiors use 20px to 42px.
- **State:** Verified values use evidence-green ink. Unproven or cautionary values use oxide.

### Pipeline Stage Selector

- **Structure:** An index, 30px circular node, uppercase stage label, method label, and explicit connector.
- **Default:** Cobalt outline node on the atlas ground.
- **Hover / Focus / Selected:** The node fills cobalt and moves up 2px. Focus receives a two-pixel oxide outline with a 5px offset; the selected label also turns cobalt.
- **Motion:** One oxide pulse advances through the six state owners every 1.35 seconds while the trace is playing. Node changes use a 460ms exponential ease-out; connector registration uses 360ms. The loop pauses when the atlas is offscreen or the page is hidden. Reduced-motion users receive a static paused state.

### Replay Controls

- **Actions:** Play or pause the trace, advance exactly one stage, and restore from the synthetic checkpoint.
- **State:** The status line reports replay pass, selected position, and restore validation. Labels remain explicit on desktop and mobile.
- **Layout:** Controls share the pipeline rules. On mobile, play and advance occupy the first row while restore spans the second row.

### Posterior Observatory

- **Structure:** Five explicit phases sit above three aligned frequency lanes and one decision ledger. Each lane contains observations, a posterior density, a credible interval, a feasibility result, and a keyed draw only when eligible.
- **Sequence:** Observe reveals admitted outcomes. Pool resolves the posterior field. Constrain removes infeasible lanes before sampling. Draw drops one oxide sample needle for each eligible lane. Select gives evidence green only to the largest eligible draw.
- **Language:** Values are always labeled synthetic. A posterior sample is described as a decision input, never a guarantee. The final output is a research candidate with no order authority.
- **Motion:** The active phase field traverses with a 560ms exponential ease-out. Density registration uses clip and blur over 720ms. Animation pauses offscreen, pauses while the page is hidden, and resolves to the final static state for reduced-motion users.
- **Mobile:** The phase rail and density lanes pan inside their own ruled frames. The page itself never gains horizontal overflow, and a visible instruction tells the researcher to swipe.

### Research Contract Ledger

- **Structure:** Four ruled facts define the event, availability boundary, decision boundary, and falsifier.
- **Copy:** Each row is a complete research constraint, not a product benefit or crate description.
- **Layout:** Two columns on wide screens and one ordered ledger below 1100px.

### Plots and Evidence Graphics

- **Style:** Authored responsive SVG with visible axes, ruled minor grid, a cobalt response, pale confidence wash, oxide event marker, and green verified recovery point.
- **Labels:** Recursive metadata stays concise. Callouts attach directly to evidence with hairline leaders.
- **Boundary:** Synthetic data is labeled in the graphic description and again in the evidence status region.

### Code Plates

- **Shape:** Square dark-ink plate with a one-pixel border.
- **Text:** Light Recursive code at a generous 1.72 line height, with cobalt-blue keywords and green numeric values.
- **Header:** A compact ruled bar identifies the file and its architectural role.

## Do's and Don'ts

### Do:

- Do align state, annotation, and plot evidence to shared rules or axes.
- Do keep synthetic demonstrations visibly labeled and distinguish tested mechanics from unproven alpha.
- Do use cobalt for actions and selected paths, oxide for event or caution, and green only for verified state.
- Do preserve keyboard focus, semantic controls, and a static reduced-motion state.
- Do use authored SVG for diagrams and plots so labels and evidence remain inspectable.
- Do lead a researcher from causal contract to operator state, replay proof, and capital boundary.

### Don't:

- Don't use generic SaaS feature cards, floating panels, gradients, or drop shadows.
- Don't introduce dark trading-terminal styling, candlestick shorthand, or speculative profit cues.
- Don't round structural containers or turn pipeline nodes into decorative badges.
- Don't scatter evidence into unrelated tiles when a ruled ledger or shared axis can reveal the relationship.
- Don't ship rasterized interface text. Any future shipping raster must carry prompt and provenance metadata.
- Don't call a candidate signal a trade, an open bucket a result, or tested mechanics proven alpha.
