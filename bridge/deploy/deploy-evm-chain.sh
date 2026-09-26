#!/usr/bin/env bash
# Deploys the PaxeerXVault of one EVM chain of the Paxeer X Network bridge from
# that chain's configuration and nothing else, then reads the deployment back
# and writes a record of it.
#
# Usage:
#   deploy-evm-chain.sh [--preflight] <chain>
#
# The chain name is the directory under bridge/evm/chains. --preflight runs
# every check that needs no endpoint - the configuration, the environment
# variables it names and the tools it needs - and stops before the first call to
# the chain, so a deployment can be rehearsed offline. The sources and the
# pinned libraries are checked where they are used, when the deployment runs.
#
# Inputs, all through environment variables and never through a literal in this
# script or in the configuration:
#   the variable the configuration names in environment.rpc_url      the endpoint
#   the variable the configuration names in environment.deploy_key   the deployer key
#   PAXEER_BRIDGE_DEPLOYMENT_RECORD   where the deployment record is written
#   PAXEER_BRIDGE_EVM_CHAINS_ROOT     optional chains root, for a run-local
#                                     configuration under <root>/<chain>/config.json
#
# The configuration is the only source of the chain id, the owner, the attestor
# set, the threshold and the caps. Every placeholder, every missing variable,
# every chain id the endpoint does not confirm and every deployed code hash that
# is not the built one stops the run. There is no default endpoint, key, owner
# or address anywhere in this script.
#
# The endpoint reaches cast through ETH_RPC_URL rather than a command argument.
# The pinned foundry offers no environment variable and no standard-input path
# for a raw deploy key, and importing one into a keystore needs a terminal, so
# the key reaches exactly one command - the broadcasting forge script - as an
# argument; run a deployment only where process arguments are not readable by
# another user, or hand foundry a keystore out of band.
#
# bridge/deploy/chainconfig is the schema of a configuration and the authority
# on what a valid one looks like; its Go tests assert every committed file. The
# checks below are the deployment preconditions this script refuses to deploy
# without, enforced where a deployment happens.
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd "$SCRIPT_DIR/../.." && pwd)
EVM_ROOT="$REPO_ROOT/bridge/evm"
PLACEHOLDER_PREFIX='PLACEHOLDER:'
ZERO_ADDRESS=0x0000000000000000000000000000000000000000

fail() {
    printf 'deploy-evm-chain: error: %s\n' "$*" >&2
    exit 1
}

usage() {
    printf 'usage: deploy-evm-chain.sh [--preflight] <chain>\n' >&2
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

read_field() {
    local field=$1 value
    value=$(jq -r --arg field "$field" 'getpath($field | split(".")) // empty' "$config") \
        || refuse "$field is unreadable"
    [ -n "$value" ] || refuse "$field is required"
    printf '%s' "$value"
}

require_variable_name() {
    local field=$1 name=$2
    [[ $name =~ ^[A-Z][A-Z0-9]*(_[A-Z0-9]+)*$ ]] \
        || refuse "$field: $name is not an upper snake case environment variable name"
}

require_address() {
    local field=$1 value=$2
    case $value in
    "$PLACEHOLDER_PREFIX"*)
        refuse "$field: $value is a placeholder; fill in the real value before deploying"
        ;;
    esac
    [[ $value =~ ^0x[0-9a-fA-F]{40}$ ]] || refuse "$field: $value is not a 20-byte address"
    [ "$(lower "$value")" != "$ZERO_ADDRESS" ] || refuse "$field: the zero address is not a value"
}

require_amount() {
    local field=$1 value=$2
    [[ $value =~ ^[1-9][0-9]*$ ]] || refuse "$field: $value is not a cap above zero"
}

lower() { printf '%s' "$1" | tr '[:upper:]' '[:lower:]'; }

kind=$(read_field kind)
[ "$kind" = evm ] || refuse "kind: $kind is not an EVM chain"
declared=$(read_field chain)
[ "$declared" = "$chain" ] || refuse "chain: the file lies in $chain but names $declared"
chain_id=$(read_field chain_id)
[[ $chain_id =~ ^[1-9][0-9]*$ ]] || refuse "chain_id: $chain_id is not a chain id"
native_symbol=$(read_field native.symbol)

rpc_variable=$(read_field environment.rpc_url)
require_variable_name environment.rpc_url "$rpc_variable"
key_variable=$(read_field environment.deploy_key)
require_variable_name environment.deploy_key "$key_variable"

owner=$(read_field owner)
require_address owner "$owner"

threshold=$(read_field threshold)
[[ $threshold =~ ^[1-9][0-9]*$ ]] || refuse "threshold: $threshold is not a threshold above zero"

mapfile -t attestors < <(jq -r '.attestors[]' "$config")
[ "${#attestors[@]}" -gt 0 ] || refuse "attestors: the attestor set is empty"
[ "$threshold" -le "${#attestors[@]}" ] \
    || refuse "threshold: $threshold is above the ${#attestors[@]} attestors of the set"
previous=""
for index in "${!attestors[@]}"; do
    require_address "attestors[$index]" "${attestors[index]}"
    current=$(lower "${attestors[index]}")
    if [ -n "$previous" ] && [[ ! $previous < $current ]]; then
        refuse "attestors[$index]: ${attestors[index]} does not follow the attestor before it; the set is strictly ascending"
    fi
    previous=$current
done

mapfile -t asset_addresses < <(jq -r '.assets[].address' "$config")
mapfile -t asset_symbols < <(jq -r '.assets[].symbol' "$config")
mapfile -t asset_per_tx < <(jq -r '.assets[].per_tx_cap' "$config")
mapfile -t asset_total < <(jq -r '.assets[].total_cap' "$config")
mapfile -t asset_ids < <(jq -r '.assets[].asset_id' "$config")
[ "${#asset_addresses[@]}" -gt 0 ] || refuse "assets: the asset list is empty"
[ "$(lower "${asset_addresses[0]}")" = "$ZERO_ADDRESS" ] \
    || refuse "assets[0].address: the first asset of $chain is its native coin $native_symbol at $ZERO_ADDRESS"
for index in "${!asset_addresses[@]}"; do
    [[ ${asset_addresses[index]} =~ ^0x[0-9a-fA-F]{40}$ ]] \
        || refuse "assets[$index].address: ${asset_addresses[index]} is not a 20-byte address"
    [ "$(lower "${asset_ids[index]}")" = "$(lower "${asset_addresses[index]}")" ] \
        || refuse "assets[$index].asset_id: an asset on $chain enters the digests as its own address"
    require_amount "assets[$index].per_tx_cap" "${asset_per_tx[index]}"
    require_amount "assets[$index].total_cap" "${asset_total[index]}"
done

if jq -e 'has("big_blocks")' "$config" > /dev/null; then
    acknowledged=$(jq -r '.big_blocks.acknowledged' "$config")
    requirement=$(jq -r '.big_blocks.requirement' "$config")
    [ "$acknowledged" = true ] || refuse "big_blocks.acknowledged: $requirement"
fi

for variable in "$rpc_variable" "$key_variable" PAXEER_BRIDGE_DEPLOYMENT_RECORD; do
    [ -n "${!variable:-}" ] || fail "$variable is required and is not set"
done
record=$PAXEER_BRIDGE_DEPLOYMENT_RECORD
record_directory=$(dirname "$record")
[ -d "$record_directory" ] || fail "$record_directory does not exist, so the deployment record cannot be written"
[ -w "$record_directory" ] || fail "$record_directory is not writable, so the deployment record cannot be written"

if [ "$preflight_only" -eq 1 ]; then
    printf 'deploy-evm-chain: %s (chain %s) is ready to deploy: %s owned by %s, %d attestors at threshold %s, %d assets\n' \
        "$chain" "$chain_id" "$native_symbol" "$owner" "${#attestors[@]}" "$threshold" "${#asset_addresses[@]}" >&2
    exit 0
fi

[ -r "$EVM_ROOT/foundry.toml" ] || fail "$EVM_ROOT/foundry.toml is missing"
[ -r "$EVM_ROOT/src/PaxeerXVault.sol" ] || fail "$EVM_ROOT/src/PaxeerXVault.sol is missing"
[ -r "$EVM_ROOT/script/DeployPaxeerXVault.s.sol" ] || fail "$EVM_ROOT/script/DeployPaxeerXVault.s.sol is missing"
[ -x "$EVM_ROOT/bootstrap-libs.sh" ] || [ -r "$EVM_ROOT/bootstrap-libs.sh" ] \
    || fail "$EVM_ROOT/bootstrap-libs.sh is missing, so the pinned libraries cannot be fetched"

rpc=${!rpc_variable}
deploy_key=${!key_variable}
export ETH_RPC_URL="$rpc"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
chmod 0700 "$work"

confirmed_chain_id=$(cast chain-id) || fail "$rpc_variable did not answer eth_chainId"
[ "$confirmed_chain_id" = "$chain_id" ] \
    || fail "$rpc_variable answers for chain $confirmed_chain_id, and $config names chain $chain_id"

bash "$EVM_ROOT/bootstrap-libs.sh" >&2 || fail "the pinned libraries were not bootstrapped"
forge build --root "$EVM_ROOT" >&2 || fail "the vault did not build"

joined() {
    local IFS=,
    printf '%s' "$*"
}

# The one command that broadcasts keeps its endpoint argument: the pinned foundry
# documents no environment variable for a script's endpoint, and a broadcast that
# silently addressed foundry's default endpoint instead of this chain would be a
# worse outcome than the argument. Everything read back above and below reaches
# the chain through ETH_RPC_URL.
PAXEER_BRIDGE_VAULT_OWNER=$owner \
    PAXEER_BRIDGE_VAULT_ATTESTORS=$(joined "${attestors[@]}") \
    PAXEER_BRIDGE_VAULT_THRESHOLD=$threshold \
    PAXEER_BRIDGE_VAULT_ASSETS=$(joined "${asset_addresses[@]}") \
    PAXEER_BRIDGE_VAULT_PER_TX_CAPS=$(joined "${asset_per_tx[@]}") \
    PAXEER_BRIDGE_VAULT_TOTAL_CAPS=$(joined "${asset_total[@]}") \
    FOUNDRY_BROADCAST="$work/broadcast" \
    forge script --root "$EVM_ROOT" script/DeployPaxeerXVault.s.sol:DeployPaxeerXVault \
    --rpc-url "$rpc" --private-key "$deploy_key" --broadcast --slow > "$work/deploy.log" 2>&1 \
    || {
        cat "$work/deploy.log" >&2
        fail "the deployment did not run"
    }

# Only the record this run wrote: a record left under $EVM_ROOT by an earlier
# deployment to the same chain id would pass every read-back below, because the
# bytecode, the attestors and the caps of that vault are the same, and the run
# would name the earlier vault as this deployment.
run="$work/broadcast/DeployPaxeerXVault.s.sol/$chain_id/run-latest.json"
[ -r "$run" ] || {
    cat "$work/deploy.log" >&2
    fail "the deployment wrote no broadcast record under $work/broadcast"
}
vault=$(jq -r '[.transactions[] | select(.transactionType == "CREATE") | .contractAddress] | first // empty' "$run")
[ -n "$vault" ] || fail "the broadcast record names no created contract"
block_hex=$(jq -r '[.receipts[] | select(.contractAddress != null) | .blockNumber] | first // empty' "$run")
[ -n "$block_hex" ] || fail "the broadcast record names no block for the created contract"
block=$(cast to-dec "$block_hex") || fail "the broadcast record carries the block $block_hex"
deployer=$(jq -r '[.transactions[] | .transaction.from] | first // empty' "$run")
[[ $deployer =~ ^0x[0-9a-fA-F]{40}$ ]] || fail "the broadcast record names no deployer"

normalise() { sed -e 's/ \[[^]]*\]//g' -e 's/[][]//g' -e 's/, */,/g'; }

deployed_code=$(cast code "$vault") || fail "the deployed code of $vault is unreadable"
if [ -z "$deployed_code" ] || [ "$deployed_code" = 0x ]; then
    fail "$vault carries no code after the deployment"
fi
deployed_hash=$(cast keccak "$deployed_code")
built_code=$(forge inspect --root "$EVM_ROOT" PaxeerXVault deployedBytecode 2> "$work/inspect.log" \
    | tr -d '"' | grep -oE '0x[0-9a-fA-F]{40,}' | head -n 1) || true
[ -n "$built_code" ] || {
    cat "$work/inspect.log" >&2
    fail "the built deployed bytecode of PaxeerXVault is unreadable"
}
built_hash=$(cast keccak "$built_code")
[ "$deployed_hash" = "$built_hash" ] \
    || fail "the code at $vault hashes to $deployed_hash and the built vault hashes to $built_hash"

deployed_owner=$(cast call "$vault" 'owner()(address)' | normalise)
deployed_threshold=$(cast call "$vault" 'threshold()(uint256)' | normalise)
[ "$deployed_threshold" = "$threshold" ] \
    || fail "$vault carries the threshold $deployed_threshold, and $config names $threshold"
deployed_attestors=$(cast call "$vault" 'attestors()(address[])' | normalise)
expected_attestors=$(lower "$(joined "${attestors[@]}")")
[ "$(lower "$deployed_attestors")" = "$expected_attestors" ] \
    || fail "$vault carries the attestor set $deployed_attestors, and $config names $expected_attestors"

caps=$work/caps.json
printf '[]' > "$caps"
for index in "${!asset_addresses[@]}"; do
    asset=${asset_addresses[index]}
    mapfile -t reported < <(cast call "$vault" 'caps(address)(uint256,uint256)' "$asset" | normalise)
    [ "${#reported[@]}" -eq 2 ] || fail "$vault reported no cap pair for $asset"
    [ "${reported[0]}" = "${asset_per_tx[index]}" ] \
        || fail "$vault caps $asset at ${reported[0]} per transaction, and $config names ${asset_per_tx[index]}"
    [ "${reported[1]}" = "${asset_total[index]}" ] \
        || fail "$vault caps $asset at ${reported[1]} in total, and $config names ${asset_total[index]}"
    jq --arg asset "$asset" --arg symbol "${asset_symbols[index]}" \
        --arg per_tx "${reported[0]}" --arg total "${reported[1]}" \
        '. + [{asset: $asset, symbol: $symbol, per_tx_cap: $per_tx, total_cap: $total}]' \
        "$caps" > "$caps.next"
    mv "$caps.next" "$caps"
done

if [ "$(lower "$deployed_owner")" = "$(lower "$owner")" ]; then
    ownership=held
else
    ownership=proposed
fi

umask 077
jq -n --arg chain "$chain" --argjson chain_id "$chain_id" --arg kind evm \
    --arg configuration "${config#"$REPO_ROOT"/}" \
    --arg vault "$vault" --arg deployer "$deployer" --arg code_hash "$deployed_hash" \
    --argjson block "$block" --arg owner "$deployed_owner" --arg configured_owner "$owner" \
    --arg ownership "$ownership" --argjson threshold "$deployed_threshold" \
    --argjson attestors "$(jq -n --arg list "$deployed_attestors" '$list | split(",")')" \
    --slurpfile caps "$caps" \
    '{chain: $chain, chain_id: $chain_id, kind: $kind, configuration: $configuration,
      vault: $vault, deployer: $deployer, code_hash: $code_hash, block: $block,
      owner: $owner, configured_owner: $configured_owner, ownership: $ownership,
      threshold: $threshold, attestors: $attestors, caps: $caps[0]}' > "$record"

printf 'deploy-evm-chain: %s vault %s deployed by %s in block %s, code %s\n' \
    "$chain" "$vault" "$deployer" "$block" "$deployed_hash" >&2
if [ "$(lower "$deployed_owner")" != "$(lower "$owner")" ]; then
    printf 'deploy-evm-chain: ownership is proposed to %s and is not held until that account accepts it\n' \
        "$owner" >&2
fi
printf 'deploy-evm-chain: record written to %s\n' "$record" >&2
