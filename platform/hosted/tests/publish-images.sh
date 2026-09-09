#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd "$SCRIPT_DIR/../../.." && pwd)
source "$SCRIPT_DIR/beta-images.sh"

fail() { printf 'publish-images: error: %s\n' "$*" >&2; exit 1; }

publish_images() {
    local check_only=0
    if [ "${1:-}" = --check ]; then check_only=1; shift; fi
    local inventory=${1:-$REPO_ROOT/build/beta-cluster/images}
    local name canonical ref id extra expected dockerfile actual tag revision="" digest beta_digest
    local -A refs=() ids=()
    [ "$#" -le 1 ] || fail "usage: publish-images.sh [--check] [build-image-inventory]"
    [ -f "$inventory" ] && [ ! -L "$inventory" ] || fail "missing regular image inventory $inventory"
    for name in docker jq git; do
        command -v "$name" >/dev/null || fail "required tool $name is unavailable"
    done
    while read -r name canonical ref id extra; do
        [ -n "$name" ] && [ -z "$extra" ] || fail "malformed image inventory"
        read -r expected dockerfile <<<"$(image_source "$name")"
        [ "$canonical" = "$expected" ] || fail "unexpected canonical image for $name"
        [ -z "${refs[$name]+present}" ] || fail "duplicate image $name"
        [[ $id =~ ^sha256:[0-9a-f]{64}$ ]] || fail "invalid image ID for $name"
        [[ $ref == */"$name":* ]] || fail "source image name differs for $name"
        tag=${ref##*:}
        [[ $tag =~ ^[0-9a-f]{7,40}(-dirty)?$ ]] || fail "source image lacks a git revision: $ref"
        if [ -z "$revision" ]; then revision=$tag; fi
        [ "$tag" = "$revision" ] || fail "images were built from different revisions"
        actual=$(docker image inspect --format '{{.Id}}' "$ref") || fail "missing local image $ref"
        [ "$actual" = "$id" ] || fail "local image $ref differs from build inventory"
        refs[$name]=$ref
        ids[$name]=$id
    done < "$inventory"
    [ "${#refs[@]}" -eq "${#IMAGE_NAMES[@]}" ] || fail "inventory must contain every beta image"
    for name in "${IMAGE_NAMES[@]}"; do
        [ -n "${refs[$name]:-}" ] || fail "missing local image $name"
    done
    tag=${revision%-dirty}
    git -C "$REPO_ROOT" cat-file -e "$tag^{commit}" || fail "build revision is not a known commit"
    if [ "$revision" != "$tag" ]; then
        printf 'publish-images: source inventory is %s; SHA tag %s identifies its base commit plus recorded uncommitted changes\n' "$revision" "$tag" >&2
    fi
    if [ "$check_only" = 1 ]; then
        printf 'publish-images: verified %s local images from %s; no tags or pushes performed\n' "${#refs[@]}" "$revision"
        return
    fi
    for name in "${IMAGE_NAMES[@]}"; do
        ref="ghcr.io/sidiora-labs/$name"
        docker tag "${ids[$name]}" "$ref:$tag"
        docker push "$ref:$tag"
        digest=$(registry_image_digest "$ref:$tag")
        docker image inspect "${ids[$name]}" --format '{{json .RepoDigests}}' \
            | jq -e --arg expected "$ref@$digest" 'index($expected) != null' >/dev/null \
            || fail "registry manifest differs from pushed image $name"
        docker tag "${ids[$name]}" "$ref:beta"
        docker push "$ref:beta"
        beta_digest=$(registry_image_digest "$ref:beta")
        [ "$digest" = "$beta_digest" ] || fail "published tags differ for $name"
        printf 'published %s %s %s source=%s\n' "$ref" "$tag,beta" "$digest" "$revision"
    done
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
    publish_images "$@"
fi
