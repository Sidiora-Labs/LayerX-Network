#!/bin/sh
set -eu

program_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
target=wasm32-unknown-unknown
target_dir=${CARGO_TARGET_DIR:-target}
case "$target_dir" in
    /*) ;;
    *) target_dir="$program_dir/$target_dir" ;;
esac
artifact="$target_dir/$target/release/layerx_reference_web_reader.wasm"
CARGO=${CARGO:-cargo}

(cd "$program_dir" && "$CARGO" build --locked --release --target "$target")
test -s "$artifact"
printf '%s\n' "$artifact"
