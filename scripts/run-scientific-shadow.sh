#!/usr/bin/env bash
# Run live scientific observations and the local read-only operator together.
set -euo pipefail

cd "$(dirname "$0")/.."
repo_root="$PWD"

# Homebrew installs Rust's command proxies outside its normal bin directory.
if ! command -v cargo >/dev/null 2>&1 && command -v brew >/dev/null 2>&1; then
  export PATH="$(brew --prefix rustup)/bin:$PATH"
fi
for tool in uv npm cargo; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    printf 'Missing required tool: %s\n' "$tool" >&2
    exit 1
  fi
done

if [[ "${HELIOS_REUSE_BUILD:-0}" != "1" ]]; then
  uv sync --frozen --python 3.12 --extra dev --extra execution
  npm ci
  npm run operator:build
  (
    cd rust
    cargo build --locked -p helio_operatord
  )
fi

shadow_args=(
  --journal "$repo_root/data/shadow/space-weather.sqlite3"
  --operator-projection "$repo_root/data/shadow/operator-projection.json"
)
# A scientific provider outage must not prevent broker holdings from opening.
# Reuse the existing projection on restart; the gateway validates it and the
# scientific collector continues to report each source's own freshness/errors.
if [[ ! -s "$repo_root/data/shadow/operator-projection.json" ]]; then
  if ! uv run --no-sync helios-shadow "${shadow_args[@]}"; then
    if [[ ! -s "$repo_root/data/shadow/operator-projection.json" ]]; then
      printf 'No scientific projection is available; initial collection failed.\n' >&2
      exit 1
    fi
    printf 'Some scientific sources are unavailable; starting with the validated projection.\n' >&2
  fi
fi

shadow_pid=""
gateway_pid=""
broker_sync_pid=""
cleanup() {
  trap - EXIT INT TERM
  for child in "$gateway_pid" "$shadow_pid" "$broker_sync_pid"; do
    if [[ -n "$child" ]]; then
      kill "$child" 2>/dev/null || true
    fi
  done
  # An open browser SSE stream can keep the read-only gateway draining forever.
  # Bound shutdown, then reap only the children started by this launcher.
  for _ in {1..50}; do
    alive=0
    for child in "$gateway_pid" "$shadow_pid" "$broker_sync_pid"; do
      if [[ -n "$child" ]] && kill -0 "$child" 2>/dev/null; then alive=1; fi
    done
    [[ "$alive" == "0" ]] && break
    sleep 0.1
  done
  for child in "$gateway_pid" "$shadow_pid" "$broker_sync_pid"; do
    if [[ -n "$child" ]]; then
      kill -KILL "$child" 2>/dev/null || true
      wait "$child" 2>/dev/null || true
    fi
  done
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

uv run --no-sync helios-shadow "${shadow_args[@]}" --interval-seconds 60 &
shadow_pid=$!

# The monitor has its own persisted OAuth grant and only calls broker read tools.
# Credentials and the explicit import-role configuration stay outside the repo.
if [[ -f "$HOME/.local/share/helios-alpha/private/broker-sync-config.json" ]]; then
  "$repo_root/.venv/bin/python" -m helios_alpha.control.broker_sync &
  broker_sync_pid=$!
fi

# This entry point supplies observations only; broker execution is disabled.
env -u HELIOS_OPERATOR_SESSION_TOKEN -u HELIOS_COMMAND_CSRF_SECRET \
  -u HELIOS_SNAPSHOT_PATH \
  HELIOS_ALPACA_PAPER_ENABLED=0 \
  HELIOS_GOLEM_ENABLED=0 \
  HELIOS_BIND=127.0.0.1:8080 \
  HELIOS_STATIC_DIR="$repo_root/apps/operator/dist" \
  HELIOS_TIME_SERIES_PROJECTION_PATH="$repo_root/data/shadow/operator-projection.json" \
  HELIOS_TIME_SERIES_PROJECTION_POLL_MS=1000 \
  "$repo_root/rust/target/debug/helio_operatord" &
gateway_pid=$!

printf 'Scientific shadow operator: http://127.0.0.1:8080\n'
wait "$gateway_pid"
