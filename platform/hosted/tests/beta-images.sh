#!/usr/bin/env bash

IMAGE_NAMES=(layerx-testnet-control layerx-gateway layerx-faucet layerx-program-registry layerx-webhooks layerx-dashboard layerx-dashboard-web
    layerx-internal layerx-human layerx-node layerx-core-boundary layerx-receipt-authority layerx-agent-boundary layerx-identity layerx-paxeer-boundary
    layerx-mirror paxd-node paxd)

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
        layerx-node) printf 'ghcr.io/sidiora-labs/layerx-node:0.1.0 platform/hosted/node/Dockerfile' ;;
        layerx-core-boundary) printf 'ghcr.io/sidiora-labs/layerx-core-boundary:0.1.0 platform/hosted/core/Dockerfile' ;;
        layerx-receipt-authority) printf 'ghcr.io/sidiora-labs/layerx-receipt-authority:0.1.0 platform/hosted/authority/Dockerfile' ;;
        layerx-agent-boundary) printf 'ghcr.io/sidiora-labs/layerx-agent-boundary:0.1.0 platform/hosted/agent-boundary/Dockerfile' ;;
        layerx-identity) printf 'ghcr.io/sidiora-labs/layerx-identity:0.1.0 platform/hosted/identity/Dockerfile' ;;
        layerx-paxeer-boundary) printf 'ghcr.io/sidiora-labs/layerx-paxeer-boundary:0.1.0 platform/hosted/paxeer/Dockerfile' ;;
        layerx-mirror) printf 'ghcr.io/sidiora-labs/layerx-mirror:0.1.0 interop/deploy/mirror/Dockerfile' ;;
        paxd-node) printf 'ghcr.io/sidiora-labs/paxd-node:0.1.0 platform/hosted/paxeer/Dockerfile.paxd-node' ;;
        paxd) printf 'ghcr.io/sidiora-labs/paxd:0.1.0 platform/hosted/paxeer/Dockerfile.paxd' ;;
        *) fail "unknown image $1" ;;
    esac
}


image_build_args() {
    case "$1" in
        layerx-node) printf -- '--build-arg LXP_REVISION=%s' "$REVISION" ;;
        paxd-node) printf -- '--build-arg PAX_CHAIN_REF=%s' "$REVISION" ;;
        paxd) printf -- '--build-arg PAXD_IMAGE=%s' "$(image_ref paxd-node)" ;;
        *) ;;
    esac
}

registry_image_digest() {
    local manifest digest
    manifest=$(docker manifest inspect --verbose "$1") || return 1
    digest=$(jq -er '
        if type == "object" then .Descriptor.digest
        else error("expected a single-platform beta image manifest") end
        | select(type == "string" and test("^sha256:[0-9a-f]{64}$"))
    ' <<<"$manifest") || return 1
    printf '%s\n' "$digest"
}
