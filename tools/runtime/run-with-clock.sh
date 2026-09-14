#!/bin/sh
set -eu
test "$#" -gt 0
if [ -n "${LAYERX_RUNTIME_CLOCK_SOCKET:-}" ]; then
    exec "$@"
fi
repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
if [ -n "${LAYERX_RUNTIME_CLOCK_BIN:-}" ]; then
    clock=$LAYERX_RUNTIME_CLOCK_BIN
else
    clock_target=${CARGO_TARGET_DIR:-$repo_root/.lane-target}
    CARGO_BUILD_JOBS=4 cargo build --locked --manifest-path "$repo_root/platform/Cargo.toml" \
        --target-dir "$clock_target" -p layerx-runtime-clock --bin layerx-runtime-clock
    clock=$clock_target/debug/layerx-runtime-clock
fi
test -x "$clock"
exec "$clock" --runtime-dir "${LAYERX_RUNTIME_CLOCK_DIRECTORY:-/tmp}" -- "$@"
