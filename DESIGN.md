---
name: "Helios Operator: Market Atlas"
description: A dense institutional evidence workstation with synchronized financial panes, causal event rails, and docked OMS truth.
colors:
  shell-black: "#050b12"
  graphite-ground: "#07101a"
  rail-navy: "#08121c"
  surface-alt: "#0a1520"
  surface-strong: "#0e1d2a"
  primary-ink: "#e7edf2"
  muted-ink: "#9ca9b4"
  axis-ink: "#8795a1"
  lapis-inspection: "#4f94ee"
  lapis-field: "#10213a"
  ice-blue: "#b9d5ff"
  oxide-event: "#e17455"
  mineral-green: "#59b77c"
  mineral-green-ink: "#79d09a"
  evidence-cyan: "#47c2cf"
  exception-amber: "#e5a12c"
  model-violet: "#a16de0"
  rule: "#253544"
  rule-soft: "#172533"
typography:
  unavailable-display:
    fontFamily: "Archivo Variable, system-ui, sans-serif"
    fontSize: "clamp(28px, 5vw, 44px)"
    lineHeight: 1
    letterSpacing: "-0.035em"
  section-title:
    fontFamily: "Archivo Variable, system-ui, sans-serif"
    fontSize: "16px"
    lineHeight: 1.2
    letterSpacing: "-0.01em"
  atlas-title:
    fontFamily: "Archivo Variable, system-ui, sans-serif"
    fontSize: "18px"
    fontWeight: 610
    lineHeight: 1.2
    letterSpacing: "0.19em"
  body:
    fontFamily: "Archivo Variable, system-ui, sans-serif"
    fontSize: "13px"
    lineHeight: 1.45
  data:
    fontFamily: "Recursive Variable, ui-monospace, monospace"
    fontSize: "11px"
    lineHeight: 1.35
    fontVariation: '"MONO" 1, "CASL" 0'
  atlas-data:
    fontFamily: "Recursive Variable, ui-monospace, monospace"
    fontSize: "7px"
    lineHeight: 1.25
    fontVariation: '"MONO" 1, "CASL" 0'
  control:
    fontFamily: "Recursive Variable, ui-monospace, monospace"
    fontSize: "9px"
    letterSpacing: "0.03em"
    fontVariation: '"MONO" 1, "CASL" 0'
  chart-label:
    fontFamily: "Recursive Variable, ui-monospace, monospace"
    fontSize: "10px"
    fontWeight: 650
    letterSpacing: "0.02em"
    fontVariation: '"MONO" 1, "CASL" 0'
  chart-unit:
    fontFamily: "Recursive Variable, ui-monospace, monospace"
    fontSize: "8px"
    letterSpacing: "0.03em"
    fontVariation: '"MONO" 1, "CASL" 0'
  event-label:
    fontFamily: "Recursive Variable, ui-monospace, monospace"
    fontSize: "7px"
    letterSpacing: "0.03em"
    fontVariation: '"MONO" 1, "CASL" 0'
rounded:
  square: "0px"
  hairline: "1px"
  marker: "50%"
spacing:
  micro: "4px"
  compact: "7px"
  control: "9px"
  cell: "11px"
  panel: "14px"
  shell: "18px"
  inspector: "28px"
components:
  operator-shell:
    backgroundColor: "{colors.graphite-ground}"
    textColor: "{colors.primary-ink}"
    rounded: "{rounded.square}"
  navigation-rail:
    backgroundColor: "{colors.surface-alt}"
    textColor: "{colors.muted-ink}"
    typography: "{typography.control}"
    rounded: "{rounded.square}"
    width: "188px"
  desk-toolbar:
    backgroundColor: "{colors.surface-alt}"
    textColor: "{colors.axis-ink}"
    typography: "{typography.control}"
    rounded: "{rounded.square}"
    height: "44px"
  registered-series-row:
    backgroundColor: "{colors.graphite-ground}"
    textColor: "{colors.primary-ink}"
    typography: "{typography.data}"
    rounded: "{rounded.square}"
    height: "58px"
  series-register:
    backgroundColor: "{colors.graphite-ground}"
    textColor: "{colors.primary-ink}"
    typography: "{typography.control}"
    rounded: "{rounded.square}"
    width: "190px"
  chart-field:
    backgroundColor: "{colors.graphite-ground}"
    textColor: "{colors.axis-ink}"
    typography: "{typography.chart-label}"
    rounded: "{rounded.square}"
    height: "546px"
  event-rails:
    backgroundColor: "{colors.graphite-ground}"
    textColor: "{colors.axis-ink}"
    typography: "{typography.event-label}"
    rounded: "{rounded.square}"
    height: "48px"
  global-scrubber:
    backgroundColor: "{colors.rail-navy}"
    textColor: "{colors.ice-blue}"
    typography: "{typography.event-label}"
    rounded: "{rounded.square}"
    height: "42px"
  evidence-rail:
    backgroundColor: "{colors.rail-navy}"
    textColor: "{colors.primary-ink}"
    typography: "{typography.data}"
    rounded: "{rounded.square}"
    width: "286px"
  oms-ledger-dock:
    backgroundColor: "{colors.graphite-ground}"
    textColor: "{colors.muted-ink}"
    typography: "{typography.atlas-data}"
    rounded: "{rounded.square}"
    height: "142px"
  stale-boundary:
    backgroundColor: "{colors.graphite-ground}"
    textColor: "{colors.oxide-event}"
    typography: "{typography.control}"
    rounded: "{rounded.square}"
    height: "38px"
  investigation-result:
    backgroundColor: "{colors.rail-navy}"
    textColor: "{colors.muted-ink}"
    typography: "{typography.data}"
    rounded: "{rounded.square}"
  perspective-explorer:
    backgroundColor: "{colors.shell-black}"
    textColor: "{colors.primary-ink}"
    typography: "{typography.data}"
    rounded: "{rounded.square}"
---

# Design System: Helios Operator

## Overview

**Creative North Star: "Market Atlas"**

Helios Operator is a dense institutional evidence workstation, not a card dashboard or documentation page. Market Atlas uses a persistent three-column topology: a registered-series ledger, a synchronized financial chart field, and a point-in-time evidence rail. Orders, positions, alerts, and reconciliation dock below so every trading decision remains inspectable against the operational record. The documentation site's atlas guidance remains separately governed by `docs/DESIGN.md`.

Operators compose candlestick, bar, histogram, line, area, and baseline series into independently resizable panes or deliberate overlays. Raw, indexed, percent-change, and z-score transforms remain explicit. One shared cursor, three event rails, a global interval scrubber, and an independently zoomable viewport keep causal evidence synchronized while occurred time and available time remain distinct.

`@helios-alpha/atlas-charts` is the TypeScript boundary around TradingView Lightweight Charts. Application ports supply plain scalar or OHLC points and lifecycle markers; chart-engine types never enter OMS, strategy, or transport contracts. The Vue overview stays lightweight, Perspective 5.3 remains lazy, and AI investigation stays read-only, cited, bounded to the selected interval, and unable to execute.

**Key Characteristics:**

- Near-black graphite app shell with ink-navy surfaces and no documentation chrome.
- Three-column series register, synchronized chart field, and point-in-time evidence rail.
- Candlestick, bar, histogram, line, area, and baseline series in resizable synchronized panes.
- One cursor, three event rails, a global interval scrubber, and an independent viewport.
- Dense Recursive OMS ledgers, exact tabular numerals, and one-pixel rules.
- Lapis and ice-blue inspection, oxide events, mineral-green evidence, and scarce amber exceptions.
- Persistent mode, capital, data-class, connection, sequence, and freshness truth.
- Fail-closed initial state and visibly stale last-known-good state.
- Explicit zero-series state and truthful empty states for every OMS ledger.
- Mobile fidelity through local, keyboard-focusable horizontal scrolling.
- Read-only, cited AI investigation with a separate protected command boundary.
- Plain scalar and OHLC app contracts behind an engine-independent chart package.

## Colors

The palette behaves like precise instrumentation on near-black graphite. The surrounding ink-navy surfaces recede so registered traces, lifecycle events, evidence state, and exceptions remain legible without decorative glow.

### Primary

- **Lapis Inspection** (#4f94ee): Active navigation, selected series, current controls, focus outlines, shared inspection, and OMS links.
- **Lapis Field** (#10213a): Selected rows, active tabs, and inspected regions without simulated elevation.
- **Ice Blue** (#b9d5ff): Selected intervals, scrub handles, active control text, and precision chart chrome.

### Secondary

- **Oxide Event** (#e17455): Order lifecycle emphasis, unavailable state, stale state, rejected input, and destructive or negative facts.
- **Mineral Green** (#59b77c): Available evidence, fills, matched reconciliation, healthy state, and positive facts.
- **Mineral Green Ink** (#79d09a): Higher-contrast text form of available and reconciled state.

### Tertiary

- **Evidence Cyan** (#47c2cf): Acknowledgements, source-quality traces, and registered supporting evidence.
- **Exception Amber** (#e5a12c): Alerts, late observations, pending reconciliation, and scarce exceptions only.
- **Model Violet** (#a16de0): Signal posterior traces, replace or cancel lifecycle marks, and bounded model output.

### Neutral

- **Shell Black** (#050b12): Browser-level surround and deepest unavailable field.
- **Graphite Ground** (#07101a): Primary timeline, ledger, and application field.
- **Rail Navy** (#08121c): Navigation, point-in-time evidence, and investigation field.
- **Alternate Graphite** (#0a1520): Toolbars and table headers.
- **Strong Graphite** (#0e1d2a): Hover and selected regions.
- **Primary Ink** (#e7edf2): Headings, cursor marks, and high-priority values.
- **Muted Ink** (#9ca9b4): Ordinary data and explanatory copy.
- **Axis Ink** (#8795a1): Axes, units, provenance, pending state, and secondary measurement labels.
- **Primary Rule** (#253544): Shell divisions and major region boundaries.
- **Soft Rule** (#172533): Ledger cells, lane boundaries, and measurement grids.

**The Evidence Color Rule.** Lapis means active inspection, while ice blue registers the selected interval and precision chrome. Oxide marks lifecycle or failure. Mineral green means available, filled, matched, healthy, or positive. Cyan identifies supporting evidence. Amber is reserved for late, pending, or exceptional state. Violet identifies model state. Text and shape always carry the meaning with color.

**The Contrast Floor Rule.** Axis and unit text at normal size must remain at or above 4.5:1 against its field. The current axis and unit tokens measure above 6:1 on graphite.

**The Persistent Truth Rule.** Feed mode, connection, and capital gate stay visible in the command bar. Organization, workspace, account, data class, command authority, operator identity, sequence, checkpoint age, and clock offset stay in labeled controls or the navigation rail. Color supports these facts but never replaces text.

## Typography

**Display Font:** Archivo Variable (with system-ui and sans-serif fallbacks)

**Body Font:** Archivo Variable (with system-ui and sans-serif fallbacks)

**Label/Mono Font:** Recursive Variable (with ui-monospace and monospace fallbacks)

**Character:** Archivo gives the shell and unavailable states a direct institutional voice. Recursive turns timestamps, identifiers, quantities, units, controls, lifecycle marks, and ledger rows into exact operational evidence with tabular rhythm.

### Hierarchy

- **Unavailable Display** (28px to 44px, 1 line height): The only oversized statement, used when no validated snapshot exists.
- **Section Title** (16px, 1.2 line height): Evidence timeline and working-orders headings.
- **Atlas Title** (18px, weight 610, 0.19em tracking): The uppercase Market Atlas identity.
- **Body** (13px, 1.45 line height): Explanations, boundaries, and investigation summaries.
- **Data** (11px, Recursive mono axis): Orders, identifiers, timestamps, quantities, provenance, and evidence values.
- **Atlas Data** (7px, Recursive mono axis): Docked OMS rows, event rails, series provenance, scrubber state, and dense workstation annotations.
- **Chart Label** (10px, weight 650): Lane names and primary chart annotations.
- **Control** (9px, uppercase Recursive mono axis): Time windows, transforms, series controls, tabs, and actions.
- **Chart Unit** (8px, uppercase Recursive mono axis): Units, axis values, table headers, and secondary annotations.
- **Event Label** (7px, uppercase Recursive mono axis): Lifecycle marks and the tightest evidence metadata only.

**The Operational Mono Rule.** If a value changes, identifies, reconciles, timestamps, measures, transforms, or gates operation, set it in Recursive with tabular numerals where applicable.

## Layout

Helios Operator owns the viewport. A compact command bar carries product and tenant identity, operating state, alerts, and operator controls. Beneath it, a keyboard-resizable 156px to 264px navigation rail defaults to 188px and joins one fluid ruled workspace. Market Atlas then holds a 190px series register, a chart field no narrower than 680px, and a 286px evidence rail. Orders, positions, alerts, and reconciliation share a four-column dock below.

The chart field dedicates 546px to independently resizable panes, 48px to order, fill, and system event rails, and 42px to the global navigator and interval controls. The series register mirrors pane stretch factors, while one shared crosshair drives the point-in-time rail. The navigator shows the full domain, current viewport, selected interval, and cursor as distinct registered layers. Zooming the interval changes the chart viewport without discarding the global selection or the underlying evidence window.

Between 721px and 1400px the atlas keeps a 1000px working frame with 166px, at least 584px, and 250px columns. At 1180px the series drawer moves from three columns to two. At 720px and below, the atlas deliberately keeps a 1080px frame with 174px, 660px, and 246px columns, while the four OMS ledgers each retain a 270px working width. A visible instruction names the horizontal gesture instead of collapsing the workstation into cards.

The body hides global horizontal overflow. The full atlas, series controls, navigation tabs, filters, and docked ledgers own their density locally. Each interactive horizontal region is keyboard-focusable, carries a descriptive accessibility label, and shows a visible 2px lapis focus outline.

**The Local Scroll Rule.** Preserve the width required for temporal and ledger comparison, then isolate overflow inside a named, keyboard-focusable region. On mobile, keep the series register, chart field, evidence rail, and OMS dock in the same causal geometry. Never replace them with cards, create a page-level horizontal scrollbar, or shrink text below the established scale.

## Elevation & Depth

The app is flat and shadow-free. Depth comes from adjacent graphite and ink-navy tones, one-pixel rules, pane separators, selected-range fields, and trace opacity. Motion is limited to responsive navigation reframing and a restrained loading pulse; chart pan, scale, cursor, and scrub operations respond directly without ornamental easing. Reduced motion disables the pulse. Sticky shell regions may use backdrop blur, but no operational fact floats above another through shadow.

**The Ruled Depth Rule.** Use rules, tone, state fill, and registered alignment to establish hierarchy. Do not turn evidence, orders, or operational truth into isolated cards.

**The Restrained Motion Rule.** Motion may register loading or a direct spatial change. It must not animate market evidence for spectacle, and reduced-motion preference removes every nonessential loop.

## Shapes

Shell regions, chart panes, ledgers, controls, buttons, chips, and inspectors are square with 0px radius. Small circles register crosshair samples and connection state. Six-pixel rotated squares register lifecycle events. The orbital Helios mark remains crisp and geometric.

**The Instrument Shape Rule.** Square geometry holds information, circles register sampled points or status, and diamonds register lifecycle events. None of these forms is decorative.

## Components

### Standalone Operator Shell

- **Structure:** Full-viewport graphite ground, compact command bar, resizable or collapsible operations rail, and one fluid ruled workspace capped at 1920px.
- **Identity:** Helios, organization, account, data class, feed mode, connection, alert count, and operator state remain available without VitePress navigation.
- **Truth:** Provider, sequence, checkpoint age, and clock offset stay visible at the foot of the expanded operations rail.
- **Boundary:** The validated snapshot stream is read-only. Mutation crosses a separate authenticated command service with explicit review and admission.

### Command Bar and Navigation Rail

- **Views:** Evidence loads by default. Positions, Orders, Signals, Activity, Sources, Alerts, Control, and Explore remain distinct deep-linked workspaces.
- **Navigation:** Arrow keys, Home, and End move among rail tabs. The rail resizer supports pointer and keyboard operation and persists width or collapsed state locally.
- **Persistent State:** Mode supports demo, shadow, paper, live, or pending. Connection supports live, frozen, connecting, reconnecting, snapshot, or offline. The capital gate remains text-labeled beside them.
- **Disclosure:** Organization, workspace, account, data class, command authority, operator identity, and active incidents retain visible text labels.

### Registered Series and Overlays

- **Toolbar:** Search, transform, time window, and Manage Series controls stay in one 44px ruled strip.
- **Catalog:** Every series exposes label, domain, provenance, state color, and shown or add state. The three-column drawer reduces to two columns at 1180px and one at 720px.
- **Placement:** A registered series can be removed, added in its own pane, or overlaid with an existing pane.
- **Transforms:** Raw, indexed, percent-change, and z-score views are explicit. A raw overlay containing mixed units automatically uses indexed values.
- **Persistence:** The evidence window, active series, pane placement, and transform persist per account and can be restored to the registered default.

### Atlas Charts Boundary

- **Package:** `@helios-alpha/atlas-charts` is the reusable TypeScript boundary around TradingView Lightweight Charts 5.2.
- **Public Contract:** Applications provide pane identifiers, weights, series definitions, and plain numeric scalar or OHLC points. Vue, OMS records, strategy concepts, transports, and engine-specific types stay outside the package boundary.
- **Series:** Candlestick and bar consume OHLC points. Histogram, line, area, and baseline consume scalar points.
- **State:** The controller owns rendering, synchronized time scales, crosshair samples, pane resizing, visible-range retention, cursor placement, and viewport changes.

### Synchronized Chart Field

- **Composition:** Series with the same pane identifier overlay. Different pane identifiers create independently resizable panes on one shared time scale.
- **Register:** The left series ledger mirrors pane weights, provenance, current values, and trace colors without consuming plotting width.
- **Cursor:** The chart crosshair, navigator cursor, evidence timestamp, available observations, and selected lifecycle event follow one evidence cut.
- **Viewport:** Wheel, drag, pinch, axis scale, Zoom Interval, and Full Window change the visible range without changing the selected global interval.
- **Zero Series:** Removing every series destroys the chart instance and reveals an explicit Choose Series action on a ruled empty field.

### Lifecycle Rails and Global Scrubber

- **Rails:** Orders, fills, and system events occupy three labeled tracks below the chart. Six-pixel diamonds use lifecycle colors and move the shared cursor when selected.
- **Navigator:** A miniature price trace, retained viewport, selected interval, and cursor remain visually distinct across the full evidence window.
- **Controls:** Start, end, and cursor range controls preserve a minimum interval. Back, forward, Zoom Interval, and Full Window remain native keyboard controls.
- **Timing:** Event occurrence and evidence availability are stored and displayed separately.
- **OMS Link:** A marker associated with an order or fill opens the corresponding OMS record instead of creating a parallel detail surface.

### Point-in-Time Evidence Rail

- **Header:** Selected lifecycle label and millisecond cursor timestamp establish the inspection cut.
- **Availability:** Every active series reports the most recent value that had occurred and arrived by the cursor. Meaningful late arrivals remain separate.
- **Decision:** Model version, risk decision, order intent, broker acknowledgement, fill result, and selected price form one causal record.
- **Investigation:** AI may summarize the selected interval, cite observations, and suggest registered series. It is read-only, bounded, explicit about limitations, and cannot execute.
- **Continuity:** Suggested series changes the local evidence workspace only. It does not alter trading state or command authority.

### Docked OMS Ledgers

- **Registers:** Orders, positions, alerts, and reconciliation occupy four aligned ruled ledgers directly below the atlas.
- **Density:** 7px Recursive labels and values, 25px body rows, 19px headers, exact tabular numerals, no wrapping, and one-pixel row rules preserve workstation scale.
- **Order Controls:** Order timestamps are native buttons, remain keyboard reachable, and open the same OMS record used by causal evidence.
- **Empty Truth:** The ledgers say No active orders, No open positions, No open alerts, or No reconciliation records. Empty never means loading or unavailable.
- **Overflow:** Each register owns local overflow. On mobile, the four 270px ledgers stay docked in one horizontally inspectable row.

### Fail-Closed and Stale State

- **Initial Load:** No validated snapshot means no operational tables and no substituted demo fixture.
- **Unavailable:** Connection progress and source failure use distinct text. Retry appears only after error.
- **Last Known Good:** A later stream failure preserves the last validated snapshot, marks it stale, names the last update, and raises an incident.
- **Admission:** Stale or absent evidence never grants capital or command authority.

### Protected Command Boundary

- **Read Model:** Snapshot, time-series, investigation, and Perspective ports cannot issue trading commands.
- **Control:** Strategy, stage, order, position, and emergency actions cross the separately authenticated command port.
- **Admission:** Commands require a reason, exact confirmation where appropriate, CSRF protection, idempotency, and the current snapshot sequence.
- **Receipts:** The interface displays validated command receipts and waits for the read stream to report resulting state.

### Lazy Perspective Explorer

- **Boundary:** Vue loads the explorer through an async component only after Data Explorer is requested.
- **Runtime:** Perspective 5.3 client, viewer, datagrid, Pro Dark theme, worker, JavaScript, and WebAssembly stay out of the initial overview path.
- **Data:** Signals, positions, orders, fills, and sources become an entity-bounded table keyed by row identity; later snapshots use keyed updates.
- **Failure:** Loading stages are explicit and timeout-bounded. Worker failure produces an oxide error state with retry guidance.

## Do's and Don'ts

### Do:

- Do keep mode, connection, capital gate, tenant, data class, authority, operator, sequence, checkpoint, and clock facts text-labeled.
- Do keep the series register, synchronized chart field, evidence rail, and docked OMS ledgers in one continuous workstation.
- Do let operators add or remove registered series and place each in its own resizable pane or a deliberate overlay.
- Do use candlestick or bar definitions for OHLC points and histogram, line, area, or baseline definitions for scalar points.
- Do normalize mixed-unit raw overlays to indexed values and label the resulting transform.
- Do drive every pane, lifecycle marker, evidence observation, and selected value from one shared cursor.
- Do keep the global interval, chart viewport, and cursor visually distinct and independently controllable.
- Do keep occurred time and available time separate in data, labels, investigations, and OMS evidence.
- Do link lifecycle and working-order evidence to the corresponding OMS record.
- Do use native keyboard controls for interval handles, cursor steps, zoom, full-window reset, and OMS order links.
- Do show an explicit Choose Series action at zero series and a truthful named empty state in every OMS ledger.
- Do reject malformed snapshots before they replace the last validated state, and show no fixture when initial validation fails.
- Do mark the last validated snapshot stale when later updates fail, including its last update time.
- Do keep AI investigation cited, bounded, read-only, and unable to execute.
- Do preserve Market Atlas fidelity on mobile through named, keyboard-focusable local scroll regions.
- Do keep Lightweight Charts types inside `@helios-alpha/atlas-charts` and give app ports only plain data contracts.
- Do keep motion restrained and disable loading animation when reduced motion is requested.
- Do keep Perspective JavaScript, worker, datagrid, and WebAssembly out of the initial Vue overview bundle.
- Do label deterministic fixtures synthetic and observed operations data observed.

### Don't:

- Don't wrap Helios Operator in documentation navigation or treat its timeline as a VitePress component.
- Don't replace Market Atlas with a dashboard of summary cards or split its three-column causal reading path.
- Don't compare unlike raw units on the same overlay or imply a common scale when none exists.
- Don't let panes, event rails, the navigator, or the evidence rail drift to different event-time cuts.
- Don't confuse the selected global interval with the independently zoomed chart viewport.
- Don't collapse occurred and available time into one timestamp or present late evidence as available.
- Don't hide mode, connection, capital, data class, freshness, or command authority inside color-only state.
- Don't replace malformed data with demo data or present stale state as live.
- Don't leak TradingView Lightweight Charts types into OMS, strategy, time-series, investigation, or transport ports.
- Don't add execution authority to snapshot, time-series, chart, investigation, or explorer ports.
- Don't eagerly import Perspective or its WebAssembly assets.
- Don't collapse the mobile workstation into cards, create global horizontal overflow, shrink axis text, add decorative gradients, or use drop shadows.
- Don't imply that a candidate signal, cited investigation, confirmed fill, or observed feed authorizes live capital.
