#!/usr/bin/env bash
# Fetches the two pinned Solidity libraries the PaxeerXVault build needs into
# bridge/evm/lib. Neither library is committed: this script and the bridge
# workflow are the only ways lib is populated, and both clone the same tags.
# Running it again with both libraries already at their pinned tag and unmodified
# does nothing; a checkout whose contents were edited is replaced.
set -euo pipefail

FORGE_STD_TAG='v1.9.6'
FORGE_STD_URL='https://github.com/foundry-rs/forge-std'
OPENZEPPELIN_TAG='v5.3.0'
OPENZEPPELIN_URL='https://github.com/OpenZeppelin/openzeppelin-contracts'

lib_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/lib"

# Prints the tag the checkout at $1 is on, or nothing when it is not a
# checkout of that tag.
checked_out_tag() {
    local dir="$1"
    [ -d "$dir/.git" ] || return 0
    git -C "$dir" describe --tags --exact-match 2>/dev/null || true
}

# Succeeds when the checkout at $1 carries the tag's own contents: no modified
# file, no deleted file and no extra file. A checkout that is at the pinned tag
# but was edited afterwards is not the pinned dependency, so it is not reused.
checkout_is_pristine() {
    local dir="$1"
    [ -z "$(git -C "$dir" status --porcelain 2>/dev/null)" ]
}

fetch_library() {
    local name="$1" url="$2" tag="$3" current
    local dest="$lib_dir/$name"

    current="$(checked_out_tag "$dest")"
    if [ "$current" = "$tag" ] && checkout_is_pristine "$dest"; then
        printf 'bootstrap-libs: %s already at %s\n' "$name" "$tag"
        return 0
    fi
    if [ -e "$dest" ]; then
        if [ "$current" = "$tag" ]; then
            printf 'bootstrap-libs: %s is at %s but its contents are modified, replacing it\n' "$name" "$tag" >&2
        elif [ -n "$current" ]; then
            printf 'bootstrap-libs: %s is at %s, replacing it with %s\n' "$name" "$current" "$tag" >&2
        else
            printf 'bootstrap-libs: %s is present but not a checkout of %s, replacing it\n' "$name" "$tag" >&2
        fi
        rm -rf -- "$dest"
    fi
    printf 'bootstrap-libs: cloning %s %s\n' "$name" "$tag"
    git clone --depth 1 --branch "$tag" "$url" "$dest"
}

mkdir -p -- "$lib_dir"
fetch_library 'forge-std' "$FORGE_STD_URL" "$FORGE_STD_TAG"
fetch_library 'openzeppelin-contracts' "$OPENZEPPELIN_URL" "$OPENZEPPELIN_TAG"
