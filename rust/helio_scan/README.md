# helio_scan

Composable **scan machines** over ordered streams: step with multi-emission, optional **flush**, **snapshot/restore**, and **checkpoint + offset** hooks.

## Full documentation

See the repository guide: [docs/HELIO_SCAN.md](../../docs/HELIO_SCAN.md).

## Quick start

From the **workspace root** `rust/`:

```bash
cargo test -p helio_scan
cargo doc -p helio_scan --no-deps --open
```

## Workspace

This crate is a member of the Cargo workspace defined in `../Cargo.toml` (`helio_scan` + `helios_signald`). The workspace uses `default-members = ["helio_scan"]` so `cargo test` in `rust/` does not require building the ZMQ binary.

## Design slogan

Scans are restartable, flushable, causality-aware state machines over ordered streams. Composition preserves structure. State is inspectable, snapshotable, and resumable by offset.
