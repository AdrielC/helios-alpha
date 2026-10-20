#!/usr/bin/env bash
# Resolve a personal-vault reference only inside the decision runner's process.
set -euo pipefail
set +x

if [[ "${1:-}" == "--help" ]]; then
  cat <<'USAGE'
Run the OpenAI decision worker using 1Password desktop authentication.

Required:
  HELIOS_1PASSWORD_ACCOUNT  Your personal account's sign-in address or account ID.

Optional:
  HELIOS_OPENAI_SECRET_REF  Defaults to op://Personal/Helios OpenAI/credential

Build the native runner first. Pass its arguments, for example --smoke.
The Mac may require 1Password approval even when its vault is unlocked.
USAGE
  exit 0
fi

account="${HELIOS_1PASSWORD_ACCOUNT:-}"
reference="${HELIOS_OPENAI_SECRET_REF:-op://Personal/Helios OpenAI/credential}"
if [[ -z "$account" ]]; then
  printf '%s\n' 'Set HELIOS_1PASSWORD_ACCOUNT to your personal account address or ID.' >&2
  exit 2
fi
if [[ "$reference" != op://Personal/* || "$reference" == *$'\n'* || "$reference" == *$'\r'* ]]; then
  printf '%s\n' 'Expected a secret reference in the Personal vault, not an API key.' >&2
  exit 2
fi

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
binary="$repo_root/rust/target/debug/helios-decision"
if [[ ! -x "$binary" ]]; then
  printf '%s\n' 'Build first: cargo build --locked --manifest-path rust/Cargo.toml -p helio_decision --features native' >&2
  exit 2
fi
if ! op_binary="$(command -v op)"; then
  printf '%s\n' 'Install the official 1Password CLI before running this command.' >&2
  exit 2
fi

# Keep unrelated secret references and service-account tokens out of op run.
# The API key is resolved by op in memory, never written to a file or argument.
cd "$repo_root"
exec /usr/bin/env -i \
  HOME="$HOME" PATH="$PATH" \
  OP_BIOMETRIC_UNLOCK_ENABLED=true \
  OPENAI_API_KEY="$reference" \
  "$op_binary" --account "$account" run -- \
  "$binary" --provider openai "$@"
