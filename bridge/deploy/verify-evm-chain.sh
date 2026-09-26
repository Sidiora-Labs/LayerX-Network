#!/usr/bin/env bash
# Submits the PaxeerXVault source of one EVM chain of the Paxeer X Network
# bridge to that chain's explorer, through the key the chain's configuration
# names, and reports the explorer's own answer.
#
# Usage:
#   verify-evm-chain.sh [--preflight] <chain>
#
# --preflight runs every check that needs no explorer - the configuration, the
# environment variables it names and the deployment record - and stops before
# the submission. The sources are checked where they are used, at submission.
#
# Inputs, all through environment variables:
#   the variable the configuration names in environment.explorer_key  the explorer key
#   PAXEER_BRIDGE_DEPLOYMENT_RECORD   the record bridge/deploy/deploy-evm-chain.sh wrote
#   PAXEER_BRIDGE_EVM_CHAINS_ROOT     optional chains root, for a run-local configuration
#
# The vault address and the constructor arguments come from the deployment
# record, so a verification cannot claim a source for a deployment that was
# never made. Nothing here assumes the explorer succeeded: the submission's own
# answer decides, and anything else stops the run.
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd "$SCRIPT_DIR/../.." && pwd)
EVM_ROOT="$REPO_ROOT/bridge/evm"
PLACEHOLDER_PREFIX='PLACEHOLDER:'
CONTRACT=src/PaxeerXVault.sol:PaxeerXVault

fail() {
    printf 'verify-evm-chain: error: %s\n' "$*" >&2
    exit 1
}

usage() {
    printf 'usage: verify-evm-chain.sh [--preflight] <chain>\n' >&2
    exit 2
}

preflight_only=0
case ${1:-} in
--preflight)
    preflight_only=1
    shift
    ;;
-*)
    usage
    ;;
esac
[ $# -eq 1 ] || usage
chain=$1
[[ $chain =~ ^[a-z][a-z0-9]*$ ]] || fail "$chain is not a chain name"

chains_root=${PAXEER_BRIDGE_EVM_CHAINS_ROOT:-$EVM_ROOT/chains}
config="$chains_root/$chain/config.json"
[ -r "$config" ] || fail "$config is not readable; $chain is not a bridge EVM chain"

refuse() { fail "$config: $*"; }

for tool in jq forge cast; do
    command -v "$tool" > /dev/null 2>&1 || fail "$tool is required and is not on the PATH"
done
jq -e . "$config" > /dev/null 2>&1 || refuse "the file is not JSON"

kind=$(jq -r '.kind // empty' "$config")
[ "$kind" = evm ] || refuse "kind: $kind is not an EVM chain"
declared=$(jq -r '.chain // empty' "$config")
[ "$declared" = "$chain" ] || refuse "chain: the file lies in $chain but names $declared"
chain_id=$(jq -r '.chain_id // empty' "$config")
[[ $chain_id =~ ^[1-9][0-9]*$ ]] || refuse "chain_id: $chain_id is not a chain id"
key_variable=$(jq -r '.environment.explorer_key // empty' "$config")
[ -n "$key_variable" ] || refuse "environment.explorer_key is required to verify a source"
[[ $key_variable =~ ^[A-Z][A-Z0-9]*(_[A-Z0-9]+)*$ ]] \
    || refuse "environment.explorer_key: $key_variable is not an upper snake case environment variable name"

for variable in "$key_variable" PAXEER_BRIDGE_DEPLOYMENT_RECORD; do
    [ -n "${!variable:-}" ] || fail "$variable is required and is not set"
done
record=$PAXEER_BRIDGE_DEPLOYMENT_RECORD
[ -r "$record" ] || fail "$record is not readable; deploy the chain before verifying its source"
jq -e . "$record" > /dev/null 2>&1 || fail "$record is not JSON"

recorded_chain=$(jq -r '.chain // empty' "$record")
[ "$recorded_chain" = "$chain" ] || fail "$record records $recorded_chain, not $chain"
recorded_chain_id=$(jq -r '.chain_id // empty' "$record")
[ "$recorded_chain_id" = "$chain_id" ] \
    || fail "$record records chain $recorded_chain_id, and $config names chain $chain_id"
vault=$(jq -r '.vault // empty' "$record")
[[ $vault =~ ^0x[0-9a-fA-F]{40}$ ]] || fail "$record records no vault address"
deployer=$(jq -r '.deployer // empty' "$record")
[[ $deployer =~ ^0x[0-9a-fA-F]{40}$ ]] || fail "$record records no deployer"
threshold=$(jq -r '.threshold // empty' "$record")
[[ $threshold =~ ^[1-9][0-9]*$ ]] || fail "$record records no threshold"
mapfile -t recorded_attestors < <(jq -r '.attestors[]' "$record")
[ "${#recorded_attestors[@]}" -gt 0 ] || fail "$record records no attestor set"
for attestor in "${recorded_attestors[@]}"; do
    case $attestor in
    "$PLACEHOLDER_PREFIX"*)
        fail "$record records the placeholder attestor $attestor"
        ;;
    esac
    [[ $attestor =~ ^0x[0-9a-fA-F]{40}$ ]] || fail "$record records the attestor $attestor"
done
attestors=$(
    IFS=,
    printf '%s' "${recorded_attestors[*]}"
)

constructor_arguments=$(cast abi-encode 'constructor(address,address[],uint256)' \
    "$deployer" "[$attestors]" "$threshold") \
    || fail "the constructor arguments of $vault are not encodable from $record"

if [ "$preflight_only" -eq 1 ]; then
    printf 'verify-evm-chain: %s (chain %s) vault %s is ready to submit through %s\n' \
        "$chain" "$chain_id" "$vault" "$key_variable" >&2
    exit 0
fi

[ -r "$EVM_ROOT/foundry.toml" ] || fail "$EVM_ROOT/foundry.toml is missing"
[ -r "$EVM_ROOT/src/PaxeerXVault.sol" ] || fail "$EVM_ROOT/src/PaxeerXVault.sol is missing"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
chmod 0700 "$work"

# The explorer key reaches forge through ETHERSCAN_API_KEY rather than a command
# argument, so it is not readable in this process's arguments.
export ETHERSCAN_API_KEY="${!key_variable}"
status=0
forge verify-contract --root "$EVM_ROOT" --chain-id "$chain_id" \
    --constructor-args "$constructor_arguments" --watch \
    "$vault" "$CONTRACT" > "$work/verify.log" 2>&1 || status=$?
cat "$work/verify.log" >&2

if [ "$status" -ne 0 ]; then
    fail "the explorer of chain $chain_id did not verify $vault (exit $status); its answer is above"
fi
if ! grep -qiE 'successfully verified|already verified' "$work/verify.log"; then
    fail "the explorer of chain $chain_id did not report $vault as verified; its answer is above"
fi
printf 'verify-evm-chain: %s vault %s is verified by the explorer of chain %s\n' \
    "$chain" "$vault" "$chain_id" >&2
