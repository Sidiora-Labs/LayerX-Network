#!/bin/sh
set -eu

inventory=${1:-tools/workspace/paxeer-manifest-roots.txt}
actual=$(mktemp)
declared=$(mktemp)
trap 'rm -f "$actual" "$declared"' EXIT HUP INT TERM

find paxeer-network -path '*/node_modules' -prune -o -path '*/target' -prune -o \
    \( -name Cargo.toml -o -name go.mod -o -name package.json -o -name foundry.toml \) \
    -print | LC_ALL=C sort > "$actual"
awk -F '|' '!/^#/ && NF == 3 { print $2 }' "$inventory" | LC_ALL=C sort > "$declared"
cmp "$actual" "$declared"

while IFS='|' read -r kind path classification; do
    case "$kind" in \#*|'') continue ;; esac
    test -s "$path"
    case "$classification" in
        build_test_lint|build_static_live_test_blocked|build_static) ;;
        vendored_source_checksum_bound)
            test "$path" = paxeer-network/loadtest/contracts/evm/lib/openzeppelin-contracts/contracts/package.json
            test -s paxeer-network/loadtest/contracts/evm/VENDORING.md
            test -x paxeer-network/loadtest/contracts/evm/setup.sh
            ;;
        *) echo "unknown Paxeer manifest classification: $classification" >&2; exit 1 ;;
    esac
    if test "$kind" = rust; then
        test -s "${path%/Cargo.toml}/Cargo.lock"
    fi
done < "$inventory"
