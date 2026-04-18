# helio_backtest_wasm

Browser build of the **helio_backtest** harness using [Ratzilla](https://github.com/ratatui/ratzilla) (Ratatui + WASM).

## Prerequisites

- Rust **1.88** (see `../rust-toolchain.toml` in the `rust/` workspace).
- Target: `rustup target add wasm32-unknown-unknown`
- [Trunk](https://trunkrs.dev): `cargo install trunk`

## Run locally

```bash
cd rust/crates/helio_backtest_wasm
trunk serve
```

Open the URL Trunk prints (usually http://127.0.0.1:8080). **Space** runs the harness, **w** toggles wall clock, **f** bumps the fixed clock anchor.

## Release bundle

```bash
trunk build --release
```

Static files land in `dist/`.
