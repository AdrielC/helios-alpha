# Restart a pipeline

A restart is correct only when operator state and source position describe the same processed prefix. Serializing a struct is necessary, but it is not sufficient.

## What a checkpoint contains

Helios checkpoints carry:

- A versioned operator snapshot.
- The source offset to resume from.
- An optional event-time watermark.
- A format version.
- An optional pipeline fingerprint and label.

`read_and_restore_checkpoint` validates metadata first, then invokes the scan's fallible restore path so corrupt or impossible states fail closed.

## Commit order

For an at-least-once source with an idempotent sink:

```text
read input at offset N
        ↓
update scan state and stage outputs
        ↓
write checkpoint(state, N, watermark)
        ↓
publish outputs with idempotency key
        ↓
commit source offset N
```

This sequence is not exactly-once by itself. A crash between steps can replay an output. Exactly-once behavior requires a shared transaction or a sink that deduplicates a stable output identity.

## Compatibility gate

```rust
use helio_scan::{CheckpointRequirements, read_and_restore_checkpoint};

let restored = read_and_restore_checkpoint(
    &pipeline,
    &mut store,
    &"strategy/main",
    CheckpointRequirements {
        format_version: 1,
        snapshot_version: Some(1),
        pipeline_fingerprint: Some("strategy-v7"),
    },
)?;
```

If the fingerprint changes, decide explicitly whether to reject the checkpoint, migrate it, or replay from an earlier durable boundary. Never guess that two configurations are state-compatible.

## Stop behavior

A controlled stop should:

1. Stop accepting new source records.
2. Drain in-flight records in source order.
3. Apply the intended flush reason.
4. Persist state and its matching offset.
5. Commit externally visible work according to the sink protocol.

Use `Shutdown`, `Checkpoint`, `Watermark`, and `EndOfInput` as distinct controls. They are not interchangeable, and scans are allowed to react differently to each.
