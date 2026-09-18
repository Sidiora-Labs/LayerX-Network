#!/usr/bin/env bash

IMAGE_NAMES=(layerx-testnet-control layerx-gateway layerx-faucet layerx-program-registry layerx-webhooks layerx-dashboard layerx-dashboard-web
    layerx-internal layerx-human layerx-human-web layerx-node layerx-core-boundary layerx-receipt-authority layerx-agent-boundary layerx-identity layerx-paxeer-boundary
    layerx-mirror layerx-relay-archive layerx-interop-gateway layerx-reference-ramp
    paxd-node paxd)

image_source() {
    case "$1" in
        layerx-testnet-control) printf 'ghcr.io/sidiora-labs/layerx-testnet-control:0.1.0 platform/hosted/testnet/Dockerfile' ;;
        layerx-gateway) printf 'ghcr.io/sidiora-labs/layerx-gateway:0.1.0 platform/hosted/gateway/Dockerfile' ;;
        layerx-faucet) printf 'ghcr.io/sidiora-labs/layerx-faucet:0.1.0 platform/hosted/faucet/Dockerfile' ;;
        layerx-program-registry) printf 'ghcr.io/sidiora-labs/layerx-program-registry:0.1.0 platform/hosted/registry/Dockerfile' ;;
        layerx-webhooks) printf 'ghcr.io/sidiora-labs/layerx-webhooks:0.1.0 platform/hosted/webhooks/Dockerfile' ;;
        layerx-dashboard) printf 'ghcr.io/sidiora-labs/layerx-dashboard:0.1.0 platform/hosted/dashboard/Dockerfile' ;;
        layerx-dashboard-web) printf 'ghcr.io/sidiora-labs/layerx-dashboard-web:0.1.0 platform/hosted/dashboard/web/Dockerfile' ;;
        layerx-internal) printf 'ghcr.io/sidiora-labs/layerx-internal:0.1.0 platform/hosted/internal/Dockerfile' ;;
        layerx-human) printf 'ghcr.io/sidiora-labs/layerx-human:0.1.0 platform/hosted/human/Dockerfile' ;;
        layerx-human-web) printf 'ghcr.io/sidiora-labs/layerx-human-web:0.1.0 human/apps/web/Dockerfile' ;;
        layerx-node) printf 'ghcr.io/sidiora-labs/layerx-node:0.1.0 platform/hosted/node/Dockerfile' ;;
        layerx-core-boundary) printf 'ghcr.io/sidiora-labs/layerx-core-boundary:0.1.0 platform/hosted/core/Dockerfile' ;;
        layerx-receipt-authority) printf 'ghcr.io/sidiora-labs/layerx-receipt-authority:0.1.0 platform/hosted/authority/Dockerfile' ;;
        layerx-agent-boundary) printf 'ghcr.io/sidiora-labs/layerx-agent-boundary:0.1.0 platform/hosted/agent-boundary/Dockerfile' ;;
        layerx-identity) printf 'ghcr.io/sidiora-labs/layerx-identity:0.1.0 platform/hosted/identity/Dockerfile' ;;
        layerx-paxeer-boundary) printf 'ghcr.io/sidiora-labs/layerx-paxeer-boundary:0.1.0 platform/hosted/paxeer/Dockerfile' ;;
        layerx-mirror) printf 'ghcr.io/sidiora-labs/layerx-mirror:0.1.0 interop/deploy/mirror/Dockerfile' ;;
        layerx-relay-archive) printf 'ghcr.io/sidiora-labs/layerx-relay-archive:0.1.0 platform/relay_archive/Dockerfile' ;;
        layerx-interop-gateway) printf 'ghcr.io/sidiora-labs/layerx-interop-gateway:0.1.0 interop/deploy/gateway/Dockerfile' ;;
        layerx-reference-ramp) printf 'ghcr.io/sidiora-labs/layerx-reference-ramp:0.1.0 platform/ramps/Dockerfile' ;;
        paxd-node) printf 'ghcr.io/sidiora-labs/paxd-node:0.1.0 platform/hosted/paxeer/Dockerfile.paxd-node' ;;
        paxd) printf 'ghcr.io/sidiora-labs/paxd:0.1.0 platform/hosted/paxeer/Dockerfile.paxd' ;;
        *) fail "unknown image $1" ;;
    esac
}


image_build_args() {
    case "$1" in
        layerx-node|layerx-relay-archive) printf -- '--build-arg LXP_REVISION=%s' "$REVISION" ;;
        paxd-node) printf -- '--build-arg PAX_CHAIN_REF=%s' "$REVISION" ;;
        paxd) printf -- '--build-arg PAXD_IMAGE=%s' "$(image_ref paxd-node)" ;;
        *) ;;
    esac
}

registry_manifest_digest() {
    jq -er '
        def image_manifest:
            . == "application/vnd.oci.image.manifest.v1+json"
            or . == "application/vnd.docker.distribution.manifest.v2+json";
        def image_index:
            . == "application/vnd.oci.image.index.v1+json"
            or . == "application/vnd.docker.distribution.manifest.list.v2+json";
        def descriptor:
            type == "object"
            and (.digest | type == "string" and length == 71 and test("^sha256:[0-9a-f]{64}$"))
            and (.size | type == "number" and . > 0 and floor == .)
            and (.mediaType | image_manifest or image_index);
        if descriptor and (
            if .mediaType | image_index then
                .schemaVersion == 2
                and (.manifests | type == "array" and length > 0 and all(.[]; descriptor))
            else
                (has("schemaVersion") | not) and (has("manifests") | not)
            end
        ) then .digest else error("invalid registry root manifest descriptor") end
    '
}

registry_image_digest() {
    local manifest digest
    manifest=$(docker buildx imagetools inspect --format '{{json .Manifest}}' "$1") || return 1
    digest=$(registry_manifest_digest <<<"$manifest") || return 1
    if [[ $1 == *@* ]] && [ "${1##*@}" != "$digest" ]; then
        printf 'registry root digest does not match the requested digest\n' >&2
        return 1
    fi
    printf '%s\n' "$digest"
}
