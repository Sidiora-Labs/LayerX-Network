#!/usr/bin/env bash
set -euo pipefail

usage() {
    cat <<'USAGE'
usage: tools/ci/release-artifact-record.sh <name> <file> <digest_of> <signature> <sbom> <attestation> <location>

Appends one [artifact.<n>] section to artifacts.kvx in the current directory,
recording the retained artifact <file> (a path relative to the current
directory) of the declared package <name> at LAYERX_RELEASE_VERSION with the
sha256 of its bytes. <digest_of> states what the digest hashes: built-bytes
(the bytes the job built and uploaded), registry-bytes (the bytes the registry
serves, fetched after publication) or source-archive (the archive of the
published source tree). <signature>, <sbom> and <attestation> are retained
files relative to the current directory or external references of the form
<scheme>:<locator>; <location> is the registry URL the published bytes are
fetched from (https://..., simple+https://.../<project>/ or
git+https://<repository>#<tag>), empty only while the artifact is unpublished.
The release tool reads the record when it emits the artifact manifest.
USAGE
}

if [ "$#" -ne 7 ]; then
    usage >&2
    exit 2
fi
name=$1 file=$2 digest_of=$3 signature=$4 sbom=$5 attestation=$6 location=$7
: "${LAYERX_RELEASE_VERSION:?LAYERX_RELEASE_VERSION must name the release version}"
case "$digest_of" in
    built-bytes | registry-bytes | source-archive) ;;
    *)
        echo "release-artifact-record: digest_of must be built-bytes, registry-bytes or source-archive, got $digest_of" >&2
        exit 2
        ;;
esac
for value in "$name" "$file" "$signature" "$sbom" "$attestation" "$location"; do
    case "$value" in
        *'"'* | *$'\n'*)
            echo "release-artifact-record: values cannot contain quotes or newlines: $value" >&2
            exit 2
            ;;
    esac
done
if [ -z "$name" ] || [ -z "$file" ] || [ -z "$signature" ] || [ -z "$sbom" ] || [ -z "$attestation" ]; then
    echo "release-artifact-record: name, file, signature, sbom and attestation must be non-empty" >&2
    exit 2
fi
case "$file" in
    /* | ../* | */../* | */.. | ..)
        echo "release-artifact-record: file must be a relative path inside the current directory: $file" >&2
        exit 2
        ;;
esac
if [ ! -f "$file" ]; then
    echo "release-artifact-record: $file is not a file in $PWD" >&2
    exit 2
fi
digest=$(sha256sum "$file" | cut -d ' ' -f 1)
if [ -f artifacts.kvx ]; then
    index=$(($(grep -c '^\[artifact\.' artifacts.kvx || true) + 1))
else
    index=1
fi
{
    printf '[artifact.%s]\n' "$index"
    printf 'name = "%s"\n' "$name"
    printf 'version = "%s"\n' "$LAYERX_RELEASE_VERSION"
    printf 'file = "%s"\n' "$file"
    printf 'digest = "sha256:%s"\n' "$digest"
    printf 'digest_of = "%s"\n' "$digest_of"
    printf 'signature = "%s"\n' "$signature"
    printf 'sbom = "%s"\n' "$sbom"
    printf 'attestation = "%s"\n' "$attestation"
    printf 'location = "%s"\n' "$location"
    printf '\n'
} >> artifacts.kvx
echo "recorded $name $LAYERX_RELEASE_VERSION $file sha256:$digest ($digest_of)"
