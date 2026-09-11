#!/bin/sh
set -eu

usage() {
    cat <<'USAGE'
usage: examples-check.sh

Builds the seller middleware the public RPC examples import and runs both
examples as real programs. The Node example is driven through
platform/middleware/seller/dist and the Python example through
agent/sdk/python; each reads its endpoint from the environment and refuses the
persistent host chain by name.

The live gate examples-live-check.sh is then parsed and its whole input
contract is exercised without a cluster: it must print its usage, it must name
every missing input, it must refuse the persistent host chain on the gateway
and on the faucet endpoint, it must refuse one signed activity offered to both
examples, it must refuse an unreadable input file and it must refuse a bearer
file that holds no bearer. Each of those refusals happens before the gate
builds anything or contacts anything, so the command that needs a cluster is
never the first place its syntax or its input contract is read.

Nothing is deployed and no endpoint is contacted. The deployed surface is
proved by examples-live-check.sh against a beta cluster.
USAGE
}

case "${1:-}" in
    -h|--help) usage; exit 0 ;;
    "") ;;
    *) printf 'examples-check.sh: unknown argument %s\n' "$1" >&2; usage >&2; exit 2 ;;
esac

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
cd "$root"

for tool in node npm python3; do
    command -v "$tool" >/dev/null || { printf 'examples-check.sh: %s is required\n' "$tool" >&2; exit 2; }
done
if [ ! -d node_modules/typescript ]; then
    printf 'examples-check.sh: the workspace dependencies are not installed; run npm ci --workspace @sidiora/layerx-sdk --workspace @sidiora/layerx-seller-middleware --ignore-scripts --no-audit --no-fund\n' >&2
    exit 2
fi

npm run build --workspace @sidiora/layerx-sdk
npm run build --workspace @sidiora/layerx-seller-middleware
for built in agent/sdk/typescript/dist/src/index.js platform/middleware/seller/dist/index.js; do
    [ -s "$built" ] || { printf 'examples-check.sh: %s was not produced by the workspace build\n' "$built" >&2; exit 1; }
done

node --test platform/middleware/conformance/pay6/examples.test.mjs

live=platform/middleware/examples/examples-live-check.sh
sh -n "$live"
sh "$live" --help > /dev/null

REQUIRED_INPUTS='LAYERX_GATEWAY_URL LAYERX_FAUCET_URL LAYERX_TEST_CA_FILE
LAYERX_TEST_AUTH_TOKEN_FILE LAYERX_TEST_SOURCE_DID LAYERX_TEST_SOURCE_PUBLIC_KEY
LAYERX_EXAMPLE_OFFER_FILE LAYERX_EXAMPLE_PAYER LAYERX_EXAMPLE_NODE_ACTIVITY_FILE
LAYERX_EXAMPLE_NODE_AUTHORITY_FILE LAYERX_EXAMPLE_PYTHON_ACTIVITY_FILE
LAYERX_EXAMPLE_PYTHON_AUTHORITY_FILE'
input_count=$(printf '%s\n' $REQUIRED_INPUTS | wc -l | tr -d ' ')

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT INT TERM

expect_refusal() {
    expect_case=$1
    expect_pattern=$2
    expect_status=0
    sh "$work/run" > "$work/out" 2> "$work/err" || expect_status=$?
    if [ "$expect_status" != 2 ]; then
        printf 'examples-check.sh: the live gate exited %s for %s, not 2\n' "$expect_status" "$expect_case" >&2
        sed -n '1,20p' "$work/err" >&2
        exit 1
    fi
    grep -q "$expect_pattern" "$work/err" || {
        printf 'examples-check.sh: the live gate did not name %s for %s\n' "$expect_pattern" "$expect_case" >&2
        sed -n '1,20p' "$work/err" >&2
        exit 1
    }
}

write_run() {
    printf '#!/bin/sh\nexec env' > "$work/run"
    for named in $REQUIRED_INPUTS; do printf ' -u %s' "$named" >> "$work/run"; done
    printf ' PLATFORM_BETA_CLUSTER_ENV=/dev/null' >> "$work/run"
    for assignment in "$@"; do printf " '%s'" "$assignment" >> "$work/run"; done
    printf ' sh %s\n' "$live" >> "$work/run"
}

write_run
expect_refusal "an empty input environment" "$input_count missing input(s)"
for named in $REQUIRED_INPUTS; do
    grep -q "missing input $named" "$work/err" || {
        printf 'examples-check.sh: the live gate did not name the missing input %s\n' "$named" >&2
        sed -n '1,20p' "$work/err" >&2
        exit 1
    }
done

write_run LAYERX_GATEWAY_URL=https://127.0.0.1:18545
expect_refusal "the persistent host chain on the gateway endpoint" persistent-host-chain-forbidden
write_run LAYERX_FAUCET_URL=https://127.0.0.1:18545/
expect_refusal "the persistent host chain on the faucet endpoint" persistent-host-chain-forbidden

printf 'ca\n' > "$work/ca.crt"
printf 'bearer\n' > "$work/token"
printf '\n' > "$work/blank-token"
printf 'payment-required-header\n' > "$work/offer"
printf 'node-activity\n' > "$work/node-activity"
printf 'python-activity\n' > "$work/python-activity"
printf '{}\n' > "$work/node-authority"
printf '{}\n' > "$work/python-authority"

complete_run() {
    write_run \
        LAYERX_GATEWAY_URL=https://gateway.example \
        LAYERX_FAUCET_URL=https://faucet.example \
        "LAYERX_TEST_CA_FILE=$work/ca.crt" \
        "LAYERX_TEST_AUTH_TOKEN_FILE=$1" \
        LAYERX_TEST_SOURCE_DID=did:lxp:example \
        LAYERX_TEST_SOURCE_PUBLIC_KEY=00 \
        "LAYERX_EXAMPLE_OFFER_FILE=$work/offer" \
        LAYERX_EXAMPLE_PAYER=00 \
        "LAYERX_EXAMPLE_NODE_ACTIVITY_FILE=$2" \
        "LAYERX_EXAMPLE_NODE_AUTHORITY_FILE=$work/node-authority" \
        "LAYERX_EXAMPLE_PYTHON_ACTIVITY_FILE=$3" \
        "LAYERX_EXAMPLE_PYTHON_AUTHORITY_FILE=$work/python-authority"
}

complete_run "$work/token" "$work/node-activity" "$work/node-activity"
expect_refusal "one signed activity offered to both examples" "the same signed activity"
complete_run "$work/token" "$work/absent-activity" "$work/python-activity"
expect_refusal "an unreadable input file" "is not a readable non-empty file"
complete_run "$work/blank-token" "$work/node-activity" "$work/python-activity"
expect_refusal "a bearer file that holds no bearer" "holds no bearer"

printf 'examples-check: both public RPC examples ran against the workspace build; the live gate parses, prints its usage, names all %s missing inputs and refuses the persistent host chain, a shared signed activity, an unreadable input and an empty bearer\n' \
    "$input_count"
