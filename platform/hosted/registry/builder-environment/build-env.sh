#!/usr/bin/env bash
set -euo pipefail
umask 077
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo=$(cd "$here/../../../.." && pwd)
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
git -C "$repo" archive HEAD programs/vendor | tar -C "$out/source" --strip-components=1 -xf -
python3 "$here/verify-vendor.py" "$out/source/vendor" "$context/vendor"
for name in Dockerfile package.json package-lock.json rust-downloads.lock install-rust.sh cargo-config.toml layerx-rustc layerx-build; do
    test -f "$here/$name" && test ! -L "$here/$name"
    cp -- "$here/$name" "$context/$name"
done
docker build --platform linux/amd64 --iidfile "$out/image-id" "$context"
container=$(docker create "$(cat "$out/image-id")" /bin/true)
docker export "$container" -o "$out/export.tar"
docker rm "$container" >/dev/null
container=
tar --no-same-owner -C "$out/rootfs" -xf "$out/export.tar"
rm "$out/export.tar" "$out/rootfs/.dockerenv"
python3 "$here/flatten.py" "$out/rootfs"
python3 "$here/ldscripts.py" "$out/rootfs"
python3 "$here/digest.py" "$out/rootfs" > "$out/environment-tree-digest"
git -C "$repo" rev-parse HEAD > "$out/source-revision"
printf 'Builder environment: %s/rootfs\nTree digest: %s\n' "$out" "$(cat "$out/environment-tree-digest")"
