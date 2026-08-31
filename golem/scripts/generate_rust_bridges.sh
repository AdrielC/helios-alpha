#!/usr/bin/env bash
set -euo pipefail

readonly script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly app_dir="$(cd -- "$script_dir/.." && pwd)"
readonly output_root="$app_dir/bridge-sdk/rust"
readonly golem_bin="${GOLEM_BIN:-golem}"
readonly golem_sdk_tag="v1.5.9"

mkdir -p "$output_root"

"$golem_bin" generate-bridge --yes --language rust \
  --agent-type-name OmsAccountAgent \
  --output-dir "$output_root"

"$golem_bin" generate-bridge --yes --language rust \
  --agent-type-name ProjectionCursorAgent \
  --output-dir "$output_root"

"$golem_bin" generate-bridge --yes --language rust \
  --agent-type-name RiskAccountAgent \
  --output-dir "$output_root"

for manifest in \
  "$output_root/oms-account-agent-client/Cargo.toml" \
  "$output_root/projection-cursor-agent-client/Cargo.toml" \
  "$output_root/risk-account-agent-client/Cargo.toml"
do
  perl -pi -e "s/branch = \"main\"/tag = \"$golem_sdk_tag\"/g" "$manifest"
  if grep -q 'branch = "main"' "$manifest"; then
    echo "Generated bridge still contains an unpinned Golem dependency: $manifest" >&2
    exit 1
  fi

  client_dir="$(dirname -- "$manifest")"
  if [[ ! -s "$client_dir/src/lib.rs" ]]; then
    echo "Generated bridge is missing its Rust client source: $client_dir/src/lib.rs" >&2
    exit 1
  fi
done
