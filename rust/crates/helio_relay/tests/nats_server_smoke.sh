#!/usr/bin/env bash
set -euo pipefail

: "${NATS_SERVER_BIN:?NATS_SERVER_BIN must identify the verified nats-server binary}"

readonly test_root="$(mktemp -d /tmp/helios-nats-integration.XXXXXX)"
readonly nats_port="${HELIOS_NATS_TEST_PORT:-44223}"
readonly server_log="$test_root/nats-server.log"
nats_server_pid=""

cleanup() {
  local -r exit_code=$?
  trap - EXIT
  if [[ -n "$nats_server_pid" ]] && kill -0 "$nats_server_pid" 2>/dev/null; then
    kill -TERM "$nats_server_pid"
    wait "$nats_server_pid" 2>/dev/null || true
  fi
  if (( exit_code != 0 )); then
    tail -100 "$server_log" >&2 || true
  fi
  case "$test_root" in
    /tmp/helios-nats-integration.*) rm -rf -- "$test_root" ;;
    *) echo "Refusing to remove unexpected test path: $test_root" >&2 ;;
  esac
  exit "$exit_code"
}
trap cleanup EXIT

"$NATS_SERVER_BIN" \
  --jetstream \
  --port "$nats_port" \
  --store_dir "$test_root/data" \
  >"$server_log" 2>&1 &
nats_server_pid=$!

for _ in $(seq 1 100); do
  if nc -z 127.0.0.1 "$nats_port" 2>/dev/null; then
    break
  fi
  if ! kill -0 "$nats_server_pid" 2>/dev/null; then
    echo "nats-server exited before readiness" >&2
    exit 1
  fi
  sleep 0.1
done
if ! nc -z 127.0.0.1 "$nats_port" 2>/dev/null; then
  echo "timed out waiting for nats-server" >&2
  exit 1
fi

HELIOS_NATS_TEST_URL="nats://127.0.0.1:$nats_port" \
  cargo test -p helio_relay --features native-nats \
    --test jetstream_smoke -- --ignored --exact \
    real_jetstream_acknowledges_and_deduplicates_stable_event_identity

echo "NATS JetStream smoke passed: persisted publish acknowledgement and stable message-ID de-duplication."
