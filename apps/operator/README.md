# Helios Control

Helios Control is the standalone operations application for Helios Alpha. It is not part of the
documentation site and it never holds broker credentials. The read model and protected command
service remain separate ports.

The default application uses a deterministic synthetic source so the interface can be developed
without implying that market or space-weather observations are live. Every screen keeps the mode,
capital gate, and data classification visible.

Production demo: [helios-control-kappa.vercel.app](https://helios-control-kappa.vercel.app/)

## Run it

From the repository root:

```bash
npm ci
npm run operator:dev
```

The development server listens on `http://127.0.0.1:4174`.

Build and inspect the production artifact:

```bash
npm run operator:build
npm run operator:check-performance
npm run operator:preview
```

The static artifact is written to `apps/operator/dist` and can be deployed independently from the
VitePress output.

## Connect an operations service

The deployment writes `public/runtime-config.js` without rebuilding the application:

```js
window.__HELIOS_OPERATIONS__ = {
  snapshotUrl: "/api/operations/v1/snapshot",
  streamUrl: "/api/operations/v1/events",
  commandSessionUrl: "/api/commands/v1/session",
  commandUrl: "/api/commands/v1/commands",
};
```

Both URLs must be same-origin. Snapshot reads use same-origin credentials. The optional stream is
an SSE channel whose `snapshot` events contain complete, versioned operations snapshots. Incoming
payloads are validated before replacing the last known state. Initial load failure shows no demo
data. Later stream failure preserves the last validated observation only after marking it stale.

The read model owns:

- candidate signals, posterior state, blockers, lineage, and decision cuts;
- held positions and broker marks;
- active orders and reconciliation state;
- source watermarks, lag, and health;
- exposure, capacity, incident, kill-switch, and capital-admission state.

Mutation belongs to a separate authenticated command service. Do not extend the operations port
with mutation methods. Without both command URLs the UI stays visibly unbound and every control is
disabled.

The command session endpoint returns a short-lived operator identity, expiry, and CSRF token. The
browser keeps that token in memory only. Each command request carries same-origin credentials,
the CSRF token, a unique idempotency key, and the reviewed snapshot sequence in both `If-Match`
and the request body. The UI requires an operational reason and exact typed confirmation, then
waits for a validated receipt. It never optimistically changes a position, order, strategy, or
kill-switch state.

The command service must independently authenticate the operator, authorize the action, validate
the confirmation phrase, reject stale snapshot sequences, enforce risk policy, durably record the
intent before side effects, and return the same receipt for a repeated idempotency key.

## Deployment boundary

Deploy this application on a dedicated origin behind the organization identity proxy. A production
deployment should provide at least:

- authenticated access before static assets and APIs are served;
- `frame-ancestors 'none'`, `object-src 'none'`, and a restrictive `connect-src` policy;
- `worker-src 'self' blob:` and WebAssembly support for the on-demand Perspective worker;
- no broker secrets, signing material, or raw venue credentials in runtime configuration;
- immutable caching for hashed assets and no-cache delivery for `runtime-config.js`;
- request IDs and deploy versions on snapshot and stream responses;
- a health endpoint that proves both the static release and read-model service are current.

Perspective 5.3 is an analytical workbench, not the initial dashboard. Its JavaScript and WebAssembly
load only after the operator opens Data Explorer. CI rejects an initial bundle above its budget or
an eager Perspective payload.
