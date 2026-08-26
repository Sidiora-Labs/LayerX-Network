#!/bin/sh
set -eu

program_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
sdk_dir=$(CDPATH= cd -- "$program_dir/.." && pwd)
programs_dir=$(CDPATH= cd -- "$sdk_dir/../.." && pwd)

target=wasm32-unknown-unknown
artifact="$program_dir/target/$target/release/layerx_quickstart.wasm"

CARGO=${CARGO:-cargo}

compile() {
	(cd "$program_dir" && "$CARGO" build --release --target "$target")
}

lint() {
	(cd "$programs_dir" && "$CARGO" run --quiet -p layerx-program-lint \
		--bin layerx-program-lint -- --abi-version 1 "$program_dir" "$artifact")
}

case "${1:-all}" in
all)
	compile
	lint
	printf '%s\n' "$artifact"
	;;
compile)
	compile
	printf '%s\n' "$artifact"
	;;
lint)
	lint
	;;
clean)
	rm -rf "$program_dir/target"
	;;
*)
	printf 'usage: build.sh [all|compile|lint|clean]\n' >&2
	exit 2
	;;
esac
