#!/usr/bin/env bash
set -euo pipefail
repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$repo_root"
[[ $(id -u) == 0 ]] || { echo 'protocol2-restart requires root for the distinct daemon uid' >&2; exit 1; }
[[ -x build/bin/layerxd && -x build/bin/layerx-genesis-build ]] || {
    echo 'build layerxd and layerx-genesis-build before protocol2-restart' >&2
    exit 1
}
cargo_command=${AGENT_CARGO:-cargo}
"$cargo_command" test --offline --manifest-path agent/Cargo.toml --locked \
    -p layerx-agentd --test protocol2_restart \
    protocol2_recovers_identical_signed_program_call_evidence \
    -- --exact --nocapture --test-threads=1
