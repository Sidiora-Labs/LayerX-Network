#!/usr/bin/env bash
set -euo pipefail
umask 077
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo=$(cd "$here/../../../.." && pwd)
recipe_path=platform/hosted/registry/builder-environment
revision=$(git -C "$repo" rev-parse HEAD)
git -C "$repo" diff --quiet "$revision" -- programs/vendor "$recipe_path" || {
    printf 'Commit builder inputs before constructing the source-bound environment\n' >&2
    exit 1
}
if [ "$#" -ne 1 ] || [ -e "$1" ] || [ -L "$1" ]; then
    printf 'usage: build-env.sh NEW_OUTPUT_DIRECTORY (must not exist)\n' >&2
    exit 64
fi
for tool in docker git tar python3; do command -v "$tool" >/dev/null; done
mkdir -p -- "$(dirname -- "$1")"
mkdir -- "$1"
out=$(cd "$1" && pwd)
context="$out/context"
mkdir "$context" "$out/rootfs" "$out/source"
container=
cleanup() {
    if [ -n "$container" ]; then docker rm "$container" >/dev/null; fi
}
trap cleanup EXIT
git -C "$repo" archive "$revision" programs/vendor "$recipe_path" | tar -C "$out/source" -xf -
recipe="$out/source/$recipe_path"
python3 "$recipe/verify-vendor.py" "$out/source/programs/vendor" "$context/vendor"
for name in Dockerfile package.json package-lock.json rust-downloads.lock install-rust.sh cargo-config.toml layerx-rustc layerx-build; do
    test -f "$recipe/$name" && test ! -L "$recipe/$name"
    cp -- "$recipe/$name" "$context/$name"
done
docker build --platform linux/amd64 --iidfile "$out/image-id" "$context"
container=$(docker create "$(cat "$out/image-id")" /bin/true)
docker export "$container" -o "$out/export.tar"
docker rm "$container" >/dev/null
container=
tar --no-same-owner -C "$out/rootfs" -xf "$out/export.tar"
rm "$out/export.tar" "$out/rootfs/.dockerenv"
python3 "$recipe/flatten.py" "$out/rootfs"
python3 "$recipe/ldscripts.py" "$out/rootfs"
python3 "$recipe/digest.py" "$out/rootfs" > "$out/environment-tree-digest"
printf '%s\n' "$revision" > "$out/source-revision"
printf 'Builder environment: %s/rootfs\nTree digest: %s\n' "$out" "$(cat "$out/environment-tree-digest")"
