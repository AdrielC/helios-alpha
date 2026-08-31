---
name: Helios Control
description: A standalone obsidian mission-control surface with separate read and protected command planes.
colors:
  shell-black: "#060a0f"
  obsidian-ground: "#080d13"
  tape-black: "#05090d"
  surface-alt: "#0b1219"
  surface-strong: "#101a22"
  primary-ink: "#eef3f0"
  muted-ink: "#a6b0b8"
  axis-ink: "#64717c"
  polar-cyan: "#70c7df"
  polar-cyan-soft: "#10252c"
  coral-blocker: "#ff846e"
  ion-lime: "#b9e74b"
  ion-lime-ink: "#cff873"
  rule: "#25323b"
  rule-soft: "#18232b"
typography:
  unavailable-display:
    fontFamily: "Archivo Variable, system-ui, sans-serif"
    fontSize: "clamp(28px, 5vw, 44px)"
    lineHeight: 0.98
    letterSpacing: "-0.045em"
  section-title:
    fontFamily: "Archivo Variable, system-ui, sans-serif"
    fontSize: "16px"
    lineHeight: 1.2
    letterSpacing: "-0.02em"
  pane-title:
    fontFamily: "Archivo Variable, system-ui, sans-serif"
    fontSize: "24px"
    lineHeight: 1.2
    letterSpacing: "-0.025em"
  app-title:
    fontFamily: "Archivo Variable, system-ui, sans-serif"
    fontSize: "16px"
    fontWeight: 660
    letterSpacing: "-0.015em"
  body:
    fontFamily: "Archivo Variable, system-ui, sans-serif"
    fontSize: "13px"
    lineHeight: 1.45
  metric:
    fontFamily: "Recursive Variable, ui-monospace, monospace"
    fontSize: "21px"
    letterSpacing: "-0.035em"
    fontVariation: '"MONO" 1, "CASL" 0'
  data:
    fontFamily: "Recursive Variable, ui-monospace, monospace"
    fontSize: "11px"
    fontVariation: '"MONO" 1, "CASL" 0'
  label:
    fontFamily: "Recursive Variable, ui-monospace, monospace"
    fontSize: "10px"
    letterSpacing: "0.05em"
    fontVariation: '"MONO" 1, "CASL" 0'
  compact-label:
    fontFamily: "Recursive Variable, ui-monospace, monospace"
    fontSize: "9px"
    letterSpacing: "0.04em"
    fontVariation: '"MONO" 1, "CASL" 0'
  micro-label:
    fontFamily: "Recursive Variable, ui-monospace, monospace"
    fontSize: "8px"
    letterSpacing: "0.04em"
    fontVariation: '"MONO" 1, "CASL" 0'
  nano-label:
    fontFamily: "Recursive Variable, ui-monospace, monospace"
    fontSize: "7px"
    letterSpacing: "0.04em"
    fontVariation: '"MONO" 1, "CASL" 0'
rounded:
  square: "0px"
  hairline: "1px"
  circle: "50%"
spacing:
  hairline-gap: "3px"
  compact: "6px"
  small: "8px"
  control: "10px"
  cell: "14px"
  panel: "18px"
  shell: "22px"
components:
  app-shell:
    backgroundColor: "{colors.obsidian-ground}"
    textColor: "{colors.primary-ink}"
    rounded: "{rounded.square}"
  command-tab:
    backgroundColor: "transparent"
    textColor: "{colors.muted-ink}"
    typography: "{typography.label}"
    rounded: "{rounded.square}"
    padding: "0 20px"
    height: "64px"
  command-tab-active:
    backgroundColor: "{colors.surface-strong}"
    textColor: "{colors.polar-cyan}"
    typography: "{typography.label}"
    rounded: "{rounded.square}"
    padding: "0 20px"
    height: "64px"
  truth-chip:
    backgroundColor: "transparent"
    textColor: "{colors.polar-cyan}"
    typography: "{typography.label}"
    rounded: "{rounded.square}"
    padding: "5px 8px"
    height: "30px"
  truth-chip-blocked:
    backgroundColor: "transparent"
    textColor: "{colors.coral-blocker}"
    typography: "{typography.label}"
    rounded: "{rounded.square}"
    padding: "5px 8px"
    height: "30px"
  truth-chip-verified:
    backgroundColor: "transparent"
    textColor: "{colors.ion-lime-ink}"
    typography: "{typography.label}"
    rounded: "{rounded.square}"
    padding: "5px 8px"
    height: "30px"
  stale-boundary:
    backgroundColor: "{colors.surface-alt}"
    textColor: "{colors.coral-blocker}"
    typography: "{typography.label}"
    rounded: "{rounded.square}"
    padding: "0 22px"
    height: "44px"
  ruled-table:
    backgroundColor: "{colors.obsidian-ground}"
    textColor: "{colors.muted-ink}"
    typography: "{typography.data}"
    rounded: "{rounded.square}"
  unavailable-panel:
    backgroundColor: "{colors.tape-black}"
    textColor: "{colors.primary-ink}"
    rounded: "{rounded.square}"
    padding: "32px"
---

# Design System: Helios Control

## Overview

**Creative North Star: "The Annotated Event Atlas"**

Helios Control is a standalone operations instrument, not a documentation page. It renders operational truth on an obsidian field with ruled ledgers, compact state labels, and explicit causal paths. A separately authenticated command plane can pause strategies, hold stage transitions, cancel orders, flatten positions, or activate the kill switch without putting broker credentials in the browser.

The atlas language survives from Helios Alpha, but the app expression is darker, denser, and more operational. Polar cyan traces computation and active inspection, coral exposes blockers and stale state, and ion-lime marks healthy, observed, reconciled, or authorized facts. The interface never converts research evidence into capital authority by implication.

The first load is deliberately lightweight. A purpose-built Vue overview renders one validated snapshot through shallow state. Perspective 5.3, its JavaScript, its cacheable WebAssembly assets, and its isolated worker arrive only after the operator opens Data Explorer. The analytical workbench extends the overview; it does not own the initial shell.

**Key Characteristics:**

- Obsidian full-screen app shell with no documentation chrome.
- Persistent mode, capital, data-class, connection, and sequence truth.
- Dense Recursive tables and ledgers joined by one-pixel rules.
- Polar-cyan computation, coral blockers, and scarce ion-lime verification.
- Fail-closed initial state and visibly stale last-known-good state.
- Local, keyboard-focusable horizontal scrolling with visible cues.
- Independent read-model and protected-command boundaries with lazy Perspective analysis.

## Colors

The palette behaves like luminous instrumentation on obsidian glass. Accent color always communicates state or causality.

### Primary

- **Polar Cyan:** Active view, computation path, selected rows, inspection links, and connecting state.
- **Polar Cyan Soft:** Selected-row and active-navigation field without creating elevation.

### Secondary

- **Coral Blocker:** Capital closed, blocked signals, stale snapshots, degraded sources, errors, negative values, and focus outlines.

### Tertiary

- **Ion-Lime:** Healthy sources, observed data, authorized capital, reconciled facts, confirmed executions, positive values, and ready analytical state.
- **Ion-Lime Ink:** Higher-contrast text form of the verification color.

### Neutral

- **Shell Black:** Browser-level page ground and selection inverse.
- **Obsidian Ground:** Primary application field.
- **Tape Black:** Deep event-tape, unavailable, and Perspective-stage field.
- **Alternate and Strong Surfaces:** Navigation, table header, and selected-region tone shifts.
- **Primary Ink:** Headings and high-priority values.
- **Muted Ink:** Explanations and ordinary row content.
- **Axis Ink:** Pending, disabled, stale-source, and secondary measurement labels.
- **Rule and Soft Rule:** Shell divisions, table cells, row groups, and measurement grids.

**The Truth Color Rule.** Cyan means active computation or inspection. Coral means blocked, stale, degraded, failed, or negative. Lime means healthy, observed, reconciled, authorized, ready, or positive. Never use these accents interchangeably.

**The Persistent Truth Rule.** Feed mode and connection state remain visible in the command bar. Organization, workspace, account, data class, command authority, and operator identity are one disclosure away in labeled controls. Color supports their text labels but never replaces them.

## Typography

**Display Font:** Archivo Variable (with system-ui and sans-serif fallbacks)

**Body Font:** Archivo Variable (with system-ui and sans-serif fallbacks)

**Label/Mono Font:** Recursive Variable (with ui-monospace and monospace fallbacks)

**Character:** Archivo supplies direct explanations and incident-scale unavailable messaging. Recursive turns identifiers, timestamps, quantities, statuses, table headings, and controls into a compact operational instrument with tabular rhythm.

### Hierarchy

- **Unavailable Display** (28px to 44px, 0.98 line height): The only oversized statement, used when no validated snapshot exists.
- **Pane Title** (24px, 1.2 line height): Positions, orders, signals, activity, sources, and primary workspace headings.
- **Section Title** (16px, 1.2 line height): Telemetry, ledgers, order entry, and explorer headings.
- **App Title** (16px, weight 660): Helios Control identity in the persistent command bar.
- **Body** (13px, 1.45 line height): Operational explanations and boundary detail.
- **Metric** (21px, Recursive mono axis): Portfolio facts and compact high-priority numeric state.
- **Data** (11px, Recursive mono axis): Dense tables, identifiers, timestamps, quantities, and event-tape rows.
- **Label** (10px, uppercase Recursive mono axis): Tabs, truth chips, status controls, and ledger labels.
- **Compact Label** (9px, uppercase Recursive mono axis): Filters, compact controls, and supporting measurements.
- **Micro Label** (8px, uppercase Recursive mono axis): Table headers, signal state, and tightly constrained annotations.
- **Nano Label** (7px, uppercase Recursive mono axis): Dense source facts and secondary identifiers only.

**The Operational Mono Rule.** If a value changes, identifies, reconciles, timestamps, measures, or gates operation, set it in Recursive with tabular numerals where applicable.

## Layout

Helios Control owns the viewport. A 64px sticky command bar holds product and tenant identity, Operations, Control, and Explore views, feed state, the alert center, and operator identity. The main atlas is capped at 1920px, with a keyboard-resizable 156px to 264px operations index that defaults to 188px, collapses to 54px, and joins a fluid workspace with one-pixel rules.

The workspace favors horizontal ledgers over card grids. Overview, Positions, Orders, Signals, Activity, and Sources are distinct deep-linked panes. One pane renders at a time. Overview joins five portfolio facts to one telemetry surface. Positions owns value and return. Orders joins a reviewed order ticket to active OMS state and confirmed executions. Signals pairs the candidate list with its inspector. Activity and Sources remain dedicated operational registers.

At 1180px the workspace navigation follows the two-row command bar and order-entry columns stack. At 820px the rail becomes a sticky horizontal tab strip, signal regions stack, and source health moves to one column. At 720px the command bar stops sticking while the operations tabs pin to the top during content scroll. Organization, live state, alert bell, and operator identity remain available without global horizontal overflow.

The body always hides global horizontal overflow. Summary strips, the event tape, lineage, positions, executions, and other wide tables own their overflow. Each interactive scroll region receives `tabindex="0"`, a descriptive accessibility label, and a visible 2px coral focus outline. Narrow layouts show explicit right-arrow copy such as "Scroll for reorder, reduce, and effect."

**The Local Scroll Rule.** Preserve the width that makes an operations table useful, then contain its overflow inside a named, keyboard-focusable region. Never solve density by shrinking text below the established scale or by giving the page a horizontal scrollbar.

## Elevation & Depth

The app is flat and shadow-free. Depth comes from adjacent obsidian tones, one-pixel rules, table header fields, selection washes, and the deepest tape-black analytical stages. The command bar may use an 18px backdrop blur to remain legible while sticky, but it does not cast a shadow.

**The Ruled Depth Rule.** Use rules, tone, and state fill to establish hierarchy. Do not float operational facts in cards or imply priority with shadow.

## Shapes

Shell regions, tables, panels, truth chips, tabs, and ledger rows are square. Circles are restricted to connection and health indicators, the operator avatar, and the alert count. The Helios mark is a crisp 28px orbital instrument: one cyan orbit around an ion-lime source and eight calibrated rays.

**The Instrument Shape Rule.** Square geometry holds information. Small circles report status. Neither form is decorative.

## Components

### Standalone App Shell

- **Structure:** Full-viewport obsidian ground, 64px command bar, resizable and collapsible operations index, and one fluid ruled pane.
- **Identity:** The Helios mark, organization context, alert bell, and operator session appear without VitePress navigation, sidebars, or document controls.
- **Boundary:** The snapshot stream remains read-only. Mutation crosses a separate, same-origin authenticated command service with CSRF protection, idempotency, and sequence preconditions.

### Command Bar and Session Controls

- **Views:** Operations loads by default. Control and Explore remain disabled until a valid snapshot exists.
- **Persistent State:** Mode supports demo, shadow, paper, live, or pending. Connection supports live, frozen, connecting, reconnecting, snapshot, or offline.
- **Disclosure:** The organization control owns workspace, account, and data-class identity. The operator control owns read or command authority and audit identity. The alert bell owns active incident count and immediate investigation actions.
- **Connection Control:** Pause, resume, retry, reconnecting, connecting, and snapshot-only states remain text-labeled with a seven-pixel status dot.

### Read and Command Boundary

- **Normal:** States whether an authenticated command service is attached and includes the current capital-gate reason.
- **Stale:** Adds a coral-tinted ground, names the snapshot stale, and reports the last successful observation time.
- **Sequence:** The validated snapshot sequence remains visible at the opposite edge.

### Protected Command Plane

- **Strategies:** Pause and resume requests are explicit commands, never local state toggles.
- **Stages:** Each transition can be held before entry without mutating the read model in the browser.
- **Orders and Positions:** Cancel and flatten actions require an operational reason and exact typed confirmation.
- **Emergency Stop:** Kill-switch activation stays available as a distinct action and does not imply automatic liquidation.
- **Admission:** Requests carry a CSRF token, idempotency key, and current snapshot sequence. The server owns authorization, risk policy, durability, and side effects.
- **Receipts:** The interface displays validated command receipts and waits for the operations stream to report resulting state.

### Reviewed Order Entry

- **Draft:** Instrument, side, exact fixed-point quantity, market or limit type, optional limit price, time in force, and optional strategy attribution form one typed intent.
- **Review:** Estimated notional, snapshot sequence, operational reason, and exact confirmation remain visible together before submission.
- **Authority:** The public synthetic deployment permits draft and review only. Submit requires the same-origin authenticated command port and a current snapshot.
- **Ownership:** The command service assigns durable order identity, authorizes the operator, runs risk admission, records the intent, and returns the receipt. The browser never inserts an optimistic order.

### Fail-Closed Unavailable State

- **Initial Load:** No validated snapshot means no operational tables and no substituted demo fixture.
- **Message:** Distinguishes an in-progress first connection from a failed source and reports the last successful observation honestly.
- **Recovery:** A coral retry control appears only after error. It cannot authorize capital or issue trading commands.

### Operations Summary

- **Structure:** Five ruled facts for gross exposure, reserved capacity, unrealized result, daily orders, and worst source lag.
- **Typography:** 21px Recursive values, 8px uppercase keys, and 11px explanations.
- **Responsive:** The strip keeps 190px cells below 1180px and 160px cells below 520px, with local scrolling and visible instruction.

### Signal Inspector

- **Selection:** Signal rows expose observing, eligible, and blocked state before instrument, hypothesis, and posterior value.
- **Inspector:** Trigger, availability, decision cut, proposed effect, blocker, trace, and lineage stay joined in one ruled evidence region.

### Dense Operations Tables

- **Positions:** Eleven columns keep quantity, cost, broker mark, market value, open P&L, open return, day P&L, day return, and mark age distinct.
- **Executions:** Nine columns preserve executed time, strategy, venue, liquidity, price, execution identity, instrument, side, and quantity.
- **Rows:** 42px rows use 11px content, 8px uppercase headers, tabular numerals, no wrapping, and one-pixel cell rules.
- **Orders:** A selectable order list remains linked to a reconciliation detail ledger and sits beside reviewed order entry.

### Source Health

- **Structure:** Four source records show name, channel, lag, watermark, and detail.
- **State:** Healthy is lime, degraded is coral, and stale is axis gray. Text always accompanies the status dot.
- **Action:** The only section action opens the read-only Perspective explorer.

### Lazy Perspective Explorer

- **Boundary:** Vue loads the explorer through an async component only after the Data Explorer view is requested.
- **Runtime:** Perspective 5.3 client, server, viewer, datagrid, Pro Dark theme, three WebAssembly assets, and isolated worker stay out of the initial overview path.
- **Data:** Signals, positions, orders, fills, and sources become an entity-bounded table keyed by `row_id`; later snapshots use keyed table updates.
- **Failure:** Loading stages are explicit and timeout-bounded. Worker failure produces a coral error state with retry-by-reload guidance.
- **Budget:** CI caps initial JavaScript at 112 KiB, initial CSS at 40 KiB, and on-demand Perspective assets at 5 MiB. It also rejects eager Perspective WebAssembly or datagrid code.

## Do's and Don'ts

### Do:

- Do keep mode and connection visible as text, with tenant, data, authority, operator, and sequence available in their labeled operational controls.
- Do reject malformed or unsupported snapshots before they replace the last validated state.
- Do show no operational fixture when the configured initial snapshot fails.
- Do mark the last validated snapshot stale when later updates fail, including its observation time.
- Do keep cancel, flatten, strategy, stage, and kill-switch actions outside the read-only operations port.
- Do require reason, typed confirmation, idempotency, CSRF protection, and sequence admission for every command.
- Do preserve wide operational tables inside named, keyboard-focusable scroll regions.
- Do keep Perspective and all related JavaScript and WebAssembly out of the initial overview bundle.
- Do label deterministic fixtures synthetic and observed operations data observed.

### Don't:

- Don't wrap Helios Control in documentation navigation or reuse document-page composition.
- Don't hide feed mode or connection state inside a menu, tooltip, or color-only indicator.
- Don't replace a malformed snapshot with demo data or present stale state as live.
- Don't add command authority to the snapshot or SSE port, and never optimistically mutate its state.
- Don't eagerly import Perspective, its datagrid, themes, worker, or WebAssembly assets.
- Don't create global horizontal overflow to preserve table width.
- Don't use generic dashboard cards, soft rounded containers, decorative gradients, or drop shadows.
- Don't imply that a candidate signal, confirmed fill, or observed feed authorizes live capital.
