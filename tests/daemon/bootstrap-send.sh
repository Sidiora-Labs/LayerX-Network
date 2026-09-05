#!/usr/bin/env bash
set -euo pipefail
repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$repo_root"
[[ $(id -u) == 0 ]] || { echo 'bootstrap-send requires root for the distinct daemon uid' >&2; exit 1; }
[[ -x build/bin/layerxd && -x build/bin/layerx-genesis-build ]] || {
    echo 'build layerxd and layerx-genesis-build before bootstrap-send' >&2
    exit 1
}
cargo_command=${PLATFORM_CARGO:-cargo}
for log_mode in absent empty; do
    echo "bootstrap-send: genesis, replica, sequencer, signed SEND and receipt proofs; $log_mode logs"
    LAYERX_TEST_BOOTSTRAP_LOG_MODE=$log_mode "$cargo_command" test \
        --offline --manifest-path platform/Cargo.toml --locked \
        -p layerx-platform-authority --test real_node \
        real_node_authority_serves_verified_facts_and_reflects_replica_loss \
        -- --exact --nocapture --test-threads=1
done
