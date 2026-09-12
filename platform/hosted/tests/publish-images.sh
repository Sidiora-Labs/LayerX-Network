#!/usr/bin/env bash
# Publishes the beta container images to the canonical GHCR organisation with an
# SPDX SBOM, a keyless signature and a build-provenance attestation per image,
# and verifies every published digest from the registry before the release tag
# and the moving :beta tag are repointed at it.
#
# usage: publish-images.sh <mode> [options] [build-image-inventory]
#
# modes
#   --check           validate the local build inventory only: no SBOM, no
#                     registry access, no tags and no pushes
#   --dry-run         resolve the release binding, generate the SBOM of every
#                     image and write the publication plan; no registry access
#   --phase push      push the immutable :<revision> tag of every image, verify
#                     the registry manifest digest against the pushed image,
#                     sign that digest and attach its SBOM attestation
#   --phase verify    verify the recorded signatures, SBOMs and source provenance;
#                     perform no registry writes or tag changes
#   --phase promote   verify the signature, the SBOM attestation and the build
#                     provenance of every published digest, then repoint the
#                     release tag and the moving :beta tag and verify both
#   --self-test       exercise the inventory, release-binding and publication
#                     refusals and the dry run against the local images
#
# options
#   --release-tag <name>       the git tag whose commit the images were built
#                              from; also published as :<name>
#   --release-candidate <sha>  the 40-hex release-candidate commit on the
#                              default branch the images were built from
#   --output <dir>             where the SBOMs, the digests and the publication
#                              record are written and read back
#                              (default build/beta-cluster/publication)
#
# Every mode but --check requires exactly one release binding: the beta images
# are published from a release tag or from the named release candidate and never
# from an arbitrary branch push, and the moving :beta tag is repointed only
# after the published digest verifies.

set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
SCRIPT_PATH="$SCRIPT_DIR/$(basename "${BASH_SOURCE[0]}")"
REPO_ROOT=$(cd "$SCRIPT_DIR/../../.." && pwd)
source "$SCRIPT_DIR/beta-images.sh"

REGISTRY_ORG=ghcr.io/sidiora-labs
MOVING_TAG=beta
PUBLISH_WORKFLOW=.github/workflows/publish-images.yml
OIDC_ISSUER=https://token.actions.githubusercontent.com
SBOM_PREDICATE_TYPE=spdxjson
PROVENANCE_PREDICATE_TYPE=https://slsa.dev/provenance/v1

MODE=""
RELEASE_TAG_NAME=""
RELEASE_CANDIDATE=""
RELEASE_KIND=""
RELEASE_NAME=""
RELEASE_COMMIT=""
RECORDED_REGISTRY=""
RECORDED_COMMIT=""
BUILD_REVISION=""
PUBLISH_TAG=""
INVENTORY=""
OUTPUT_DIR=""
declare -A IMAGE_REFS=()
declare -A IMAGE_IDS=()
declare -A IMAGE_DIGESTS=()
declare -A IMAGE_TARGETS=()

log() { printf 'publish-images: %s\n' "$*" >&2; }
fail() { printf 'publish-images: error: %s\n' "$*" >&2; exit 1; }
usage() { sed -n '2,/^$/p' "$SCRIPT_PATH" | sed 's/^# \{0,1\}//' >&2; }

require_tools() {
    local name
    for name in "$@"; do
        command -v "$name" >/dev/null || fail "required tool $name is unavailable"
    done
}

parse_arguments() {
    local positional=0
    while [ "$#" -gt 0 ]; do
        case $1 in
        --check | --dry-run | --self-test)
            [ -z "$MODE" ] || fail "only one mode may be given"
            MODE=${1#--}
            shift
            ;;
        --phase)
            [ "$#" -ge 2 ] || { usage; exit 2; }
            [ -z "$MODE" ] || fail "only one mode may be given"
            case $2 in
            push | verify | promote) MODE=$2 ;;
            *) fail "--phase must be push, verify or promote" ;;
            esac
            shift 2
            ;;
        --release-tag)
            [ "$#" -ge 2 ] || { usage; exit 2; }
            RELEASE_TAG_NAME=$2
            shift 2
            ;;
        --release-candidate)
            [ "$#" -ge 2 ] || { usage; exit 2; }
            RELEASE_CANDIDATE=$2
            shift 2
            ;;
        --output)
            [ "$#" -ge 2 ] || { usage; exit 2; }
            OUTPUT_DIR=$2
            shift 2
            ;;
        -h | --help)
            usage
            exit 0
            ;;
        -*)
            fail "unknown option $1"
            ;;
        *)
            positional=$((positional + 1))
            [ "$positional" -eq 1 ] || fail "usage: publish-images.sh <mode> [options] [build-image-inventory]"
            INVENTORY=$1
            shift
            ;;
        esac
    done
    [ -n "$MODE" ] || fail "a mode is required: --check, --dry-run, --phase push, --phase promote or --self-test"
    [ -n "$INVENTORY" ] || INVENTORY="$REPO_ROOT/build/beta-cluster/images"
    [ -n "$OUTPUT_DIR" ] || OUTPUT_DIR="$REPO_ROOT/build/beta-cluster/publication"
}

read_inventory() {
    local name canonical ref id extra expected dockerfile actual tag
    [ -f "$INVENTORY" ] && [ ! -L "$INVENTORY" ] || fail "missing regular image inventory $INVENTORY"
    while read -r name canonical ref id extra; do
        [ -n "$name" ] && [ -z "$extra" ] || fail "malformed image inventory"
        read -r expected dockerfile <<<"$(image_source "$name")"
        [ -n "$dockerfile" ] || fail "no Dockerfile is declared for $name"
        [ "$canonical" = "$expected" ] || fail "unexpected canonical image for $name"
        [ -z "${IMAGE_REFS[$name]+present}" ] || fail "duplicate image $name"
        [[ $id =~ ^sha256:[0-9a-f]{64}$ ]] || fail "invalid image ID for $name"
        [[ $ref == */"$name":* ]] || fail "source image name differs for $name"
        tag=${ref##*:}
        [[ $tag =~ ^[0-9a-f]{7,40}(-dirty)?$ ]] || fail "source image lacks a git revision: $ref"
        if [ -z "$BUILD_REVISION" ]; then BUILD_REVISION=$tag; fi
        [ "$tag" = "$BUILD_REVISION" ] || fail "images were built from different revisions"
        actual=$(docker image inspect --format '{{.Id}}' "$ref") || fail "missing local image $ref"
        [ "$actual" = "$id" ] || fail "local image $ref differs from build inventory"
        IMAGE_REFS[$name]=$ref
        IMAGE_IDS[$name]=$id
        IMAGE_TARGETS[$name]="$REGISTRY_ORG/$name"
    done < "$INVENTORY"
    [ "${#IMAGE_REFS[@]}" -eq "${#IMAGE_NAMES[@]}" ] || fail "inventory must contain every beta image"
    for name in "${IMAGE_NAMES[@]}"; do
        [ -n "${IMAGE_REFS[$name]:-}" ] || fail "missing local image $name"
    done
    PUBLISH_TAG=${BUILD_REVISION%-dirty}
    git -C "$REPO_ROOT" cat-file -e "$PUBLISH_TAG^{commit}" || fail "build revision is not a known commit"
    if [ "$BUILD_REVISION" != "$PUBLISH_TAG" ]; then
        log "source inventory is $BUILD_REVISION; SHA tag $PUBLISH_TAG identifies its base commit plus recorded uncommitted changes"
    fi
}

resolve_release_binding() {
    local bindings=0 resolved
    [ -z "$RELEASE_TAG_NAME" ] || bindings=$((bindings + 1))
    [ -z "$RELEASE_CANDIDATE" ] || bindings=$((bindings + 1))
    [ "$bindings" -eq 1 ] || fail "exactly one of --release-tag and --release-candidate is required: the beta images are published from a release tag or from the named release candidate, never from an arbitrary push"
    if [ -n "$RELEASE_TAG_NAME" ]; then
        [[ $RELEASE_TAG_NAME =~ ^[A-Za-z0-9_][A-Za-z0-9_.-]{0,126}$ ]] || fail "release tag $RELEASE_TAG_NAME is not a usable image tag"
        resolved=$(git -C "$REPO_ROOT" rev-parse --verify --quiet "refs/tags/$RELEASE_TAG_NAME^{commit}") \
            || fail "release tag $RELEASE_TAG_NAME does not name a commit in this repository"
        RELEASE_KIND=tag
        RELEASE_NAME=$RELEASE_TAG_NAME
        RELEASE_COMMIT=$resolved
    else
        [[ $RELEASE_CANDIDATE =~ ^[0-9a-f]{40}$ ]] || fail "release candidate $RELEASE_CANDIDATE is not a 40-hex commit"
        git -C "$REPO_ROOT" cat-file -e "$RELEASE_CANDIDATE^{commit}" 2>/dev/null \
            || fail "release candidate $RELEASE_CANDIDATE is not a commit in this repository"
        RELEASE_KIND=candidate
        RELEASE_NAME=$RELEASE_CANDIDATE
        RELEASE_COMMIT=$RELEASE_CANDIDATE
    fi
    [ "${RELEASE_COMMIT:0:${#PUBLISH_TAG}}" = "$PUBLISH_TAG" ] \
        || fail "the images were built from $PUBLISH_TAG, not from the $RELEASE_KIND commit $RELEASE_COMMIT"
}

require_gated_publication() {
    [ "${GITHUB_ACTIONS:-}" = true ] || fail "the beta images are published only from the gated $PUBLISH_WORKFLOW job"
    [ -n "${GITHUB_REPOSITORY:-}" ] || fail "GITHUB_REPOSITORY is unset: the publication cannot bind the signer identity"
    [ -n "${LAYERX_PUBLISH_GATE_REVISION:-}" ] \
        || fail "LAYERX_PUBLISH_GATE_REVISION is unset: publication requires the revision the test gate passed on"
    [ "$LAYERX_PUBLISH_GATE_REVISION" = "$RELEASE_COMMIT" ] \
        || fail "the test gate passed on $LAYERX_PUBLISH_GATE_REVISION, not on the published revision $RELEASE_COMMIT"
    [ "$BUILD_REVISION" = "$PUBLISH_TAG" ] \
        || fail "the images were built from a modified tree ($BUILD_REVISION); only a clean checkout of the release revision is published"
    [ "$RELEASE_KIND" != candidate ] || require_candidate_on_default_branch
}

require_candidate_on_default_branch() {
    local branch=${LAYERX_PUBLISH_DEFAULT_BRANCH:-} status
    require_tools gh
    [ -n "$branch" ] || fail "LAYERX_PUBLISH_DEFAULT_BRANCH is unset: a release candidate is published only from the default branch"
    status=$(gh api "repos/$GITHUB_REPOSITORY/compare/$branch...$RELEASE_COMMIT" --jq '.status') \
        || fail "cannot compare release candidate $RELEASE_COMMIT with $branch"
    case $status in
    identical | behind) ;;
    *) fail "release candidate $RELEASE_COMMIT is $status relative to $branch: only a commit on $branch is published" ;;
    esac
}

regexp_quote() {
    local text=$1 out="" index character
    for ((index = 0; index < ${#text}; index++)); do
        character=${text:index:1}
        case $character in
        [A-Za-z0-9/_-]) out+=$character ;;
        *) out+="\\$character" ;;
        esac
    done
    printf '%s' "$out"
}

signer_identity_regexp() {
    local server=${GITHUB_SERVER_URL:-https://github.com}
    printf '^%s/%s/%s@refs/' \
        "$(regexp_quote "$server")" "$(regexp_quote "$GITHUB_REPOSITORY")" "$(regexp_quote "$PUBLISH_WORKFLOW")"
}

release_tags() {
    [ "$RELEASE_KIND" != tag ] || printf '%s\n' "$RELEASE_NAME"
    printf '%s\n' "$MOVING_TAG"
}

sbom_path() { printf '%s/%s.sbom.spdx.json' "$OUTPUT_DIR" "$1"; }

generate_sboms() {
    local name ref sbom
    mkdir -p "$OUTPUT_DIR"
    for name in "${IMAGE_NAMES[@]}"; do
        ref=${IMAGE_REFS[$name]}
        sbom=$(sbom_path "$name")
        log "generating the SPDX SBOM of $ref"
        syft scan "docker:$ref" --output "spdx-json=$sbom" > "$OUTPUT_DIR/$name.sbom.log" 2>&1 \
            || { tail -n 20 "$OUTPUT_DIR/$name.sbom.log" >&2; fail "SBOM generation failed for $name"; }
        jq -e --arg image "${ref%:*}" '
            .SPDXID == "SPDXRef-DOCUMENT"
            and (.spdxVersion | startswith("SPDX-"))
            and .name == $image
            and ((.packages | length) > 0)
        ' "$sbom" >/dev/null \
            || fail "the SBOM of $name is not an SPDX document describing ${ref%:*} with packages"
    done
}

write_publication_record() {
    local record="$OUTPUT_DIR/publication.txt" name
    mkdir -p "$OUTPUT_DIR"
    {
        printf 'registry=%s\n' "$REGISTRY_ORG"
        printf 'release_kind=%s\n' "$RELEASE_KIND"
        printf 'release_name=%s\n' "$RELEASE_NAME"
        printf 'release_commit=%s\n' "$RELEASE_COMMIT"
        printf 'build_revision=%s\n' "$BUILD_REVISION"
        printf 'publish_tag=%s\n' "$PUBLISH_TAG"
        printf 'moving_tag=%s\n' "$MOVING_TAG"
        printf 'release_tags=%s\n' "$(release_tags | tr '\n' ' ' | sed 's/ $//')"
        printf 'sbom_generator=syft scan\n'
        printf 'sbom_predicate_type=%s\n' "$SBOM_PREDICATE_TYPE"
        printf 'provenance_predicate_type=%s\n' "$PROVENANCE_PREDICATE_TYPE"
        printf 'images=%s\n' "${IMAGE_NAMES[*]}"
        if [ -n "${GITHUB_WORKFLOW_REF:-}" ]; then printf 'workflow_ref=%s\n' "$GITHUB_WORKFLOW_REF"; fi
        if [ -n "${GITHUB_RUN_ID:-}" ]; then printf 'run=%s/%s\n' "$GITHUB_RUN_ID" "${GITHUB_RUN_ATTEMPT:-0}"; fi
        for name in "${IMAGE_NAMES[@]}"; do
            printf 'source %s %s %s\n' "$name" "${IMAGE_REFS[$name]}" "${IMAGE_IDS[$name]}"
        done
    } > "$record"
    cat "$record"
}

publish_plan() {
    local name target tag
    for name in "${IMAGE_NAMES[@]}"; do
        target=${IMAGE_TARGETS[$name]}
        printf 'push %s:%s from %s (%s)\n' "$target" "$PUBLISH_TAG" "${IMAGE_REFS[$name]}" "${IMAGE_IDS[$name]}"
        printf 'sign %s@<digest> keyless as %s\n' "$target" "$OIDC_ISSUER"
        printf 'attest %s@<digest> %s from %s\n' "$target" "$SBOM_PREDICATE_TYPE" "$(sbom_path "$name")"
        printf 'attest %s@<digest> %s\n' "$target" "$PROVENANCE_PREDICATE_TYPE"
        while read -r tag; do
            printf 'promote %s:%s after verification\n' "$target" "$tag"
        done < <(release_tags)
    done
}

push_images() {
    local name target digest
    mkdir -p "$OUTPUT_DIR"
    : > "$OUTPUT_DIR/digests.txt"
    for name in "${IMAGE_NAMES[@]}"; do
        target=${IMAGE_TARGETS[$name]}
        docker tag "${IMAGE_IDS[$name]}" "$target:$PUBLISH_TAG"
        docker push "$target:$PUBLISH_TAG"
        digest=$(registry_image_digest "$target:$PUBLISH_TAG") || fail "no registry manifest for $name"
        docker image inspect "${IMAGE_IDS[$name]}" --format '{{json .RepoDigests}}' \
            | jq -e --arg expected "$target@$digest" 'index($expected) != null' >/dev/null \
            || fail "registry manifest differs from pushed image $name"
        IMAGE_DIGESTS[$name]=$digest
        printf '%s %s %s\n' "$name" "$target" "$digest" >> "$OUTPUT_DIR/digests.txt"
        log "pushed $target:$PUBLISH_TAG at $digest"
    done
}

sign_images() {
    local name target digest
    for name in "${IMAGE_NAMES[@]}"; do
        target=${IMAGE_TARGETS[$name]}
        digest=${IMAGE_DIGESTS[$name]}
        cosign sign --yes "$target@$digest"
        cosign attest --yes --type "$SBOM_PREDICATE_TYPE" --predicate "$(sbom_path "$name")" "$target@$digest"
        log "signed $target@$digest and attached its $SBOM_PREDICATE_TYPE SBOM"
    done
}

read_digests() {
    local digests="$OUTPUT_DIR/digests.txt" name target digest extra count=0
    [ -f "$digests" ] && [ ! -L "$digests" ] || fail "missing regular publication digest record $digests"
    while read -r name target digest extra; do
        [ -n "$name" ] && [ -z "$extra" ] || fail "malformed publication digest record"
        [ -n "${IMAGE_TARGETS[$name]:-}" ] || fail "unknown image $name in the publication digest record"
        [ "$target" = "${IMAGE_TARGETS[$name]}" ] || fail "unexpected published repository for $name"
        [[ $digest =~ ^sha256:[0-9a-f]{64}$ ]] || fail "invalid published digest for $name"
        [ -z "${IMAGE_DIGESTS[$name]+present}" ] || fail "duplicate published digest for $name"
        IMAGE_DIGESTS[$name]=$digest
        count=$((count + 1))
    done < "$digests"
    [ "$count" -eq "${#IMAGE_NAMES[@]}" ] || fail "the publication digest record must cover every beta image"
}

verify_published() {
    local identity name target digest sbom attested built
    identity=$(signer_identity_regexp)
    for name in "${IMAGE_NAMES[@]}"; do
        target=${IMAGE_TARGETS[$name]}
        digest=${IMAGE_DIGESTS[$name]}
        sbom=$(sbom_path "$name")
        [ -f "$sbom" ] || fail "missing the SBOM of $name at $sbom"
        [ "$(registry_image_digest "$target:$PUBLISH_TAG")" = "$digest" ] \
            || fail "$target:$PUBLISH_TAG no longer resolves to the published digest $digest"
        cosign verify \
            --certificate-identity-regexp "$identity" \
            --certificate-oidc-issuer "$OIDC_ISSUER" \
            "$target@$digest" > "$OUTPUT_DIR/$name.signature.json" \
            || fail "no signature by $PUBLISH_WORKFLOW for $target@$digest"
        cosign verify-attestation \
            --type "$SBOM_PREDICATE_TYPE" \
            --certificate-identity-regexp "$identity" \
            --certificate-oidc-issuer "$OIDC_ISSUER" \
            "$target@$digest" > "$OUTPUT_DIR/$name.sbom-attestation.json" \
            || fail "no $SBOM_PREDICATE_TYPE SBOM attestation by $PUBLISH_WORKFLOW for $target@$digest"
        [ "$(jq -s 'length' "$OUTPUT_DIR/$name.sbom-attestation.json")" -eq 1 ] \
            || fail "expected exactly one verified SBOM attestation for $name"
        attested=$(jq -r '.payload' "$OUTPUT_DIR/$name.sbom-attestation.json" | base64 -d \
            | jq -S -c '[.predicate.packages[] | {name, versionInfo}] | sort')
        built=$(jq -S -c '[.packages[] | {name, versionInfo}] | sort' "$sbom")
        [ "$attested" = "$built" ] \
            || fail "the published SBOM attestation of $name does not describe the SBOM this build produced"
        gh attestation verify "oci://$target@$digest" \
            --repo "$GITHUB_REPOSITORY" \
            --signer-workflow "$GITHUB_REPOSITORY/$PUBLISH_WORKFLOW" \
            --predicate-type "$PROVENANCE_PREDICATE_TYPE" \
            --source-digest "$RELEASE_COMMIT" \
            --format json > "$OUTPUT_DIR/$name.provenance.json" \
            || fail "no build provenance by $PUBLISH_WORKFLOW at $RELEASE_COMMIT for $target@$digest"
        jq -e --arg subject "$target" --arg digest "${digest#sha256:}" '
            any(.[]; any(.verificationResult.statement.subject[]?; .name == $subject and .digest.sha256 == $digest))
        ' "$OUTPUT_DIR/$name.provenance.json" >/dev/null \
            || fail "the build provenance of $name does not name the published digest"
        log "verified the signature, the SBOM attestation and the build provenance of $target@$digest"
    done
}

promote_tags() {
    local name target digest tag moved published
    for name in "${IMAGE_NAMES[@]}"; do
        target=${IMAGE_TARGETS[$name]}
        digest=${IMAGE_DIGESTS[$name]}
        published=$PUBLISH_TAG
        while read -r tag; do
            cosign copy --force "$target@$digest" "$target:$tag"
            moved=$(registry_image_digest "$target:$tag") || fail "no registry manifest for $target:$tag"
            [ "$moved" = "$digest" ] || fail "$target:$tag resolves to $moved, not the verified digest $digest"
            published="$published,$tag"
        done < <(release_tags)
        printf 'published %s %s %s source=%s %s=%s\n' \
            "$target" "$published" "$digest" "$BUILD_REVISION" "$RELEASE_KIND" "$RELEASE_NAME"
    done
    {
        printf 'verified=true\n'
        printf 'promoted=true\n'
    } >> "$OUTPUT_DIR/publication.txt"
}

mode_check() {
    require_tools docker jq git
    read_inventory
    printf 'publish-images: verified %s local images from %s; no tags or pushes performed\n' \
        "${#IMAGE_REFS[@]}" "$BUILD_REVISION"
}

mode_dry_run() {
    require_tools docker jq git syft
    read_inventory
    resolve_release_binding
    generate_sboms
    write_publication_record
    publish_plan | tee "$OUTPUT_DIR/plan.txt"
    if [ "$BUILD_REVISION" != "$PUBLISH_TAG" ]; then
        log "this tree is modified: a real publication of $BUILD_REVISION is refused"
    fi
    printf 'publish-images: planned %s images from the %s %s; nothing was pushed\n' \
        "${#IMAGE_REFS[@]}" "$RELEASE_KIND" "$RELEASE_NAME"
}

mode_push() {
    require_tools docker jq git syft cosign
    read_inventory
    resolve_release_binding
    require_gated_publication
    generate_sboms
    write_publication_record
    push_images
    sign_images
    printf 'publish-images: pushed, signed and attested %s images at %s from the %s %s\n' \
        "${#IMAGE_REFS[@]}" "$PUBLISH_TAG" "$RELEASE_KIND" "$RELEASE_NAME"
}

mode_verify() {
    local name
    require_tools docker jq git cosign gh
    export GITHUB_REPOSITORY=Sidiora-Labs/LayerX-Network
    for name in "${IMAGE_NAMES[@]}"; do IMAGE_TARGETS[$name]="$REGISTRY_ORG/$name"; done
    read_publication_record
    resolve_release_binding
    [ "$RELEASE_COMMIT" = "$RECORDED_COMMIT" ] \
        || fail "publication and requested release revisions differ"
    [ "$BUILD_REVISION" = "$PUBLISH_TAG" ] || fail "release images were built from a modified tree"
    read_digests
    verify_published
    printf 'publish-images: verified %s immutable images at %s\n' "${#IMAGE_DIGESTS[@]}" "$RELEASE_COMMIT"
}

mode_promote() {
    local name
    require_tools docker jq git cosign gh
    for name in "${IMAGE_NAMES[@]}"; do IMAGE_TARGETS[$name]="$REGISTRY_ORG/$name"; done
    read_publication_record
    resolve_release_binding
    [ "$RELEASE_COMMIT" = "$RECORDED_COMMIT" ] \
        || fail "the publication record was written for $RECORDED_COMMIT, not for the $RELEASE_KIND commit $RELEASE_COMMIT"
    read_digests
    require_gated_publication
    verify_published
    promote_tags
    printf 'publish-images: verified and promoted %s images at %s from the %s %s\n' \
        "${#IMAGE_DIGESTS[@]}" "$PUBLISH_TAG" "$RELEASE_KIND" "$RELEASE_NAME"
}

read_publication_record() {
    local record="$OUTPUT_DIR/publication.txt"
    [ -f "$record" ] && [ ! -L "$record" ] || fail "missing regular publication record $record"
    RECORDED_REGISTRY=$(sed -n 's/^registry=//p' "$record" | tail -n 1)
    RECORDED_COMMIT=$(sed -n 's/^release_commit=//p' "$record" | tail -n 1)
    BUILD_REVISION=$(sed -n 's/^build_revision=//p' "$record" | tail -n 1)
    [[ $BUILD_REVISION =~ ^[0-9a-f]{7,40}(-dirty)?$ ]] || fail "the publication record carries no build revision"
    [[ $RECORDED_COMMIT =~ ^[0-9a-f]{40}$ ]] || fail "the publication record carries no release commit"
    [ "$RECORDED_REGISTRY" = "$REGISTRY_ORG" ] \
        || fail "the publication record was written for $RECORDED_REGISTRY, not for the canonical $REGISTRY_ORG"
    PUBLISH_TAG=${BUILD_REVISION%-dirty}
}

self_test_run() {
    local expected=$1 status output
    shift
    set +e
    output=$(env "$@" 2>&1)
    status=$?
    set -e
    if [ -z "$expected" ]; then
        [ "$status" -eq 0 ] || { printf '%s\n' "$output" >&2; fail "self-test: '$*' was refused"; }
        printf 'self-test: accepted %s\n' "$*"
        return
    fi
    [ "$status" -ne 0 ] || { printf '%s\n' "$output" >&2; fail "self-test: '$*' was accepted; expected the refusal: $expected"; }
    case $output in
    *"$expected"*) printf 'self-test: refused as required: %s\n' "$expected" ;;
    *) printf '%s\n' "$output" >&2; fail "self-test: '$*' failed without the refusal: $expected" ;;
    esac
}

self_test_cleanup() {
    local ref
    for ref in "${SELF_TEST_TAGS[@]:-}"; do
        [ -n "$ref" ] || continue
        docker rmi "$ref" >/dev/null 2>&1 || true
    done
    [ -z "${SELF_TEST_DIR:-}" ] || rm -rf "$SELF_TEST_DIR"
}

mode_self_test() {
    local head_commit unrelated dirty_inventory name canonical ref id
    require_tools docker jq git syft
    read_inventory
    head_commit=$(git -C "$REPO_ROOT" rev-parse HEAD)
    unrelated=$(git -C "$REPO_ROOT" rev-parse "HEAD~1")
    SELF_TEST_DIR=$(mktemp -d)
    SELF_TEST_TAGS=()
    trap self_test_cleanup EXIT

    cp "$INVENTORY" "$SELF_TEST_DIR/valid"
    cp "$INVENTORY" "$SELF_TEST_DIR/duplicate"
    head -n 1 "$INVENTORY" >> "$SELF_TEST_DIR/duplicate"
    sed '$d' "$INVENTORY" > "$SELF_TEST_DIR/short"
    sed '1s#ghcr.io/sidiora-labs#ghcr.io/impostor#' "$INVENTORY" > "$SELF_TEST_DIR/canonical"
    sed '1s/sha256:[0-9a-f]\{64\}/sha256:not-a-digest/' "$INVENTORY" > "$SELF_TEST_DIR/id"
    awk 'NR==1 { sub(/sha256:[0-9a-f]{64}$/, "sha256:0000000000000000000000000000000000000000000000000000000000000000") } { print }' \
        "$INVENTORY" > "$SELF_TEST_DIR/mismatch"
    awk -v tag="$BUILD_REVISION" 'NR==2 { sub(":" tag "$", ":deadbeefcafe", $3) } { print $1, $2, $3, $4 }' \
        "$INVENTORY" > "$SELF_TEST_DIR/revisions"

    self_test_run "" bash "$SCRIPT_PATH" --check "$SELF_TEST_DIR/valid"
    self_test_run "duplicate image" bash "$SCRIPT_PATH" --check "$SELF_TEST_DIR/duplicate"
    self_test_run "inventory must contain every beta image" bash "$SCRIPT_PATH" --check "$SELF_TEST_DIR/short"
    self_test_run "unexpected canonical image" bash "$SCRIPT_PATH" --check "$SELF_TEST_DIR/canonical"
    self_test_run "invalid image ID" bash "$SCRIPT_PATH" --check "$SELF_TEST_DIR/id"
    self_test_run "differs from build inventory" bash "$SCRIPT_PATH" --check "$SELF_TEST_DIR/mismatch"
    self_test_run "images were built from different revisions" bash "$SCRIPT_PATH" --check "$SELF_TEST_DIR/revisions"
    self_test_run "a mode is required" bash "$SCRIPT_PATH" "$SELF_TEST_DIR/valid"
    self_test_run "only one mode may be given" bash "$SCRIPT_PATH" --check --dry-run "$SELF_TEST_DIR/valid"

    self_test_run "exactly one of --release-tag and --release-candidate is required" \
        bash "$SCRIPT_PATH" --dry-run "$SELF_TEST_DIR/valid"
    self_test_run "exactly one of --release-tag and --release-candidate is required" \
        bash "$SCRIPT_PATH" --dry-run --release-tag beta --release-candidate "$head_commit" "$SELF_TEST_DIR/valid"
    self_test_run "is not a 40-hex commit" \
        bash "$SCRIPT_PATH" --dry-run --release-candidate "${head_commit:0:12}" "$SELF_TEST_DIR/valid"
    self_test_run "not from the candidate commit" \
        bash "$SCRIPT_PATH" --dry-run --release-candidate "$unrelated" "$SELF_TEST_DIR/valid"
    self_test_run "does not name a commit in this repository" \
        bash "$SCRIPT_PATH" --dry-run --release-tag no-such-release-tag "$SELF_TEST_DIR/valid"

    self_test_run "published only from the gated $PUBLISH_WORKFLOW job" \
        -u GITHUB_ACTIONS bash "$SCRIPT_PATH" --phase push --release-candidate "$head_commit" "$SELF_TEST_DIR/valid"
    self_test_run "LAYERX_PUBLISH_GATE_REVISION is unset" \
        -u LAYERX_PUBLISH_GATE_REVISION GITHUB_ACTIONS=true GITHUB_REPOSITORY=Sidiora-Labs/LayerX-Network \
        bash "$SCRIPT_PATH" --phase push --release-candidate "$head_commit" "$SELF_TEST_DIR/valid"
    self_test_run "not on the published revision" \
        GITHUB_ACTIONS=true GITHUB_REPOSITORY=Sidiora-Labs/LayerX-Network "LAYERX_PUBLISH_GATE_REVISION=$unrelated" \
        bash "$SCRIPT_PATH" --phase push --release-candidate "$head_commit" "$SELF_TEST_DIR/valid"

    dirty_inventory="$SELF_TEST_DIR/dirty"
    : > "$dirty_inventory"
    while read -r name canonical ref id; do
        docker tag "$id" "${ref}-dirty"
        SELF_TEST_TAGS+=("${ref}-dirty")
        printf '%s %s %s %s\n' "$name" "$canonical" "${ref}-dirty" "$id" >> "$dirty_inventory"
    done < "$SELF_TEST_DIR/valid"
    self_test_run "only a clean checkout of the release revision is published" \
        GITHUB_ACTIONS=true GITHUB_REPOSITORY=Sidiora-Labs/LayerX-Network "LAYERX_PUBLISH_GATE_REVISION=$head_commit" \
        bash "$SCRIPT_PATH" --phase push --release-candidate "$head_commit" "$dirty_inventory"

    self_test_run "" bash "$SCRIPT_PATH" --dry-run --release-candidate "$head_commit" \
        --output "$SELF_TEST_DIR/publication" "$SELF_TEST_DIR/valid"
    for name in "${IMAGE_NAMES[@]}"; do
        [ -s "$SELF_TEST_DIR/publication/$name.sbom.spdx.json" ] || fail "self-test: the dry run produced no SBOM for $name"
    done
    grep -q "^promote $REGISTRY_ORG/layerx-node:$MOVING_TAG after verification\$" "$SELF_TEST_DIR/publication/plan.txt" \
        || fail "self-test: the dry-run plan does not promote the moving tag after verification"
    grep -q "^attest $REGISTRY_ORG/layerx-node@<digest> $PROVENANCE_PREDICATE_TYPE\$" "$SELF_TEST_DIR/publication/plan.txt" \
        || fail "self-test: the dry-run plan does not attest the build provenance"

    printf 'publish-images: self-test passed\n'
}

publish_images() {
    parse_arguments "$@"
    case $MODE in
    check) mode_check ;;
    dry-run) mode_dry_run ;;
    push) mode_push ;;
    verify) mode_verify ;;
    promote) mode_promote ;;
    self-test) mode_self_test ;;
    *) fail "unhandled mode $MODE" ;;
    esac
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
    publish_images "$@"
fi
