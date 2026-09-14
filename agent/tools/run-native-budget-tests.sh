#!/bin/sh
set -eu
if [ "$#" -ne 1 ] || [ "$(id -u)" -ne 0 ]; then
    echo "native Budget tests require one scenario and root for the distinct node uid" >&2
    exit 2
fi
case "$1" in recovery|unknown|refusals) ;; *) echo "expected recovery, unknown or refusals" >&2; exit 2 ;; esac
repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
cd "$repo_root"
export CARGO_BUILD_JOBS=4
export LAYERX_TEST_NATIVE_BUDGET_SCENARIO=$1
export LAYERX_TEST_NATIVE_BUILD_DIR=${LAYERX_TEST_NATIVE_BUILD_DIR:-$repo_root/build}
export LAYERX_TEST_NATIVE_BIN_DIR=${LAYERX_TEST_NATIVE_BIN_DIR:-$LAYERX_TEST_NATIVE_BUILD_DIR/bin}
export LAYERX_TEST_NATIVE_BUDGET_CLIENT=${LAYERX_TEST_NATIVE_BUDGET_CLIENT:-${CARGO_TARGET_DIR:-$repo_root/.lane-target}/debug/examples/native_budget_recovery}
export LAYERX_TEST_RUNTIME_CLOCK_BIN=${LAYERX_TEST_RUNTIME_CLOCK_BIN:-${CARGO_TARGET_DIR:-$repo_root/.lane-target}/debug/layerx-runtime-clock}
: "${PAXD:?actual disposable Paxeer daemon is required}"
: "${LAYERX_CUSTODY_PROOF_BIN:?actual custody proof binary is required}"
for executable in forge cast anvil; do command -v "$executable" >/dev/null; done
for executable in "$LAYERX_TEST_NATIVE_BUDGET_CLIENT" "$LAYERX_TEST_RUNTIME_CLOCK_BIN" \
    "$LAYERX_TEST_NATIVE_BIN_DIR/layerxd" "$LAYERX_TEST_NATIVE_BIN_DIR/layerx-genesis-build" \
    "$LAYERX_TEST_NATIVE_BUILD_DIR/tests/lxp_test_module_maintenance" \
    "$LAYERX_TEST_NATIVE_BUILD_DIR/tests/lxp_test_guarantor_runtime" \
    "$LAYERX_TEST_NATIVE_BUILD_DIR/tests/bridge/sign-credit" "$PAXD" "$LAYERX_CUSTODY_PROOF_BIN"; do
    test -x "$executable"
done
exec "${LAYERX_TEST_PYTHON:-python3}" tests/daemon/withdraw-custody.py "$LAYERX_TEST_NATIVE_BUILD_DIR" --native-budget
