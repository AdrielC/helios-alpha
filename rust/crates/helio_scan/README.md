# helio_scan

Composable **scan machines** over ordered streams: step with multi-emission, optional **flush**, **snapshot/restore**, and **checkpoint + offset** hooks.

## Full documentation

See the repository guide: [docs/HELIO_SCAN.md](../../../docs/HELIO_SCAN.md) and [docs/HELIO_RUST_WORKSPACE.md](../../../docs/HELIO_RUST_WORKSPACE.md).

## Quick start

From the **workspace root** `rust/`:

```bash
cargo test -p helio_scan
cargo run -p helio_scan --example arrow_pipeline
cargo doc -p helio_scan --no-deps --open
```

Arrow-style combinators (`Arr`, `Split`, `Merge`, `Choose`, `Fanin`, `First`, `Second`) and the [`scan_then!`](https://docs.rs/helio_scan/latest/helio_scan/macro.scan_then.html) macro live in the crate root re-exports; see `examples/arrow_pipeline.rs`.

## Workspace

This crate lives under `rust/crates/helio_scan` in the workspace defined in `../../Cargo.toml`. Default members include this crate but not `helios_signald`, so `cargo test` in `rust/` does not require ZMQ.

## Design slogan

Scans are restartable, flushable, causality-aware state machines over ordered streams. Composition preserves structure. State is inspectable, snapshotable, and resumable by offset.

## Stability

Prefer **proving** the existing traits on real workloads over adding new kernel traits. Runners (`run_iter`, `run_batch`, `run_receiver`, optional `run_stream`) stay **adapters**; transports do not belong on `Scan`.
