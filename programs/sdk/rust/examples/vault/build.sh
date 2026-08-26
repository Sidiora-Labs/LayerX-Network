#!/bin/sh
set -eu

program_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
programs_dir=$(CDPATH= cd -- "$program_dir/../../../.." && pwd)
target=wasm32-unknown-unknown
artifact="$program_dir/target/$target/release/layerx_reference_vault.wasm"
CARGO=${CARGO:-cargo}

(cd "$program_dir" && "$CARGO" build --locked --release --target "$target")
(cd "$programs_dir" && "$CARGO" run --locked --quiet -p layerx-program-lint --bin layerx-program-lint -- "$program_dir" "$artifact")
printf '%s\n' "$artifact"
