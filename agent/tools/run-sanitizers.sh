#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
manifest="$repo_root/agent/Cargo.toml"
target=x86_64-unknown-linux-gnu
tsan_toolchain=nightly-2025-11-10
rust_source=$(rustc +"$tsan_toolchain" --print sysroot)/lib/rustlib/src/rust/library/Cargo.toml
if [ ! -f "$rust_source" ]; then
    echo "rust-src is required to instrument the ThreadSanitizer standard library" >&2
    exit 1
fi

RUSTC_BOOTSTRAP=1 RUSTFLAGS='-Zsanitizer=address' \
    RUSTDOCFLAGS='-Zsanitizer=address' \
    cargo test --manifest-path "$manifest" --locked --workspace --target "$target" --no-fail-fast

tsan_target=${LAYERX_AGENT_TSAN_TARGET_DIR:-${CARGO_TARGET_DIR:-$repo_root/agent/target}/thread-sanitizer}
CARGO_TARGET_DIR="$tsan_target" RUSTUP_TOOLCHAIN="$tsan_toolchain" \
    LAYERX_TEST_SANITIZER=thread \
    RUSTFLAGS='-Zsanitizer=thread' RUSTDOCFLAGS='-Zsanitizer=thread' \
    cargo test -Zbuild-std --manifest-path "$manifest" --locked --workspace \
        --target "$target" --no-fail-fast
