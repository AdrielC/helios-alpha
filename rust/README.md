# Rust workspace

```bash
cd rust
cargo test
cargo build --release -p helios_signald   # needs libzmq + C++ toolchain
```

Crates: `crates/helio_scan`, `crates/helio_time`, `crates/helio_window`, `crates/helio_event`, `crates/helios_signald`.

See [docs/HELIO_RUST_WORKSPACE.md](../docs/HELIO_RUST_WORKSPACE.md).
