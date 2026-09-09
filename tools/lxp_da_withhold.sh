#!/bin/sh
set -eu

mode=${1:-all}
binary=${2:-build/tests/test_da_unavailable}

run_class() {
    case "$1" in
        activities|receipts|oracle|state-diff|recovery) ;;
        *) echo "unknown DA class: $1" >&2; exit 2 ;;
    esac
    if "$binary" "$1"; then
        printf 'DA class %s: passed (exit 0)\n' "$1"
    else
        status=$?
        printf 'DA class %s: %s failed (exit %s)\n' "$1" "$binary" "$status" >&2
        exit "$status"
    fi
}

if [ "$mode" = all ]; then
    for class in activities receipts oracle state-diff recovery; do
        run_class "$class"
    done
else
    run_class "$mode"
fi
