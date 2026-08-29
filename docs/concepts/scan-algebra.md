# Scan algebra

`Scan` models an ordered state machine. One input updates owned state and may emit zero, one, or many outputs through a caller-supplied sink.

```rust
pub trait Scan {
    type In;
    type Out;
    type State;

    fn init(&self) -> Self::State;
    fn step<E: Emit<Self::Out>>(
        &self,
        state: &mut Self::State,
        input: Self::In,
        emit: &mut E,
    );
}
```

## Why the emit sink matters

Returning `Vec<Out>` from every step is convenient but allocates in the hot path. `Emit<T>` preserves the general zero-to-many contract while allowing a runner, a downstream scan, or a test collector to receive values without forcing per-step allocation.

`VecEmitter` remains useful in tests. Production code can supply a direct sink.

## Static composition

- `then` routes every upstream emission into a downstream scan.
- `and` or `ZipInput` fans one input into two scans.
- `Map`, `FilterMap`, and `EmitWhen` adapt outputs without changing the kernel.
- Arrow-style combinators handle pair and `Either` inputs.

Composed state uses named types such as `ThenState`, not anonymous nested tuples. This makes the restart surface inspectable and lets tests focus typed paths into child state.

## Controls are not domain input

`FlushableScan` separates watermarks, checkpoints, session closes, shutdown, and end-of-input from ordinary observations. This avoids teaching every input type about operational boundaries.

## Laws worth testing

For a deterministic scan and a fixed input order:

- Incremental stepping equals the opaque batch adapter.
- Snapshot then restore preserves all future outputs.
- Composition preserves upstream emission order.
- Rejected input does not partially mutate state unless the contract says otherwise.
- Flush order is observable and therefore tested.

See [the full kernel design](../HELIO_SCAN) for the complete public surface.
