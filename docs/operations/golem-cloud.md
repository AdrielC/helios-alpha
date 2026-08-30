# Durable hypothesis execution on Golem

## Role

Golem owns durable hypothesis shards, conditional model workflows, scheduled wakeups, and risk
workflow coordination. Native services still own the lowest-latency feed, order-book, and colocated
execution paths.

The boundary is backed by Golem 1.5 documentation and source reviewed on August 29, 2026, plus an
executable durability proof. The portable Helios crates compile for Golem's
`wasm32-wasip2` target. A real Golem Rust agent now owns one durable hypothesis shard, restores a
versioned custom snapshot, and processes source batches through generated typed interfaces.

### Boundary match

- A Golem agent is a durable stateful unit with sequential invocation processing, which matches one
  owner per hypothesis partition.
- Constructor parameters define agent identity, which maps cleanly to strategy fingerprint, source
  partition, and shard.
- Agent invocations carry idempotency keys and agent-to-agent calls are durable.
- Crashes recover from the operation log, while explicit periodic snapshots bound recovery time.
- Scheduled invocations can wake an idle shard for a deadline.
- Rust agents target WASI Preview 2, and the domain-free Helios core already compiles for it.

### Runtime constraints

- One agent processes invocations sequentially. Throughput comes from partitioning across agents.
- Rust components cannot use native threads, `std::net`, native system calls, or C libraries.
- Outgoing HTTP uses WASI HTTP, not Tokio, `reqwest`, or a native socket stack.
- Golem durability does not make a broker cooperative. Orders still require a domain idempotency
  key, broker reconciliation, and an independently enforced risk boundary.

Primary references: [Golem concepts](https://learn.golem.cloud/v1.5/concepts),
[Rust agent constraints](https://learn.golem.cloud/v1.5/how-to-guides/rust/golem-add-agent-rust),
[Rust WASI dependencies](https://learn.golem.cloud/v1.5/how-to-guides/rust/golem-add-rust-crate),
[durability controls](https://learn.golem.cloud/v1.5/develop/durability), and
[Rust snapshots](https://learn.golem.cloud/v1.5/how-to-guides/rust/golem-custom-snapshot-rust).

## Implemented boundary

The repository has two deliberately separate layers:

- `helio_golem` is a portable adapter kernel. It owns source-offset validation, bounded atomic
  batches, deterministic invocation keys, and validated shard snapshots. It has no Golem SDK,
  trading, or event-shock types.
- `golem/` is the deployable application. It supplies Golem `Schema` wire types and a concrete
  event-shock reference model that moves from trigger prior, through a Bayesian likelihood update,
  to a market assessment and research candidate.

The agent exposes exactly three operations:

```text
HypothesisShardAgent(fingerprint, source, partition, shard, initial_offset)
  invocation_key(first_offset, last_offset) -> Result<String, AgentError>
  process_batch(batch) -> Result<ProcessReceipt, AgentError>
  status() -> Result<ShardStatus, AgentError>
```

The component chooses `snapshotting = "periodic(30s)"` explicitly and implements custom async
snapshot save and load hooks. Constructor arguments form the durable Golem agent identity. The
logical Helios shard identity excludes `initial_offset`; that value is still bound into the Golem
identity and snapshot envelope so a restore cannot silently move to a different source prefix.

### Run the durability proof

```bash
cd rust
cargo test -p helio_golem
cargo check --target wasm32-wasip2 -p helio_golem --no-default-features

cd ../golem
cargo test
cargo clippy --target wasm32-wasip2 --all-targets -- -D warnings
golem build --yes
bash tests/golem_local_smoke.sh
```

The smoke test starts a fresh Golem server with an isolated data directory, deploys the component,
and proves all of the following:

1. Offset 40 commits exactly once and advances the shard to 41.
2. Repeating the same length-prefixed invocation key returns the original receipt without a second
   transition.
3. A simulated agent crash preserves the hypothesis, source cursor, and deadline.
4. A complete server stop and restart against the same data directory preserves the same state.
5. Replaying the original key after restart still leaves the cursor at 41.
6. Offset 41 resumes the Bayesian chain, produces posterior `487429` parts per million, replaces
   the deadline, and advances to 42.

The local operation log contained one `process_batch` invocation for the repeated key and a
575-byte snapshot. CI repeats the proof on Linux with Golem v1.5.9, verifies the downloaded CLI's
SHA-256 digest, and builds the metadata-enriched WASM component with `golem build`.

This follows Golem's own open-source worker-executor test pattern: invoke with one idempotency key,
restart the executor, repeat the key, and assert the effect happened once. See the pinned
[upstream test source](https://github.com/golemcloud/golem/blob/0bf0e7d2b65f9d6e36a8112757e7ca742945aa3f/golem-worker-executor/tests/api.rs)
and Golem's [integration-test setup guide](https://learn.golem.cloud/v1.5/how-to-guides/common/golem-integration-test-setup).

The smoke server uses Golem's built-in local management port 9881 because that profile owns local
authentication. Its data, configuration, custom-request port, and MCP port are isolated per run. A
management-port collision fails the test instead of connecting to an existing developer server.

## The hybrid system

```text
venue and event feeds
        │
        ▼
native ingress and normalization
        │  raw immutable records
        ▼
partitioned durable log
        │  source / partition / offset
        ▼
Golem partition router
        │  invocation id = source:partition:start:end
        ▼
HypothesisShardAgent(strategy fingerprint, source, partition, shard)
        │
        ├── typed model requests ──► ModelAgent or mediated WASI HTTP
        │                                  │
        │                                  └── correlated evidence response
        │
        ├── research candidate ──► RiskAuthorityAgent(account, strategy group)
        │                                  │
        │                                  ▼
        │                           OrderGateway façade
        │                                  │
        │                                  ▼
        │                           broker and reconciliation
        │
        └── oplog telemetry ──► OTLP and cold analytical store
```

The native ingress exists for feed protocols, burst absorption, and timestamp discipline. It writes
an immutable source log before Golem sees a record. A Golem failure therefore cannot erase market
or event input, and replay has an authoritative prefix.

The Golem layer begins at deterministic partition routing. It receives normalized batches rather
than individual high-rate ticks. Low-volume rare-event sources can invoke it directly, but the
partition contract remains identical.

## Agent boundaries

### `HypothesisShardAgent`

Identity:

```text
strategy fingerprint + source ID + source partition + logical shard
```

State:

- `HypothesisEngine<K, Model, Reason>`
- exact next source offset
- accepted availability frontier
- versioned, validated engine state
- deterministic model-output identities

Methods:

```text
process_batch(records with contiguous source offsets) -> ProcessReceipt
status() -> ShardStatus
invocation_key(first_offset, last_offset) -> String
```

`process_batch` rejects gaps, overlaps, fingerprint mismatches, and batches that exceed configured
limits as domain errors. Panics are reserved for infrastructure failures because Golem retries
uncaught failures and can eventually mark an agent failed.

One shard owns many hypothesis keys. This preserves the runtime's deterministic global
availability order and amortizes invocation cost. Shard count is a capacity decision recorded in
the strategy fingerprint. A key is routed consistently for its entire lifecycle.

### `ModelAgent`

Expensive physics, ML, enrichment, or Bayesian fitting can run in separate agents so one hypothesis
shard does not block on CPU work. The request identity is:

```text
strategy fingerprint + hypothesis key + revision + effect index
```

Internal agent calls use generated typed clients. External services use mediated WASI HTTP. The
response returns to the shard as `CausalEvidence`, with `available_at` set to the first instant the
response became available to the system.

Avoid synchronous RPC cycles. A model agent should return directly or trigger a one-way correlated
response. It must never await a shard that is waiting on it.

### `RiskAuthorityAgent`

A hypothesis output is a research candidate, not an order. A separate risk agent owns current
positions, reservations, account limits, strategy limits, stale-data policy, and kill-switch state.
Its sequential processing is useful for account-level serialization, but accounts or strategy
groups must be partitioned when one agent becomes a bottleneck.

The risk agent produces either a typed rejection or an `OrderIntent` with a stable
`client_order_id`. It does not accept arbitrary order commands from the research component.

### `OrderGateway`

The gateway is an external idempotency façade in front of the broker. It stores each
`client_order_id` under a unique constraint before transmission, records the broker request and
response, and reconciles ambiguous timeouts against broker state before retrying.

Golem can generate a stable idempotency key for a repeated external effect, but exactly-once order
behavior still depends on the endpoint honoring that identity. The façade supplies that contract
when the broker does not. HTTP 200 with a semantic rejection is a domain result, not a retryable
transport failure.

## Offset, invocation, and effect identities

Every identity is deterministic and independently auditable:

| Boundary | Identity |
|---|---|
| Source batch | `source:partition:first_offset:last_offset` |
| Golem invocation | the exact source batch identity |
| Hypothesis mutation | `hypothesis_key:sequence:revision` |
| Model request | `fingerprint:key:revision:effect_index` |
| Research candidate | `fingerprint:key:revision:output_index` |
| Risk decision | `account:candidate_id:risk_policy_version` |
| Order intent | `account:risk_decision_id:leg_index` |

Retries reuse the same identity. A random key created during a retry is a correctness bug.

## Deadlines and watermarks

The hypothesis machine uses availability-time deadlines. Golem scheduled invocations are durable,
but a wall-clock wakeup is not automatically a source watermark.

The current component represents a frontier advance as a source mutation inside `process_batch`.
The partition router may emit that mutation only after it knows every source record through the
availability cut has been admitted. This preserves the tie rule where evidence available exactly
at a deadline may cancel it before the frontier passes.

Phase two may schedule one durable wakeup at `HypothesisState::next_timer_at()`. On wake, the shard
requests or verifies the source cut before advancing. Duplicate schedules reuse a deterministic
idempotency key based on shard identity and deadline. The scheduled call never guesses that a lagging
feed is complete.

See Golem's [scheduled invocation contract](https://learn.golem.cloud/v1.5/how-to-guides/rust/golem-schedule-agent-rust).

## Snapshots and upgrades

Golem's operation log is the primary recovery mechanism. Helios snapshots provide a compact,
validated state boundary for fast recovery and schema migration.

The Golem adapter implements custom snapshot hooks with a versioned envelope:

```text
AgentSnapshotV1
  format_version
  initial_offset
  DurableShardSnapshotV1
    shard identity
    max batch size
    next source offset
    hypothesis snapshot
```

`save_snapshot` serializes the envelope. `load_snapshot` checks its format, constructor-bound
initial offset, identity, batch capacity, and hypothesis invariants before the agent resumes.

Start with explicit `snapshotting = "periodic(30s)"`, then tune from operation-log growth and measured
recovery time. A high-frequency shard should not snapshot every record. Golem documents that the
server default may leave snapshotting disabled, so the component must choose a policy explicitly.

Compatible code changes can use automatic operation-log replay. Changes to exported methods,
state layout, ordering, or statistical semantics require manual snapshot migration or a new agent
identity with a new strategy fingerprint. Never update a running trading shard with a
replay-incompatible algorithm and assume the platform can infer the migration.

## Failure matrix

| Failure | Required result |
|---|---|
| Router retries a batch | same invocation key, no second state transition |
| Shard crashes during pure transition | Golem recovery repeats safely from its log |
| Shard crashes after internal candidate send | durable agent call is not duplicated |
| Model response arrives twice | exact evidence sequence or request identity rejects the duplicate |
| Scheduled deadline is enqueued twice | deterministic schedule identity deduplicates it |
| Broker request times out | gateway reconciles by `client_order_id` before any retry |
| Snapshot is corrupt or incompatible | restore fails closed and shard remains unavailable |
| New component changes semantics | side-by-side version or explicit snapshot migration |
| Risk service is stale or unavailable | no order intent is emitted |
| Source watermark regresses | typed rejection, alert, no state mutation |

## Admission gates

### Gate 0: WASI compatibility, complete

- Confirm the Golem CLI and `wasm32-wasip2` target are available.
- Compile the portable Helios substrate for WASI Preview 2.
- Keep Tokio and ZMQ adapters out of the Golem component.
- Gate WASI compatibility in CI.

### Gate 1: component adapter, complete

- Create a separate Golem application component that depends on `helio_hypothesis` without the
  `service` feature.
- Define Golem `Schema` transport types instead of exporting internal Rust enums directly.
- Implement `HypothesisShardAgent`, batch offset validation, receipts, and custom snapshot hooks.
- Build only with `golem build`, as required by the Golem Rust toolchain.

Exit proof: component build passes, snapshot round-trip restores identical future events, and a
repeated invocation key does not advance the sequence twice.

### Gate 2A: restart and duplicate proof, complete

- Build the generated Golem component for WASI Preview 2.
- Repeat an invocation key before and after an agent crash.
- Stop and restart the complete Golem server against the same isolated data directory.
- Resume from the exact next source offset and compare the Bayesian result.
- Run this proof in required CI with a pinned and digest-verified Golem CLI.

Exit proof: duplicate suppression, simulated crash recovery, full server restart, snapshot restore,
and contiguous source resume all produce the expected state.

### Gate 2B: expanded crash and upgrade proof

- Run deterministic input through native and Golem hosts and compare every event identity.
- Kill a shard before transition, after transition, during a model call, and after candidate send.
- Exercise operation-log replay, periodic snapshot recovery, corrupt snapshots, and manual upgrade.
- Measure invocation throughput, batch-size curve, operation-log growth, snapshot size, and recovery
  time.

Exit proof: every crash point produces the same terminal state and effect set as uninterrupted
execution.

### Gate 3: shadow event system

- Connect the immutable source log and partition router.
- Run real-time data in shadow mode with no order capability granted.
- Export source lag, invocation latency, active keys, earliest deadline, rejection counts,
  operation-log growth, snapshot age, recovery time, and candidate comparison telemetry.
- Reconcile native replay, Golem replay, and warehouse records daily.

Exit proof: a sustained shadow window meets latency and recovery SLOs with zero unexplained output
divergence.

### Gate 4: paper risk and execution

- Deploy `RiskAuthorityAgent` with least-privilege capabilities.
- Put the idempotent order gateway in front of a paper broker.
- Drill ambiguous broker timeouts, duplicate callbacks, partial fills, kill switch, and stale market
  data.

Exit proof: every simulated order traces back to one source prefix, one candidate, one risk decision,
and one reconciled broker identity.

### Gate 5: capital canary

- Require independent operational approval and a tiny predeclared risk envelope.
- Run one strategy version and one account partition first.
- Keep automatic rollback limited to software health. Risk and position state must reconcile before
  resuming after any execution incident.

Golem is approved for an implementation spike now. It is approved for live capital only after the
shadow, crash, risk, and broker-reconciliation gates above have durable evidence.
