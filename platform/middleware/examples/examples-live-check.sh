#!/bin/sh
set -eu
umask 077

usage() {
    cat <<'USAGE'
usage: examples-live-check.sh

Runs the two public RPC examples against the live gateway of a disposable beta
cluster. The seller middleware the Node example imports is built from the
workspace first, then each example runs once against the cluster: it reads the
account sequence over the public JSON-RPC, claims faucet funding, submits its
signed payment activity and verifies the returned payment against executed or
batched commitment through the trusted authority it was given. A pending
verification is not a proof and fails the gate.

The cluster endpoints, its internal CA, the funded smoke DID and the bearer the
gateway and the faucet admit are read from the cluster environment file that
platform/hosted/tests/beta-cluster.sh up writes, PLATFORM_BETA_CLUSTER_ENV
(default build/beta-cluster/env), or from the environment when that file is not
readable:

  LAYERX_GATEWAY_URL             gateway base URL; JSON-RPC is served at /rpc
  LAYERX_FAUCET_URL              faucet base URL; claims go to /v1/faucet/claims
  LAYERX_TEST_CA_FILE            internal CA both examples trust
  LAYERX_TEST_AUTH_TOKEN_FILE    file holding the bearer for RPC submission and
                                 faucet claims
  LAYERX_TEST_SOURCE_DID         funded DID whose sequence both examples read
  LAYERX_TEST_SOURCE_PUBLIC_KEY  32-byte hex public key of that DID

The payment material is produced by the operator who funded that DID; the
cluster bring-up does not write it:

  LAYERX_EXAMPLE_OFFER_FILE             payment-required header of the offer
  LAYERX_EXAMPLE_PAYER                  32-byte hex payer account
  LAYERX_EXAMPLE_NODE_ACTIVITY_FILE     signed activity for the Node example
  LAYERX_EXAMPLE_NODE_AUTHORITY_FILE    trusted authority for that activity
  LAYERX_EXAMPLE_PYTHON_ACTIVITY_FILE   signed activity for the Python example
  LAYERX_EXAMPLE_PYTHON_AUTHORITY_FILE  trusted authority for that activity

Every input is required and no leg is skipped. Each example needs its own
signed activity: a signed activity is consumed by the submission that carries
it, so one file cannot serve both runs. Each faucet claim carries an
idempotency key generated for this run.

The persistent host chain on port 18545 is refused before anything else, so
this gate only ever runs against a disposable cluster.
USAGE
}

case "${1:-}" in
    -h|--help) usage; exit 0 ;;
    "") ;;
    *) printf 'examples-live-check.sh: unknown argument %s\n' "$1" >&2; usage >&2; exit 2 ;;
esac

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
cd "$root"

env_file=${PLATFORM_BETA_CLUSTER_ENV:-build/beta-cluster/env}
if [ -r "$env_file" ]; then
    # shellcheck disable=SC1090
    . "$env_file"
fi

refuse_persistent_chain() {
    case "${2:-}" in
        *://*:18545 | *://*:18545/*)
            printf 'examples-live-check.sh: %s names the persistent host chain port 18545: persistent-host-chain-forbidden\n' "$1" >&2
            exit 2
            ;;
    esac
}
refuse_persistent_chain LAYERX_GATEWAY_URL "${LAYERX_GATEWAY_URL:-}"
refuse_persistent_chain LAYERX_FAUCET_URL "${LAYERX_FAUCET_URL:-}"

missing=0
require() {
    eval "value=\${$1:-}"
    if [ -z "$value" ]; then
        printf 'examples-live-check.sh: missing input %s: %s\n' "$1" "$2" >&2
        missing=$((missing + 1))
    fi
}
require LAYERX_GATEWAY_URL "the gateway base URL of the beta cluster"
require LAYERX_FAUCET_URL "the faucet base URL of the beta cluster"
require LAYERX_TEST_CA_FILE "the internal CA file of the beta cluster"
require LAYERX_TEST_AUTH_TOKEN_FILE "the file holding the bearer the gateway and the faucet admit"
require LAYERX_TEST_SOURCE_DID "the funded DID whose account sequence both examples read"
require LAYERX_TEST_SOURCE_PUBLIC_KEY "the 32-byte hex public key of the funded DID"
require LAYERX_EXAMPLE_OFFER_FILE "the payment-required header the seller issued for the offer"
require LAYERX_EXAMPLE_PAYER "the 32-byte hex payer account the payment debits"
require LAYERX_EXAMPLE_NODE_ACTIVITY_FILE "the signed activity the Node example submits"
require LAYERX_EXAMPLE_NODE_AUTHORITY_FILE "the trusted authority for the Node example's activity"
require LAYERX_EXAMPLE_PYTHON_ACTIVITY_FILE "the signed activity the Python example submits"
require LAYERX_EXAMPLE_PYTHON_AUTHORITY_FILE "the trusted authority for the Python example's activity"
if [ "$missing" -gt 0 ]; then
    printf 'examples-live-check.sh: %d missing input(s); supply them in the environment or in %s, the cluster environment file the beta cluster bring-up writes\n' \
        "$missing" "$env_file" >&2
    exit 2
fi

if [ "$LAYERX_EXAMPLE_NODE_ACTIVITY_FILE" = "$LAYERX_EXAMPLE_PYTHON_ACTIVITY_FILE" ]; then
    printf 'examples-live-check.sh: the two examples were given the same signed activity; a signed activity is consumed by one submission\n' >&2
    exit 2
fi
for file in "$LAYERX_TEST_CA_FILE" "$LAYERX_TEST_AUTH_TOKEN_FILE" "$LAYERX_EXAMPLE_OFFER_FILE" \
    "$LAYERX_EXAMPLE_NODE_ACTIVITY_FILE" "$LAYERX_EXAMPLE_NODE_AUTHORITY_FILE" \
    "$LAYERX_EXAMPLE_PYTHON_ACTIVITY_FILE" "$LAYERX_EXAMPLE_PYTHON_AUTHORITY_FILE"; do
    [ -f "$file" ] && [ -r "$file" ] && [ -s "$file" ] || {
        printf 'examples-live-check.sh: %s is not a readable non-empty file\n' "$file" >&2
        exit 2
    }
done
bearer=$(tr -d '\r\n' < "$LAYERX_TEST_AUTH_TOKEN_FILE")
if [ -z "$bearer" ]; then
    printf 'examples-live-check.sh: %s holds no bearer\n' "$LAYERX_TEST_AUTH_TOKEN_FILE" >&2
    exit 2
fi

rpc_url=${LAYERX_GATEWAY_URL%/}/rpc
faucet_url=${LAYERX_FAUCET_URL%/}/v1/faucet/claims
stamp=$(date -u +%Y%m%dT%H%M%SZ)

for tool in node npm python3; do
    command -v "$tool" >/dev/null || { printf 'examples-live-check.sh: %s is required\n' "$tool" >&2; exit 2; }
done
if [ ! -d node_modules/typescript ]; then
    printf 'examples-live-check.sh: the workspace dependencies are not installed; run npm ci --workspace @sidiora/layerx-sdk --workspace @sidiora/layerx-seller-middleware --ignore-scripts --no-audit --no-fund\n' >&2
    exit 2
fi
python3 -c 'import cryptography' 2>/dev/null || {
    printf 'examples-live-check.sh: the Python example verifies the receipt signature through cryptography; install it for the interpreter that runs this gate\n' >&2
    exit 2
}

npm run build --workspace @sidiora/layerx-sdk
npm run build --workspace @sidiora/layerx-seller-middleware
for built in agent/sdk/typescript/dist/src/index.js platform/middleware/seller/dist/index.js; do
    [ -s "$built" ] || { printf 'examples-live-check.sh: %s was not produced by the workspace build\n' "$built" >&2; exit 1; }
done

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT INT TERM

LAYERX_RPC_TOKEN=$bearer
LAYERX_FAUCET_TOKEN=$bearer
export LAYERX_RPC_TOKEN LAYERX_FAUCET_TOKEN

assert_legs() {
    assert_name=$1
    assert_out=$2
    grep -q 'Faucet HTTP status: 200;' "$assert_out" || {
        printf 'examples-live-check.sh: the %s example did not report a funded faucet claim\n' "$assert_name" >&2
        exit 1
    }
    grep -q '"sequence"' "$assert_out" || {
        printf 'examples-live-check.sh: the %s example did not report the account sequence\n' "$assert_name" >&2
        exit 1
    }
    grep -q '"transaction"' "$assert_out" && grep -q 'lxp:' "$assert_out" || {
        printf 'examples-live-check.sh: the %s example did not report a verified payment\n' "$assert_name" >&2
        exit 1
    }
}

run_example() {
    run_name=$1
    shift
    run_status=0
    "$@" > "$work/$run_name.out" 2> "$work/$run_name.err" || run_status=$?
    sed "s/^/examples-live-check: $run_name: /" "$work/$run_name.out"
    if [ "$run_status" != 0 ]; then
        if grep -q '"state": *"pending"' "$work/$run_name.out"; then
            printf 'examples-live-check.sh: the %s example left its payment pending; a pending verification is not a proof\n' "$run_name" >&2
        else
            printf 'examples-live-check.sh: the %s example exited %s\n' "$run_name" "$run_status" >&2
        fi
        sed -n '1,20p' "$work/$run_name.err" >&2
        exit 1
    fi
    assert_legs "$run_name" "$work/$run_name.out"
}

run_example node env NODE_EXTRA_CA_CERTS="$LAYERX_TEST_CA_FILE" \
    node platform/middleware/examples/public-rpc.mjs \
    --rpc "$rpc_url" --did "$LAYERX_TEST_SOURCE_DID" \
    --faucet "$faucet_url" --public-key "$LAYERX_TEST_SOURCE_PUBLIC_KEY" \
    --claim-key "examples-node-$stamp-$$" \
    --activity "$LAYERX_EXAMPLE_NODE_ACTIVITY_FILE" --offer "$LAYERX_EXAMPLE_OFFER_FILE" \
    --authority "$LAYERX_EXAMPLE_NODE_AUTHORITY_FILE" --payer "$LAYERX_EXAMPLE_PAYER"

run_example python env SSL_CERT_FILE="$LAYERX_TEST_CA_FILE" PYTHONPATH=agent/sdk/python \
    python3 platform/middleware/examples/public_rpc.py \
    --rpc "$rpc_url" --did "$LAYERX_TEST_SOURCE_DID" \
    --faucet "$faucet_url" --public-key "$LAYERX_TEST_SOURCE_PUBLIC_KEY" \
    --claim-key "examples-python-$stamp-$$" \
    --activity "$LAYERX_EXAMPLE_PYTHON_ACTIVITY_FILE" --offer "$LAYERX_EXAMPLE_OFFER_FILE" \
    --authority "$LAYERX_EXAMPLE_PYTHON_AUTHORITY_FILE" --payer "$LAYERX_EXAMPLE_PAYER"

printf 'examples-live-check: both public RPC examples read the sequence, claimed faucet funding and verified their payment against %s\n' "$rpc_url"
