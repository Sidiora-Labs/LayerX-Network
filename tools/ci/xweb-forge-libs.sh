#!/usr/bin/env bash
# Installs the Foundry libraries the xweb contract suite builds against, at the
# tags the forge workflow pins, into contracts/lib. A library that is already
# present must be a clone of the pinned repository at the pinned tag.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
workflow="$root/.github/workflows/paxeer-forge-test.yml"
cd "$root"

pins="$(grep -oE 'git clone --depth 1 --branch [^ ]+ [^ ]+ contracts/lib/[A-Za-z0-9._-]+' "$workflow" || true)"

for name in forge-std openzeppelin-contracts; do
  line="$(printf '%s\n' "$pins" | grep -E " contracts/lib/${name}\$" || true)"
  if [ -z "$line" ]; then
    echo "xweb-forge-libs: $workflow pins no $name clone" >&2
    exit 1
  fi
  read -r _ _ _ _ _ tag url dir <<<"$line"
  if [ ! -e "$dir" ]; then
    git clone --quiet --depth 1 --branch "$tag" "$url" "$dir"
  fi
  origin="$(git -C "$dir" config --get remote.origin.url || true)"
  if [ "$origin" != "$url" ]; then
    echo "xweb-forge-libs: $dir is a clone of '$origin', not $url" >&2
    exit 1
  fi
  head="$(git -C "$dir" rev-parse HEAD)"
  want="$(git -C "$dir" rev-parse --verify --quiet "refs/tags/${tag}^{commit}" || true)"
  if [ -z "$want" ]; then
    want="$(git ls-remote "$url" "refs/tags/${tag}^{}" "refs/tags/${tag}" | awk 'NR==1 || /\^\{\}$/ {c=$1} END {print c}')"
  fi
  if [ "$head" != "$want" ]; then
    echo "xweb-forge-libs: $dir is at $head, not $tag ($want)" >&2
    exit 1
  fi
  echo "xweb-forge-libs: $name $tag at $dir"
done
