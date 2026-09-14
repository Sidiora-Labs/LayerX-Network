#!/bin/sh
set -eu

case "${1-}" in
    test) test_target=agent-test ;;
    sanitizers) test_target=agent-test-sanitize ;;
    prepare) test_target= ;;
    *) echo "expected test, sanitizers or prepare" >&2; exit 2 ;;
esac
if [ "$#" -ne 1 ] || [ "$(id -u)" -ne 0 ]; then
    echo "real-node tests require root to launch the daemon under its distinct uid" >&2
    exit 2
fi

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
cd "$repo_root"
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$repo_root/.lane-target}
export CARGO_BUILD_JOBS=4
export TMPDIR=/tmp
for executable in forge cast anvil go; do
    command -v "$executable" >/dev/null
done
python3 -c "from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey; from Crypto.Hash import keccak; from eth_account import Account; from eth_utils import to_checksum_address"
if [ -n "$test_target" ]; then
    exec make "$test_target"
fi

export LAYERX_TEST_NATIVE_BUILD_DIR=${LAYERX_TEST_NATIVE_BUILD_DIR:-$repo_root/build}
make -j4 CC=gcc BUILD_DIR="$LAYERX_TEST_NATIVE_BUILD_DIR" \
    LXP_REVISION="$(git rev-parse HEAD)" \
    PROGRAMS_TARGET_DIR="$CARGO_TARGET_DIR" \
    PROGRAMS_RUNTIME_LIB="$CARGO_TARGET_DIR/debug/liblayerx_programs_sandbox.a" \
    "$LAYERX_TEST_NATIVE_BUILD_DIR/bin/layerxd" \
    "$LAYERX_TEST_NATIVE_BUILD_DIR/bin/layerx-genesis-build" \
    "$LAYERX_TEST_NATIVE_BUILD_DIR/tests/lxp_test_daemon_finality_authority" \
    "$LAYERX_TEST_NATIVE_BUILD_DIR/tests/lxp_test_program_admission" \
    "$LAYERX_TEST_NATIVE_BUILD_DIR/tests/lxp_test_metered_allowance" \
    "$LAYERX_TEST_NATIVE_BUILD_DIR/tests/lxp_test_module_maintenance" \
    "$LAYERX_TEST_NATIVE_BUILD_DIR/tests/lxp_test_guarantor_runtime" \
    "$LAYERX_TEST_NATIVE_BUILD_DIR/tests/bridge/sign-credit"
make BUILD_DIR="$LAYERX_TEST_NATIVE_BUILD_DIR" PAXEER_GO_JOBS=4 custody-proof-build
GOMAXPROCS=4 GOFLAGS="${GOFLAGS:-} -p=4" make paxeer-build
make public-tls-test-prerequisites
cargo build --manifest-path agent/Cargo.toml --locked -p layerx-agentd \
    --example native_budget_recovery --target-dir "$CARGO_TARGET_DIR"

export LAYERX_TEST_NATIVE_BIN_DIR=$LAYERX_TEST_NATIVE_BUILD_DIR/bin
export LAYERX_CUSTODY_PROOF_BIN=$LAYERX_TEST_NATIVE_BIN_DIR/layerx-custody-proof
export PAXD=$repo_root/paxeer-network/build/paxd
sha256sum "$LAYERX_TEST_NATIVE_BIN_DIR/layerxd" \
    "$LAYERX_TEST_NATIVE_BIN_DIR/layerx-genesis-build" "$PAXD"
