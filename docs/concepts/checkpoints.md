# Checkpoints

A checkpoint is a compatibility boundary, not a blob of bytes.

## Runtime state and serialized state may differ

`SnapshottingScan` separates the in-memory `State` from the stable `Snapshot`. This lets an implementation use caches or data structures that should not become a persistence contract.

`FallibleRestoreScan` validates externally loaded state before it re-enters the hot path. The statistics and ordered-bucket primitives reject impossible counts, non-finite values, over-capacity queues, duplicate arrival sequence numbers, and watermark inconsistencies.

## Metadata

`CheckpointMeta` currently carries:

- Format version.
- Snapshot version.
- Pipeline fingerprint.
- Optional operator label.

The checkpoint also owns a source offset and optional watermark.

## Fingerprint inputs

A useful fingerprint normally includes:

- Operator graph and configuration.
- Event-time and bucket semantics.
- Statistical parameterization.
- Floating-point partition and merge-order policy.
- Strategy policy version.
- Schema versions for externally decoded input.

## Exactly-once boundary

`write_checkpoint` proves that a store accepted a value. It does not make source offsets, emitted signals, broker orders, and state one transaction. Coordinate those systems explicitly or use idempotent output identities.

See [Restart a pipeline](../guide/restart-a-pipeline) for the operational sequence.
