#!/bin/sh
set -eu

cd "$(dirname "$0")/.."

if make build; then
    exit 0
fi

printf '%s\n' \
    "make build failed. The C library target does not require network." \
    "Confirm clang-18, make, libssl-dev, and libsqlite3-dev are installed, then run:" \
    "  make build" \
    "Workspace suites that need the rest of the image toolchain:" \
    "  make test" \
    "  cargo fmt --check --manifest-path agent/Cargo.toml --all" \
    "  forge fmt --check"
exit 1
