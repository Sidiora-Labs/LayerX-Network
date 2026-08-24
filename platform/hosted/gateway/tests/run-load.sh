#!/bin/sh
set -eu

: "${LAYERX_GATEWAY_URL:?real hosted gateway URL is required}"
: "${LAYERX_GATEWAY_CA_FILE:?gateway CA file is required}"
: "${LAYERX_GATEWAY_SESSION:?developer session is required}"
: "${LAYERX_GATEWAY_SIGNER_PUBLIC_KEY:?owned signer public key is required}"
: "${LAYERX_GATEWAY_ACTIVITY_CORPUS:?unique signed activity corpus is required}"

test -r "$LAYERX_GATEWAY_CA_FILE"
test -r "$LAYERX_GATEWAY_ACTIVITY_CORPUS"
command -v k6 >/dev/null

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
SSL_CERT_FILE="$LAYERX_GATEWAY_CA_FILE" k6 run "$script_dir/load.js"
