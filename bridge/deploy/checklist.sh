#!/usr/bin/env bash
# Post-deploy checklist of one chain of the Paxeer X Network bridge. It reads the
# deployed state back through the chain's own RPC and the Paxeer side back
# through the layerxBridge precompile's views, and compares every value with the
# chain configuration and with the governance bodies bridge/deploy/proposals
# generates from it.
#
# Usage:
#   checklist.sh <chain>
#
# The chain name is the directory under bridge/evm/chains or, for solana, under
# bridge/solana/chains. Inputs, all through environment variables; there is no
# default endpoint, key or address anywhere in this script:
#   the variable the configuration names in environment.rpc_url   the chain's endpoint
#   PAXEER_BRIDGE_PAXEER_RPC_URL        the Paxeer X Network endpoint the precompile is read through
#   PAXEER_BRIDGE_DEPLOYMENT_RECORD     the record deploy-evm-chain.sh or deploy-solana-program.sh wrote
#   PAXEER_BRIDGE_GOVERNANCE_AUTHORITY  the bech32 governance authority the bodies are generated for
#   PAXEER_BRIDGE_ATTESTOR_MANIFEST     optional attestor-set manifest; bridge/deploy/attestors.json otherwise
#   PAXEER_BRIDGE_EVM_CHAINS_ROOT       optional chains root, as deploy-evm-chain.sh reads it
#   PAXEER_BRIDGE_SOLANA_CHAINS_ROOT    optional chains root, as deploy-solana-program.sh reads it
#
# On an EVM chain it reads the vault's owner, attestors, threshold, per-asset
# registration and caps, paused flag and deployed code hash; on Solana the
# program account, the config account's owner, attestors, threshold and paused
# flag, and every asset account. On Paxeer it reads getChain, getAttestors,
# getCap and isPaused at 0x0000000000000000000000000000000000001016. Sidiora on
# Solana is the pair (Solana, 0x21f7b20a555199fa73A238B1a91FD0f549068fEe, usid),
# which only the chain's upgrade handler registers; its registration is read back
# before its cap, and a missing registration means the Sidiora cap proposal is
# not executable yet.
#
# The run only reads: eth_chainId, eth_getCode and eth_call on an EVM chain and
# on Paxeer, getGenesisHash and getAccountInfo on Solana. Nothing is signed and
# nothing is sent. It prints one line per check naming the chain, the value,
# what was expected and what was found, checks the native coin first, and exits
# 1 naming the first mismatch when any value disagrees, a placeholder is still
# in place, the native coin is unregistered, uncapped or paused, or the Paxeer
# side carries no registration for the chain.
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd "$SCRIPT_DIR/../.." && pwd)
PRECOMPILE=0x0000000000000000000000000000000000001016
PAXEER_RPC_VARIABLE=PAXEER_BRIDGE_PAXEER_RPC_URL
PLACEHOLDER_PREFIX='PLACEHOLDER:'
SIDIORA_ASSET_ID=0x21f7b20a555199fa73a238b1a91fd0f549068fee
UPGRADEABLE_LOADER=BPFLoaderUpgradeab1e11111111111111111111111
# The account layouts bridge/solana/src/state.rs writes: magic, a big-endian
# layout version, then fixed-width fields.
CONFIG_MAGIC=5058425243464730
ASSET_MAGIC=5058425241535430
LAYOUT_VERSION=0001
CONFIG_BYTES=1367
ASSET_BYTES=89
CONFIG_ATTESTORS_OFFSET=87

fail() {
    printf 'checklist: error: %s\n' "$*" >&2
    exit 1
}

usage() {
    printf 'usage: checklist.sh <chain>\n' >&2
    exit 2
}

[ $# -eq 1 ] || usage
case $1 in
-*) usage ;;
esac
chain=$1
[[ $chain =~ ^[a-z][a-z0-9]*$ ]] || fail "$chain is not a chain name"

for tool in jq curl cast go base64 od; do
    command -v "$tool" > /dev/null 2>&1 || fail "$tool is required and is not on the PATH"
done

absolute() {
    case $1 in
    /*) printf '%s' "$1" ;;
    *) printf '%s/%s' "$PWD" "$1" ;;
    esac
}

lower() { printf '%s' "$1" | tr '[:upper:]' '[:lower:]'; }

if [ "$chain" = solana ]; then
    chains_root=${PAXEER_BRIDGE_SOLANA_CHAINS_ROOT:-$REPO_ROOT/bridge/solana/chains}
else
    chains_root=${PAXEER_BRIDGE_EVM_CHAINS_ROOT:-$REPO_ROOT/bridge/evm/chains}
fi
config=$(absolute "$chains_root/$chain/config.json")
[ -r "$config" ] || fail "$config is not readable; $chain is not a bridge chain"
jq -e . "$config" > /dev/null 2>&1 || fail "$config is not JSON"
manifest=$(absolute "${PAXEER_BRIDGE_ATTESTOR_MANIFEST:-$REPO_ROOT/bridge/deploy/attestors.json}")
[ -r "$manifest" ] || fail "$manifest is not readable"

kind=$(jq -r '.kind // empty' "$config")
case $kind in
evm | solana) ;;
*) fail "$config: kind: ${kind:-nothing} is neither evm nor solana" ;;
esac
chain_id=$(jq -r '.chain_id // empty' "$config")
[[ $chain_id =~ ^[1-9][0-9]*$ ]] || fail "$config: chain_id: ${chain_id:-nothing} is not a chain id"

CHECKS=0
FAILED=0
FIRST=""

check() {
    local value=$1 expected=$2 found=$3 status=ok
    CHECKS=$((CHECKS + 1))
    if [ "$expected" != "$found" ]; then
        status=FAIL
        FAILED=$((FAILED + 1))
        [ -n "$FIRST" ] || FIRST="$value: expected $expected, found $found"
    fi
    printf 'checklist: %s %s %s: expected %s, found %s\n' "$chain" "$status" "$value" "$expected" "$found"
}

finish() {
    if [ "$FAILED" -gt 0 ]; then
        printf 'checklist: %s fails %d of %d checks; first mismatch: %s\n' "$chain" "$FAILED" "$CHECKS" "$FIRST"
        exit 1
    fi
    printf 'checklist: %s passes all %d checks\n' "$chain" "$CHECKS"
    exit 0
}

stop_on_failure() { [ "$FAILED" -eq 0 ] || finish; }

# A placeholder is refused before anything is read: a configuration nobody has
# filled in describes no deployment.
placeholders=()
owner_value=$(jq -r '.owner // empty' "$config")
case $owner_value in "$PLACEHOLDER_PREFIX"*) placeholders+=("owner=$owner_value") ;; esac
mapfile -t configured_attestors < <(jq -r '.attestors[]?' "$config")
for index in "${!configured_attestors[@]}"; do
    case ${configured_attestors[index]} in
    "$PLACEHOLDER_PREFIX"*) placeholders+=("attestors[$index]=${configured_attestors[index]}") ;;
    esac
done
if [ "$kind" = solana ]; then
    program_value=$(jq -r '.solana.program_id // empty' "$config")
    case $program_value in "$PLACEHOLDER_PREFIX"*) placeholders+=("solana.program_id=$program_value") ;; esac
fi
if [ "${#placeholders[@]}" -eq 0 ]; then
    check "placeholders in $config" none none
else
    check "placeholders in $config" none "$(IFS=' '; printf '%s' "${placeholders[*]}")"
fi
stop_on_failure

rpc_variable=$(jq -r '.environment.rpc_url // empty' "$config")
[[ $rpc_variable =~ ^[A-Z][A-Z0-9]*(_[A-Z0-9]+)*$ ]] \
    || fail "$config: environment.rpc_url: ${rpc_variable:-nothing} is not an environment variable name"
for variable in "$rpc_variable" "$PAXEER_RPC_VARIABLE" PAXEER_BRIDGE_DEPLOYMENT_RECORD PAXEER_BRIDGE_GOVERNANCE_AUTHORITY; do
    [ -n "${!variable:-}" ] || fail "$variable is required and is not set"
done
record=$(absolute "$PAXEER_BRIDGE_DEPLOYMENT_RECORD")
[ -r "$record" ] || fail "$record is not readable"
jq -e . "$record" > /dev/null 2>&1 || fail "$record is not JSON"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
chmod 0700 "$work"

found=$(jq -r '.chain // "nothing"' "$record")
check "deployment record chain" "$chain" "$found"
found=$(jq -r '.chain_id // "nothing"' "$record")
check "deployment record chain id" "$chain_id" "$found"
if [ "$kind" = evm ]; then
    vault=$(jq -r '.vault // "nothing"' "$record")
    [[ $vault =~ ^0x[0-9a-fA-F]{40}$ ]] || check "deployment record vault" "a 20-byte address" "$vault"
else
    program_id=$(jq -r '.solana.program_id' "$config")
    found=$(jq -r '.program_id // "nothing"' "$record")
    check "deployment record program id" "$program_id" "$found"
    vault=$(jq -r '.vault_handle // "nothing"' "$record")
    [[ $vault =~ ^0x[0-9a-fA-F]{40}$ ]] || check "deployment record vault handle" "a 20-byte handle" "$vault"
fi
stop_on_failure

# The bodies governance submits and the values the chain must read back, from
# the generator itself, so the checklist and the proposals cannot disagree.
bodies="$work/bodies"
readback="$work/readback.json"
if (cd "$REPO_ROOT" && go run ./bridge/deploy/proposals/cmd/paxeer-bridge-proposals \
    -manifest "$manifest" -authority "$PAXEER_BRIDGE_GOVERNANCE_AUTHORITY" -vault "$vault" \
    -readback "$readback" "$config" "$bodies") > "$work/proposals.log" 2>&1; then
    cap_bodies=("$bodies"/03-set-cap-*.json)
    check "governance bodies" "$((2 + ${#cap_bodies[@]})) bodies" "$(find "$bodies" -name '*.json' | wc -l | tr -d ' ') bodies"
else
    check "governance bodies" "a bundle generated from $config" "$(tail -n 1 "$work/proposals.log")"
fi
stop_on_failure
register_body="$bodies/01-register-chain.json"
attestors_body="$bodies/02-set-attestors.json"
asset_count=$(jq '.assets | length' "$readback")

rpc() {
    local variable=$1 method=$2 params=$3 endpoint response
    endpoint=${!variable}
    jq -cn --arg method "$method" --argjson params "$params" \
        '{jsonrpc: "2.0", id: 1, method: $method, params: $params}' > "$work/request.json"
    response=$(printf 'url = "%s"\n' "$(printf '%s' "$endpoint" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g')" \
        | curl --silent --show-error --fail --max-time 30 --config - \
            --header 'Content-Type: application/json' --data-binary @"$work/request.json") \
        || fail "$variable did not answer $method"
    jq -e 'type == "object"' <<< "$response" > /dev/null 2>&1 || fail "$variable answered $method with something other than JSON-RPC"
    if jq -e 'has("error")' <<< "$response" > /dev/null; then
        fail "$variable answered $method with an error: $(jq -c '.error' <<< "$response")"
    fi
    jq -e 'has("result")' <<< "$response" > /dev/null || fail "$variable answered $method without a result"
    jq -c '.result' <<< "$response"
}

view() {
    local variable=$1 to=$2 signature=$3 data params
    shift 3
    data=$(cast calldata "$signature" "$@") || fail "$signature cannot be encoded"
    params=$(jq -cn --arg to "$(lower "$to")" --arg data "$data" '[{to: $to, data: $data}, "latest"]')
    rpc "$variable" eth_call "$params" | jq -r .
}

# decode prints one line per returned value: annotations dropped, arrays as a
# comma-separated list, strings unquoted.
decode() {
    cast abi-decode "f()($1)" "$2" \
        | sed -e 's/ \[[^]]*\]//g' -e 's/^\[//' -e 's/\]$//' -e 's/, */,/g' -e 's/^"\(.*\)"$/\1/'
}

read_view() {
    local types=$1 out
    shift
    out=$(view "$@") || exit 1
    decode "$types" "$out" || fail "$3 returned $out, which is not ($types)"
}

u64() { printf '%u' "$((16#$1))"; }

ACCOUNT_OWNER=""
ACCOUNT_EXECUTABLE=""
ACCOUNT_HEX=""
read_account() {
    local info params
    params=$(jq -cn --arg account "$1" --arg commitment "$commitment" \
        '[$account, {encoding: "base64", commitment: $commitment}]')
    info=$(rpc "$rpc_variable" getAccountInfo "$params") || exit 1
    ACCOUNT_OWNER=""
    ACCOUNT_EXECUTABLE=""
    ACCOUNT_HEX=""
    if [ "$(jq -r '.value == null' <<< "$info")" = true ]; then
        return 0
    fi
    ACCOUNT_OWNER=$(jq -r '.value.owner' <<< "$info")
    ACCOUNT_EXECUTABLE=$(jq -r '.value.executable' <<< "$info")
    ACCOUNT_HEX=$(jq -r '.value.data[0]' <<< "$info" | base64 -d | od -An -v -tx1 | tr -d ' \n') \
        || fail "the data of $1 is not base64"
}

asset_field() { jq -r --argjson index "$1" ".assets[\$index].$2" "$readback"; }

paxeer_paused() {
    local found
    found=$(read_view bool "$PAXEER_RPC_VARIABLE" "$PRECOMPILE" 'isPaused()')
    check "Paxeer bridge paused" false "$found"
}

# paxeer_cap reads one asset's registration and caps back from the precompile
# and compares them with the denom the chain maps the asset to and the cap body.
paxeer_cap() {
    local index=$1 body symbol asset denom result label
    local -a values
    body=${cap_bodies[index]}
    symbol=$(asset_field "$index" symbol)
    asset="0x$(jq -r '.asset' "$body")"
    denom=$(asset_field "$index" denom)
    result=$(read_view 'string,uint256,uint256,uint256' "$PAXEER_RPC_VARIABLE" "$PRECOMPILE" \
        'getCap(uint64,address)' "$chain_id" "$asset")
    mapfile -t values <<< "$result"
    if [ "$kind" = solana ] && [ "$asset" = "$SIDIORA_ASSET_ID" ]; then
        label="Sidiora registration of ($chain_id, $asset, usid) on Paxeer"
        if [ -z "${values[0]}" ]; then
            check "$label" "$denom" "none; only the chain's upgrade handler registers this pair, so the Sidiora cap proposal is not executable until it has"
            return 0
        fi
        check "$label" "$denom" "${values[0]}"
    elif [ -z "${values[0]}" ]; then
        check "$symbol registration on Paxeer" "$denom" none
        return 0
    else
        check "$symbol denom on Paxeer" "$denom" "${values[0]}"
    fi
    check "$symbol per-transaction cap on Paxeer" "$(jq -r '.max_per_tx' "$body")" "${values[2]}"
    check "$symbol total cap on Paxeer" "$(jq -r '.max_in_flight' "$body")" "${values[1]}"
}

paxeer_chain() {
    local result expected
    local -a values
    result=$(read_view 'bool,address,uint64,bool' "$PAXEER_RPC_VARIABLE" "$PRECOMPILE" 'getChain(uint64)' "$chain_id")
    mapfile -t values <<< "$result"
    if [ "${values[0]}" != true ]; then
        check "Paxeer registration of chain $chain_id" registered none
        return 0
    fi
    check "Paxeer registration of chain $chain_id" registered registered
    expected="0x$(jq -r '.chain.vault' "$register_body")"
    check "Paxeer vault of chain $chain_id" "$expected" "$(lower "${values[1]}")"
    check "Paxeer finality depth of chain $chain_id" "$(jq -r '.chain.finality_depth' "$register_body")" "${values[2]}"
    check "Paxeer chain $chain_id enabled" "$(jq -r '.chain.enabled' "$register_body")" "${values[3]}"
}

paxeer_attestors() {
    local result expected
    local -a values
    result=$(read_view 'address[],uint256[],uint32' "$PAXEER_RPC_VARIABLE" "$PRECOMPILE" 'getAttestors()')
    mapfile -t values <<< "$result"
    expected=$(jq -r '[.set.attestors[].signer | "0x" + .] | join(",")' "$attestors_body")
    check "Paxeer attestor set" "$expected" "$(lower "${values[0]}")"
    check "Paxeer threshold" "$(jq -r '.set.threshold' "$attestors_body")" "${values[2]}"
}

evm_asset() {
    local index=$1 symbol address found
    local -a values
    symbol=$(asset_field "$index" symbol)
    address=$(asset_field "$index" address)
    found=$(read_view bool "$rpc_variable" "$vault" 'registered(address)' "$address")
    check "$symbol registered on the vault" true "$found"
    found=$(read_view 'uint256,uint256' "$rpc_variable" "$vault" 'caps(address)' "$address")
    mapfile -t values <<< "$found"
    check "$symbol per-transaction cap on the vault" "$(asset_field "$index" per_tx_cap)" "${values[0]}"
    check "$symbol total cap on the vault" "$(asset_field "$index" total_cap)" "${values[1]:-none}"
}

evm_checks() {
    local found code index
    found=$(rpc "$rpc_variable" eth_chainId '[]' | jq -r .)
    found=$(cast to-dec "$found") || fail "$rpc_variable reported the chain id $found"
    check "chain id" "$chain_id" "$found"
    stop_on_failure

    evm_asset 0
    found=$(read_view bool "$rpc_variable" "$vault" 'paused()')
    check "vault paused" false "$found"
    paxeer_paused
    paxeer_cap 0

    code=$(rpc "$rpc_variable" eth_getCode "$(jq -cn --arg vault "$(lower "$vault")" '[$vault, "latest"]')" | jq -r .)
    if [ -z "$code" ] || [ "$code" = 0x ]; then
        found="no code"
    else
        found=$(cast keccak "$code")
    fi
    check "vault code hash" "$(jq -r '.code_hash // "nothing"' "$record")" "$found"
    found=$(read_view address "$rpc_variable" "$vault" 'owner()')
    check "vault owner" "$(jq -r '.owner' "$readback")" "$(lower "$found")"
    found=$(read_view 'address[]' "$rpc_variable" "$vault" 'attestors()')
    check "vault attestor set" "$(jq -r '.attestors | join(",")' "$readback")" "$(lower "$found")"
    found=$(read_view uint256 "$rpc_variable" "$vault" 'threshold()')
    check "vault threshold" "$(jq -r '.threshold' "$readback")" "$found"
    for ((index = 1; index < asset_count; index++)); do
        evm_asset "$index"
    done
}

solana_asset() {
    local index=$1 symbol account mint asset_id sidiora_mint found_mint found_id
    symbol=$(asset_field "$index" symbol)
    account=$(asset_field "$index" account)
    mint=$(asset_field "$index" mint)
    asset_id=$(asset_field "$index" asset_id)
    read_account "$account"
    if [ -z "$ACCOUNT_OWNER" ]; then
        check "$symbol asset account $account" registered none
        return 0
    fi
    check "$symbol asset account owner" "$program_id" "$ACCOUNT_OWNER"
    if [ "$((${#ACCOUNT_HEX} / 2))" -ne "$ASSET_BYTES" ] || [ "${ACCOUNT_HEX:0:16}" != "$ASSET_MAGIC" ] \
        || [ "${ACCOUNT_HEX:16:4}" != "$LAYOUT_VERSION" ]; then
        check "$symbol asset account layout" "$ASSET_BYTES bytes of PXBRAST0 version 1" \
            "$((${#ACCOUNT_HEX} / 2)) bytes starting ${ACCOUNT_HEX:0:20}"
        return 0
    fi
    found_mint="0x${ACCOUNT_HEX:20:64}"
    found_id="0x${ACCOUNT_HEX:84:40}"
    sidiora_mint=$(jq -r --arg id "$SIDIORA_ASSET_ID" '[.assets[] | select(.asset_id == $id) | .mint] | first // empty' "$readback")
    if [ "$found_id" = "$SIDIORA_ASSET_ID" ] && [ "$found_mint" != "$sidiora_mint" ]; then
        check "Sidiora's asset id $SIDIORA_ASSET_ID in $symbol's asset account" "the Sidiora mint $sidiora_mint" "the mint $found_mint"
    fi
    if [ "$found_mint" = "$sidiora_mint" ] && [ "$found_id" != "$SIDIORA_ASSET_ID" ]; then
        check "Sidiora's mint in $symbol's asset account" "the asset id $SIDIORA_ASSET_ID" "the asset id $found_id"
    fi
    check "$symbol mint in its asset account" "$mint" "$found_mint"
    check "$symbol asset id" "$asset_id" "$found_id"
    check "$symbol decimals" "$(asset_field "$index" decimals)" "$((16#${ACCOUNT_HEX:124:2}))"
    check "$symbol enabled" 01 "${ACCOUNT_HEX:126:2}"
    check "$symbol per-transaction cap on the program" "$(asset_field "$index" per_tx_cap)" "$(u64 "${ACCOUNT_HEX:130:16}")"
    check "$symbol total cap on the program" "$(asset_field "$index" total_cap)" "$(u64 "${ACCOUNT_HEX:146:16}")"
}

solana_checks() {
    local found config_account config_hex count attestors index start
    commitment=$(jq -r '.solana.commitment' "$readback")
    found=$(rpc "$rpc_variable" getGenesisHash '[]' | jq -r .)
    check "genesis hash" "$(jq -r '.genesis_hash // "nothing"' "$record")" "$found"
    stop_on_failure

    read_account "$program_id"
    if [ -z "$ACCOUNT_OWNER" ]; then
        found=none
    elif [ "$ACCOUNT_EXECUTABLE" = true ]; then
        found="executable under $ACCOUNT_OWNER"
    else
        found="not executable under $ACCOUNT_OWNER"
    fi
    check "program account $program_id" "executable under $UPGRADEABLE_LOADER" "$found"
    config_account=$(jq -r '.solana.config_account' "$readback")
    read_account "$config_account"
    config_hex=$ACCOUNT_HEX
    if [ -z "$ACCOUNT_OWNER" ]; then
        check "config account $config_account" initialised none
    elif [ "$ACCOUNT_OWNER" != "$program_id" ] || [ "$((${#config_hex} / 2))" -ne "$CONFIG_BYTES" ] \
        || [ "${config_hex:0:16}" != "$CONFIG_MAGIC" ] || [ "${config_hex:16:4}" != "$LAYOUT_VERSION" ]; then
        check "config account $config_account" "$CONFIG_BYTES bytes of PXBRCFG0 version 1 owned by $program_id" \
            "$((${#config_hex} / 2)) bytes starting ${config_hex:0:20} owned by $ACCOUNT_OWNER"
    fi
    stop_on_failure

    solana_asset 0
    check "program paused" 00 "${config_hex:148:2}"
    paxeer_paused
    paxeer_cap 0

    check "program owner" "$(jq -r '.owner' "$readback")" "0x${config_hex:20:64}"
    count=$((16#${config_hex:150:2}))
    attestors=""
    for ((index = 0; index < count && index < 64; index++)); do
        start=$((2 * CONFIG_ATTESTORS_OFFSET + 40 * index))
        attestors+="${attestors:+,}0x${config_hex:start:40}"
    done
    check "program attestor set" "$(jq -r '.attestors | join(",")' "$readback")" "$attestors"
    check "program threshold" "$(jq -r '.threshold' "$readback")" "$((16#${config_hex:152:2}))"
    for ((index = 1; index < asset_count; index++)); do
        solana_asset "$index"
    done
}

if [ "$kind" = evm ]; then
    evm_checks
else
    solana_checks
fi
paxeer_chain
paxeer_attestors
for ((index = 1; index < asset_count; index++)); do
    paxeer_cap "$index"
done
finish
