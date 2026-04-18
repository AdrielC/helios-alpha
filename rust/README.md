# Rust workspace

```bash
cd rust
cargo test
cargo build --release -p helios_signald   # needs libzmq + C++ toolchain
```

Crates: `crates/helio_scan`, `crates/helio_time`, `crates/helio_window`, `crates/helio_event`, `crates/helio_backtest`, `crates/helio_backtest_wasm` (WASM + [Ratzilla](https://github.com/ratatui/ratzilla)), `crates/helios_signald`.

**Backtest harness (native TUI):** `cargo run -p helio_backtest --features tui --bin helio-backtest-tui`  
**tmux:** `bash scripts/helio-backtest-tmux.sh` from repo root  
**WASM:** see `crates/helio_backtest_wasm/README.md` (`trunk serve`).

Toolchain is pinned in `rust/rust-toolchain.toml` (required for Ratatui 0.30 / Ratzilla).

See [docs/HELIO_RUST_WORKSPACE.md](../docs/HELIO_RUST_WORKSPACE.md) and [docs/TIME_AND_WINDOWS.md](../docs/TIME_AND_WINDOWS.md).
