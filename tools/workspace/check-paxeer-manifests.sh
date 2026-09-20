#!/bin/sh
set -eu

inventory=${1:-tools/workspace/paxeer-manifest-roots.txt}
actual=$(mktemp)
declared=$(mktemp)
trap 'rm -f "$actual" "$declared"' EXIT HUP INT TERM

# The Paxeer chain sits at the repository root beside the LayerX trees, so the
# scan names every chain-owned root explicitly instead of walking the whole
# repository. A new chain top-level directory must be added here.
chain_roots="admin api assets benchmark consensus custodyproof daemon docker engine example hpx integration_test interchain loadtest modules node occ_tests parallelization precompiles ratelimiter rpc sdk storage store sync testutil types utils wasm wasm-runtime wasmbinding contracts tests/chain tools/chain tools/tx-scanner tools/utils"
for root in $chain_roots; do test -d "$root"; done
{
    for manifest in go.mod foundry.paxeer.toml; do test -f "$manifest" && printf '%s\n' "$manifest"; done
    # shellcheck disable=SC2086
    find $chain_roots -path '*/node_modules' -prune -o -path '*/target' -prune -o \
        \( -name Cargo.toml -o -name go.mod -o -name package.json -o -name foundry.toml \) \
        -print
} | LC_ALL=C sort > "$actual"
awk -F '|' '!/^#/ && NF == 3 { print $2 }' "$inventory" | LC_ALL=C sort > "$declared"
cmp "$actual" "$declared"

while IFS='|' read -r kind path classification; do
    case "$kind" in \#*|'') continue ;; esac
    test -s "$path"
    case "$classification" in
        build_test_lint|build_static_live_test_blocked|build_static) ;;
        vendored_source_checksum_bound)
            test "$path" = loadtest/contracts/evm/lib/openzeppelin-contracts/contracts/package.json
            test -s loadtest/contracts/evm/VENDORING.md
            test -x loadtest/contracts/evm/setup.sh
            ;;
        *) echo "unknown Paxeer manifest classification: $classification" >&2; exit 1 ;;
    esac
    if test "$kind" = rust; then
        test -s "${path%/Cargo.toml}/Cargo.lock"
    fi
done < "$inventory"
