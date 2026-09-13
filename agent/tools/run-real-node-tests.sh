#!/bin/sh
set -eu

case "${1-}" in
    test) test_target=agent-test ;;
    sanitizers) test_target=agent-test-sanitize ;;
    *) echo "expected test or sanitizers" >&2; exit 2 ;;
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
for executable in forge cast anvil; do
    command -v "$executable" >/dev/null
done
python3 -c "from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey; from Crypto.Hash import keccak"

make -j4 CC=gcc \
    LXP_REVISION="$(git rev-parse HEAD)" \
    PROGRAMS_TARGET_DIR="$CARGO_TARGET_DIR" \
    PROGRAMS_RUNTIME_LIB="$CARGO_TARGET_DIR/debug/liblayerx_programs_sandbox.a" \
    build/bin/layerxd build/bin/layerx-genesis-build \
    build/tests/lxp_test_daemon_finality_authority build/tests/lxp_test_program_admission

export LAYERX_TEST_NATIVE_BIN_DIR=$repo_root/build/bin
sha256sum "$LAYERX_TEST_NATIVE_BIN_DIR/layerxd" \
    "$LAYERX_TEST_NATIVE_BIN_DIR/layerx-genesis-build"
make "$test_target"
