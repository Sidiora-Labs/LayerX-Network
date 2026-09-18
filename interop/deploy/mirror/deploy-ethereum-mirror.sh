#!/usr/bin/env bash
# Deploys interop/contracts/ethereum-mirror/LayerXMirrorArchive.sol against one
# EVM chain and writes the deployment record ethereum-deployment.json requires:
# chain_id, genesis_hash, contract_address, publisher_secp256k1_address,
# runtime_code_keccak256, deployment_transaction and deployment_block_hash.
#
# Inputs (environment variables, all required):
#   LAYERX_MIRROR_RPC_URL             JSON-RPC endpoint the deployment is broadcast through
#   LAYERX_MIRROR_RPC_CA_PEM          PEM trust anchor for that endpoint
#   LAYERX_MIRROR_CHAIN_ID            chain id the endpoint must report
#   LAYERX_MIRROR_DEPLOYER_KEY_FILE   0x-prefixed secp256k1 deployer secret
#   LAYERX_MIRROR_PUBLISHER_ADDRESS   the mirror publisher address the contract is constructed with
#   LAYERX_MIRROR_DEPLOYMENT_RECORD   output path of the deployment record
#   LAYERX_MIRROR_FOUNDRY_BIN         directory holding the pinned forge and cast
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
CONTRACT_DIR=$(cd "$SCRIPT_DIR/../../contracts/ethereum-mirror" && pwd)

fail() { printf 'deploy-ethereum-mirror: error: %s\n' "$*" >&2; exit 1; }

for variable in LAYERX_MIRROR_RPC_URL LAYERX_MIRROR_RPC_CA_PEM LAYERX_MIRROR_CHAIN_ID \
    LAYERX_MIRROR_DEPLOYER_KEY_FILE LAYERX_MIRROR_PUBLISHER_ADDRESS \
    LAYERX_MIRROR_DEPLOYMENT_RECORD LAYERX_MIRROR_FOUNDRY_BIN; do
    [ -n "${!variable:-}" ] || fail "$variable is required"
done
[ -r "$LAYERX_MIRROR_RPC_CA_PEM" ] || fail "LAYERX_MIRROR_RPC_CA_PEM is not readable"
[ -r "$LAYERX_MIRROR_DEPLOYER_KEY_FILE" ] || fail "LAYERX_MIRROR_DEPLOYER_KEY_FILE is not readable"
[[ $LAYERX_MIRROR_PUBLISHER_ADDRESS =~ ^0x[0-9a-fA-F]{40}$ ]] \
    || fail "LAYERX_MIRROR_PUBLISHER_ADDRESS is not an EVM address"
[[ $LAYERX_MIRROR_CHAIN_ID =~ ^[1-9][0-9]*$ ]] || fail "LAYERX_MIRROR_CHAIN_ID is not a chain id"

FORGE="$LAYERX_MIRROR_FOUNDRY_BIN/forge"
CAST="$LAYERX_MIRROR_FOUNDRY_BIN/cast"
[ -x "$FORGE" ] && [ -x "$CAST" ] || fail "forge and cast are not executable in $LAYERX_MIRROR_FOUNDRY_BIN"
export SSL_CERT_FILE="$LAYERX_MIRROR_RPC_CA_PEM"

observed_chain=$("$CAST" chain-id --rpc-url "$LAYERX_MIRROR_RPC_URL") \
    || fail "the RPC endpoint did not answer eth_chainId"
[ "$observed_chain" = "$LAYERX_MIRROR_CHAIN_ID" ] \
    || fail "the endpoint reports chain $observed_chain, not $LAYERX_MIRROR_CHAIN_ID"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
chmod 0700 "$work"

deployer_key=$(cat "$LAYERX_MIRROR_DEPLOYER_KEY_FILE")
(cd "$CONTRACT_DIR" && "$FORGE" create LayerXMirrorArchive.sol:LayerXMirrorArchive \
    --rpc-url "$LAYERX_MIRROR_RPC_URL" --private-key "$deployer_key" --broadcast --json \
    --constructor-args "$LAYERX_MIRROR_PUBLISHER_ADDRESS") > "$work/create.json" 2> "$work/create.log" \
    || { cat "$work/create.log" >&2; fail "forge create LayerXMirrorArchive failed"; }

address=$(jq -r '.deployedTo' "$work/create.json")
transaction=$(jq -r '.transactionHash' "$work/create.json")
[[ $address =~ ^0x[0-9a-fA-F]{40}$ ]] || fail "forge reported no deployed address"
[[ $transaction =~ ^0x[0-9a-f]{64}$ ]] || fail "forge reported no deployment transaction"

runtime_code=$("$CAST" code "$address" --rpc-url "$LAYERX_MIRROR_RPC_URL")
[ "${#runtime_code}" -gt 2 ] || fail "the deployed contract carries no runtime code"
code_hash=$("$CAST" keccak "$runtime_code")
block_hash=$("$CAST" receipt "$transaction" blockHash --rpc-url "$LAYERX_MIRROR_RPC_URL")
genesis_hash=$("$CAST" block 0 hash --rpc-url "$LAYERX_MIRROR_RPC_URL")
publisher=$("$CAST" call "$address" "publisher()(address)" --rpc-url "$LAYERX_MIRROR_RPC_URL")
[ "$(printf '%s' "$publisher" | tr 'A-F' 'a-f')" = "$(printf '%s' "$LAYERX_MIRROR_PUBLISHER_ADDRESS" | tr 'A-F' 'a-f')" ] \
    || fail "the deployed contract publisher $publisher is not $LAYERX_MIRROR_PUBLISHER_ADDRESS"
[[ $block_hash =~ ^0x[0-9a-f]{64}$ ]] || fail "the deployment receipt carries no block hash"
[[ $genesis_hash =~ ^0x[0-9a-f]{64}$ ]] || fail "the chain reports no genesis block hash"

umask 077
jq -n --arg contract LayerXMirrorArchive \
    --arg source interop/contracts/ethereum-mirror/LayerXMirrorArchive.sol \
    --argjson chain_id "$LAYERX_MIRROR_CHAIN_ID" \
    --arg genesis_hash "$genesis_hash" \
    --arg contract_address "$address" \
    --arg publisher "$LAYERX_MIRROR_PUBLISHER_ADDRESS" \
    --arg code_hash "$code_hash" \
    --arg transaction "$transaction" \
    --arg block_hash "$block_hash" \
    '{contract: $contract, source: $source, chain_id: $chain_id, genesis_hash: $genesis_hash,
      contract_address: $contract_address, publisher_secp256k1_address: $publisher,
      runtime_code_keccak256: $code_hash, deployment_transaction: $transaction,
      deployment_block_hash: $block_hash}' > "$LAYERX_MIRROR_DEPLOYMENT_RECORD"

printf 'deploy-ethereum-mirror: LayerXMirrorArchive %s on chain %s (publisher %s)\n' \
    "$address" "$LAYERX_MIRROR_CHAIN_ID" "$LAYERX_MIRROR_PUBLISHER_ADDRESS" >&2
