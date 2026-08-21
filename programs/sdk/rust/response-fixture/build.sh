#!/bin/sh
set -eu

fixture_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
artifact="$fixture_dir/target/wasm32-unknown-unknown/release/layerx_response_fixture.wasm"
CARGO=${CARGO:-cargo}

(cd "$fixture_dir" && "$CARGO" build --locked --release --target wasm32-unknown-unknown)
for symbol in layerx_v2_candidate response_write program_call_response refusal_write layerx_call layerx_reserve
do
	if ! strings "$artifact" | grep -F "$symbol" >/dev/null
	then
		printf 'candidate response fixture: missing %s\n' "$symbol" >&2
		exit 1
	fi
done
(cd "$fixture_dir" && "$CARGO" run --locked --release --example qualify -- "$artifact")
printf '%s\n' "$artifact"
