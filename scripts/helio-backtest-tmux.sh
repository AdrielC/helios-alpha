#!/usr/bin/env bash
# Run the native Ratatui backtest harness inside tmux (detach with Ctrl-b d).
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT/rust"
exec tmux new-session -A -s helio-backtest \
  "cargo run -p helio_backtest --features tui --bin helio-backtest-tui"
