#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
scratch_root=$(mktemp -d)
trap 'rm -rf -- "$scratch_root"' EXIT

target_dir="$repo_root/platform/target/production-credential-refusal"
CARGO_TARGET_DIR="$target_dir" cargo build \
  --offline \
  --locked \
  --manifest-path "$repo_root/platform/Cargo.toml" \
  --package layerx-platform-cli \
  --no-default-features

output="$scratch_root/output"
config="$scratch_root/config.json"
if LAYERX_CONFIG="$config" \
  LAYERX_CREDENTIAL_STORE=mock \
  "$target_dir/debug/layerx" --json key create production-refusal \
  >"$output" 2>&1; then
  echo "production CLI accepted a test-only credential-store override" >&2
  exit 1
fi

grep -F 'credential store override mock is unavailable in this binary' "$output" >/dev/null
test ! -e "$config"
