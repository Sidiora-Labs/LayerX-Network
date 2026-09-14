#!/bin/sh
set -eu
if [ "$#" -ne 0 ]; then
    echo "native managed limit gate takes no arguments" >&2
    exit 2
fi
repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
cd "$repo_root"
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$repo_root/.lane-target}
export LAYERX_TEST_NATIVE_BUDGET_CLIENT=${LAYERX_TEST_NATIVE_MANAGED_CLIENT:-$CARGO_TARGET_DIR/debug/examples/native_managed_limit}
export LAYERX_TEST_MANAGED_AUTHORITY_BIN=${LAYERX_TEST_MANAGED_AUTHORITY_BIN:-$CARGO_TARGET_DIR/debug/layerx-receipt-authority}
export LAYERX_TEST_MANAGED_IDENTITY_BIN=${LAYERX_TEST_MANAGED_IDENTITY_BIN:-$CARGO_TARGET_DIR/debug/layerx-human-identity-provider}
export LAYERX_TEST_NATIVE_BUILD_DIR=${LAYERX_TEST_NATIVE_BUILD_DIR:-$repo_root/build}
for executable in "$LAYERX_TEST_NATIVE_BUDGET_CLIENT" "$LAYERX_TEST_MANAGED_AUTHORITY_BIN" "$LAYERX_TEST_MANAGED_IDENTITY_BIN" \
    "$LAYERX_TEST_NATIVE_BUILD_DIR/bin/layerx-module-registry"; do test -x "$executable"; done
if [ "$(id -u)" -ne 0 ]; then
    exec sudo -n --preserve-env=PATH,CARGO_TARGET_DIR,LAYERX_TEST_NATIVE_BUDGET_CLIENT,LAYERX_TEST_MANAGED_AUTHORITY_BIN,LAYERX_TEST_MANAGED_IDENTITY_BIN,LAYERX_TEST_NATIVE_BUILD_DIR,LAYERX_TEST_NATIVE_BIN_DIR,LAYERX_TEST_PYTHON,LAYERX_TEST_RUNTIME_CLOCK_BIN,LAYERX_PAXEER_BOUNDARY_BIN,LAYERX_CUSTODY_PROOF_BIN,PAXD \
        sh "$repo_root/agent/tools/run-native-budget-tests.sh" managed
fi
exec sh "$repo_root/agent/tools/run-native-budget-tests.sh" managed
