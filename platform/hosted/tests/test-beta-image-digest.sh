#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
source "$SCRIPT_DIR/beta-images.sh"

[ "$#" -eq 3 ] || { printf 'usage: %s <registry-image> <expected-index-digest> <evidence-directory>\n' "$0" >&2; exit 2; }
reference=$1
expected=$2
evidence=$3
[[ $expected =~ ^sha256:[0-9a-f]{64}$ ]]
mkdir -p "$evidence"

docker buildx imagetools inspect --format '{{json .Manifest}}' "$reference" > "$evidence/root-descriptor.json"
docker buildx imagetools inspect --raw "$reference" > "$evidence/root-manifest.json"
[ "$(registry_image_digest "$reference")" = "$expected" ]
[ "$(registry_manifest_digest < "$evidence/root-descriptor.json")" = "$expected" ]
[ "sha256:$(sha256sum "$evidence/root-manifest.json" | cut -d ' ' -f 1)" = "$expected" ]
[ "$(wc -c < "$evidence/root-manifest.json")" -eq "$(jq -r .size "$evidence/root-descriptor.json")" ]
jq -e '
    .schemaVersion == 2
    and .mediaType == "application/vnd.oci.image.index.v1+json"
    and any(.manifests[]; .platform.os == "linux")
    and any(.manifests[]; .annotations["vnd.docker.reference.type"] == "attestation-manifest")
' "$evidence/root-descriptor.json" >/dev/null
jq -S 'del(.digest, .size)' "$evidence/root-descriptor.json" > "$evidence/root-projection.json"
jq -S . "$evidence/root-manifest.json" > "$evidence/root-canonical.json"
cmp "$evidence/root-projection.json" "$evidence/root-canonical.json"

repository=${reference%@*}
if [[ ${repository##*/} == *:* ]]; then repository=${repository%:*}; fi
[ "$(registry_image_digest "$repository@$expected")" = "$expected" ]
index=0
while read -r digest size; do
    [ "$digest" != "$expected" ]
    [ "$(registry_image_digest "$repository@$digest")" = "$digest" ]
    docker buildx imagetools inspect --format '{{json .Manifest}}' "$repository@$digest" > "$evidence/child-$index-descriptor.json"
    docker buildx imagetools inspect --raw "$repository@$digest" > "$evidence/child-$index-manifest.json"
    [ "sha256:$(sha256sum "$evidence/child-$index-manifest.json" | cut -d ' ' -f 1)" = "$digest" ]
    [ "$(wc -c < "$evidence/child-$index-manifest.json")" -eq "$size" ]
    index=$((index + 1))
done < <(jq -r '.manifests[] | "\(.digest) \(.size)"' "$evidence/root-descriptor.json")

index=0
while IFS= read -r mutation; do
    jq "$mutation" "$evidence/root-descriptor.json" > "$evidence/refusal-$index.json"
    if registry_manifest_digest < "$evidence/refusal-$index.json" > "$evidence/refusal-$index.stdout" 2> "$evidence/refusal-$index.stderr"; then
        printf 'accepted malformed root descriptor: %s\n' "$mutation" >&2
        exit 1
    fi
    index=$((index + 1))
done <<'MUTATIONS'
null
[.]
del(.digest)
.digest = "sha256:1234"
.digest |= ascii_upcase
.digest += "\n"
.digest = null
.size = 0
.size = -1
.size = 1.5
.size = "856"
del(.size)
.mediaType = "application/json"
.mediaType = null
.schemaVersion = 1
del(.schemaVersion)
.manifests = []
.manifests = {}
del(.manifests)
.manifests[0].digest = "sha256:1234"
.manifests[0].size = 0
.manifests[0].mediaType = "application/json"
MUTATIONS

printf 'verified registry root, all image and provenance children, and %s malformed-root refusals\n' "$index"
