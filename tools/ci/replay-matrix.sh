#!/bin/sh
# Golden replay on every supported architecture. The release workflow runs
# this on an x86_64 and an aarch64 runner and passes the runner's expected
# machine in LXP_REPLAY_MACHINE; the script refuses to pass on any other
# machine, and always refuses an unsupported architecture.
set -eu

supported="x86_64 aarch64"
machine=$(uname -m)
case "$machine" in
    arm64) machine=aarch64 ;;
esac
case " $supported " in
    *" $machine "*) ;;
    *)
        echo "replay-matrix: unsupported architecture $machine (supported: $supported)" >&2
        exit 1
        ;;
esac
if [ -n "${LXP_REPLAY_MACHINE:-}" ] && [ "$LXP_REPLAY_MACHINE" != "$machine" ]; then
    echo "replay-matrix: expected to replay on $LXP_REPLAY_MACHINE but this runner is $machine" >&2
    exit 1
fi

make --no-print-directory test-replay-golden-local
make --no-print-directory BUILD_DIR=build/replay-clang CC=clang-18 \
    OPT_LEVEL=-O0 test-replay-golden-local

echo "replay-matrix: golden replay verified on $machine"
