#!/usr/bin/env bash
# Offline check of the three deployment scripts of the Paxeer X Network bridge:
# bridge/deploy/deploy-evm-chain.sh, bridge/deploy/verify-evm-chain.sh and
# bridge/deploy/deploy-solana-program.sh.
#
# Every committed chain configuration is refused while it carries its
# placeholder owner and attestor set, a configuration whose placeholders are
# filled in with values generated for this run is accepted by --preflight, and
# every argument, placeholder, missing variable, unreadable file and inconsistent
# deployment record is refused with a message that names the field or the
# variable at fault.
#
# No endpoint, explorer or cluster is reached: every run here stops at
# --preflight, the endpoint variables carry the reserved .invalid domain, and the
# keys and identities are generated into a private temporary directory and thrown
# away with it. The Solana run is driven as far as the pinned toolchain, which
# this check deliberately points at an empty directory: that refusal is the proof
# that every configuration and environment check before it passed. A deployment
# against a real cluster is the Solana dry run's job, not this check's.
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
DEPLOY_DIR=$(cd "$SCRIPT_DIR/.." && pwd)
REPO_ROOT=$(cd "$DEPLOY_DIR/../.." && pwd)
DEPLOY_EVM="$DEPLOY_DIR/deploy-evm-chain.sh"
VERIFY_EVM="$DEPLOY_DIR/verify-evm-chain.sh"
DEPLOY_SOLANA="$DEPLOY_DIR/deploy-solana-program.sh"
EVM_CHAINS="$REPO_ROOT/bridge/evm/chains"
SOLANA_CHAINS="$REPO_ROOT/bridge/solana/chains"
EVM_CHAIN_NAMES=(ethereum base arbitrum optimism bnb polygon avalanche hyperevm)
ENDPOINT=https://endpoint.invalid
ZERO_ADDRESS=0x0000000000000000000000000000000000000000
SID_MINT=5w3wVdJaESaJKyLmStM6Hv9UyUkmZ1b9DLQquAqqpump

fail() {
    printf 'deploy-scripts-check: error: %s\n' "$*" >&2
    exit 1
}

for tool in jq cast openssl python3; do
    command -v "$tool" > /dev/null 2>&1 || fail "$tool is required and is not on the PATH"
done
for script in "$DEPLOY_EVM" "$VERIFY_EVM" "$DEPLOY_SOLANA"; do
    [ -r "$script" ] || fail "$script is missing"
done

# Nothing an operator happens to have exported may reach the scripts under check:
# the missing-variable refusals below are only refusals in a clean environment.
unset "${!PAXEER_BRIDGE_@}"

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
chmod 0700 "$WORK"
mkdir -p "$WORK/records" "$WORK/toolchain"

STATUS=0
attempt() {
    STATUS=0
    "$@" > "$WORK/last.log" 2>&1 || STATUS=$?
}

quote() { sed -e 's/^/    /' "$WORK/last.log" >&2; }

refuses() {
    local needle=$1 label=$2
    shift 2
    attempt "$@"
    if [ "$STATUS" -eq 0 ]; then
        quote
        fail "$label was accepted"
    fi
    if [ "$STATUS" -eq 2 ]; then
        quote
        fail "$label answered with its usage instead of refusing"
    fi
    grep -qF -- "$needle" "$WORK/last.log" || {
        quote
        fail "$label was refused without naming: $needle"
    }
}

accepts() {
    local needle=$1 label=$2
    shift 2
    attempt "$@"
    if [ "$STATUS" -ne 0 ]; then
        quote
        fail "$label was refused (exit $STATUS)"
    fi
    grep -qF -- "$needle" "$WORK/last.log" || {
        quote
        fail "$label was accepted without reporting: $needle"
    }
}

answers_usage() {
    local label=$1
    shift
    attempt "$@"
    [ "$STATUS" -eq 2 ] || {
        quote
        fail "$label did not answer with its usage (exit $STATUS)"
    }
}

base58() {
    python3 - "$1" << 'PY'
import sys

ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
raw = open(sys.argv[1], "rb").read()
number = int.from_bytes(raw, "big")
text = ""
while number:
    number, remainder = divmod(number, 58)
    text = ALPHABET[remainder] + text
print("1" * (len(raw) - len(raw.lstrip(b"\x00"))) + text)
PY
}

# A real Ed25519 keypair in the file layout the Solana CLI reads, so the key file
# the scripts are handed is the kind of file an operator hands them. It is
# generated here, used by nothing but the readability checks and removed with the
# temporary directory.
ed25519_keypair() {
    local name=$1
    openssl genpkey -algorithm ed25519 -out "$WORK/$name.pem" 2> /dev/null
    openssl pkey -in "$WORK/$name.pem" -outform DER 2> /dev/null | tail -c 32 > "$WORK/$name.seed"
    openssl pkey -in "$WORK/$name.pem" -pubout -outform DER 2> /dev/null | tail -c 32 > "$WORK/$name.pub"
    python3 - "$WORK/$name.seed" "$WORK/$name.pub" > "$WORK/$name.json" << 'PY'
import json
import sys

raw = open(sys.argv[1], "rb").read() + open(sys.argv[2], "rb").read()
print(json.dumps(list(raw)))
PY
    chmod 0600 "$WORK/$name.pem" "$WORK/$name.seed" "$WORK/$name.json"
    base58 "$WORK/$name.pub"
}

secp256k1_address() { cast wallet address --private-key "0x$(openssl rand -hex 32)"; }

mapfile -t ATTESTOR_SET < <(
    for _ in 1 2 3 4 5; do secp256k1_address; done | tr '[:upper:]' '[:lower:]' | sort -u
)
[ "${#ATTESTOR_SET[@]}" -eq 5 ] || fail "five distinct attestor addresses could not be generated"
ATTESTORS=$(printf '%s\n' "${ATTESTOR_SET[@]}" | jq -R . | jq -sc .)
OWNER=$(secp256k1_address)
VAULT=$(secp256k1_address)
DEPLOYER=$(secp256k1_address)
USDC=$(secp256k1_address)
PUBLISHER=$(ed25519_keypair publisher)
PROGRAM=$(ed25519_keypair program)
[ -n "$PUBLISHER" ] || fail "the publisher identity could not be encoded"
[ -n "$PROGRAM" ] || fail "the program identity could not be encoded"
# The admin client of the Solana program is only checked for executability
# before a cluster is reached, so an executable this check already depends on
# stands in for it; it is never invoked, and the real client is exercised by the
# Solana dry run.
ADMIN=$(command -v jq)

# A run-local copy of a committed configuration with the placeholders filled in
# and one further edit applied, under <work>/<case>/<chain>/config.json.
evm_configuration() {
    local name=$1 chain=$2 edit=${3:-.} root="$WORK/$1"
    mkdir -p "$root/$chain"
    jq --arg owner "$OWNER" --argjson attestors "$ATTESTORS" \
        ".owner = \$owner | .attestors = \$attestors | $edit" \
        "$EVM_CHAINS/$chain/config.json" > "$root/$chain/config.json"
    printf '%s' "$root"
}

solana_configuration() {
    local name=$1 edit=${2:-.} root="$WORK/$1"
    mkdir -p "$root/solana"
    jq --arg owner "$PUBLISHER" --argjson attestors "$ATTESTORS" \
        ".owner = \$owner | .attestors = \$attestors | $edit" \
        "$SOLANA_CHAINS/solana/config.json" > "$root/solana/config.json"
    printf '%s' "$root"
}

deployment_record() {
    local name=$1 edit=${2:-.} path="$WORK/records/$1.json"
    jq -n --arg vault "$VAULT" --arg deployer "$DEPLOYER" --argjson attestors "$ATTESTORS" \
        "{chain: \"ethereum\", chain_id: 1, kind: \"evm\", vault: \$vault, deployer: \$deployer,
          threshold: 3, attestors: \$attestors} | $edit" > "$path"
    printf '%s' "$path"
}

variable_of() { jq -r --arg field "$2" '.environment[$field]' "$1/config.json"; }

# deploy-evm-chain.sh: arguments.
answers_usage 'deploy-evm-chain.sh with no chain' bash "$DEPLOY_EVM"
answers_usage 'deploy-evm-chain.sh with two chains' bash "$DEPLOY_EVM" ethereum base
answers_usage 'deploy-evm-chain.sh with an unknown option' bash "$DEPLOY_EVM" --broadcast ethereum
answers_usage 'deploy-evm-chain.sh --preflight with no chain' bash "$DEPLOY_EVM" --preflight
refuses 'is not a chain name' 'deploy-evm-chain.sh with a path for a chain' \
    bash "$DEPLOY_EVM" --preflight ../../etc
refuses 'is not a bridge EVM chain' 'deploy-evm-chain.sh with a chain the bridge does not carry' \
    bash "$DEPLOY_EVM" --preflight sepolia

# deploy-evm-chain.sh: every committed configuration refuses its own
# placeholders, and the same configuration with them filled in is ready to
# deploy. Every endpoint and key here arrives through the variable the
# configuration itself names.
for chain in "${EVM_CHAIN_NAMES[@]}"; do
    rpc_variable=$(variable_of "$EVM_CHAINS/$chain" rpc_url)
    key_variable=$(variable_of "$EVM_CHAINS/$chain" deploy_key)
    environment=(
        "$rpc_variable=$ENDPOINT"
        "$key_variable=0x$(openssl rand -hex 32)"
        "PAXEER_BRIDGE_DEPLOYMENT_RECORD=$WORK/records/$chain.json"
    )
    refuses 'owner: PLACEHOLDER:owner is a placeholder' "the committed $chain configuration" \
        env "PAXEER_BRIDGE_EVM_CHAINS_ROOT=$EVM_CHAINS" "${environment[@]}" \
        bash "$DEPLOY_EVM" --preflight "$chain"
    if [ "$chain" = hyperevm ]; then
        root=$(evm_configuration "filled-$chain" "$chain")
        refuses 'big_blocks.acknowledged' \
            "a filled $chain configuration whose big blocks are not acknowledged" \
            env "PAXEER_BRIDGE_EVM_CHAINS_ROOT=$root" "${environment[@]}" \
            bash "$DEPLOY_EVM" --preflight "$chain"
        root=$(evm_configuration "acknowledged-$chain" "$chain" '.big_blocks.acknowledged = true')
    else
        root=$(evm_configuration "filled-$chain" "$chain")
    fi
    accepts 'is ready to deploy' "a filled $chain configuration" \
        env "PAXEER_BRIDGE_EVM_CHAINS_ROOT=$root" "${environment[@]}" \
        bash "$DEPLOY_EVM" --preflight "$chain"
done

# deploy-evm-chain.sh: the environment it names.
ROOT=$(evm_configuration ethereum-filled ethereum)
EVM_ENVIRONMENT=(
    "PAXEER_BRIDGE_EVM_CHAINS_ROOT=$ROOT"
    "PAXEER_BRIDGE_ETHEREUM_RPC_URL=$ENDPOINT"
    "PAXEER_BRIDGE_ETHEREUM_DEPLOY_KEY=0x$(openssl rand -hex 32)"
    "PAXEER_BRIDGE_DEPLOYMENT_RECORD=$WORK/records/ethereum.json"
)
refuses 'PAXEER_BRIDGE_ETHEREUM_RPC_URL is required and is not set' \
    'a deployment with no endpoint' \
    env "PAXEER_BRIDGE_EVM_CHAINS_ROOT=$ROOT" \
    "PAXEER_BRIDGE_ETHEREUM_DEPLOY_KEY=0x$(openssl rand -hex 32)" \
    "PAXEER_BRIDGE_DEPLOYMENT_RECORD=$WORK/records/ethereum.json" \
    bash "$DEPLOY_EVM" --preflight ethereum
refuses 'PAXEER_BRIDGE_ETHEREUM_DEPLOY_KEY is required and is not set' \
    'a deployment with no deployer key' \
    env "PAXEER_BRIDGE_EVM_CHAINS_ROOT=$ROOT" "PAXEER_BRIDGE_ETHEREUM_RPC_URL=$ENDPOINT" \
    "PAXEER_BRIDGE_DEPLOYMENT_RECORD=$WORK/records/ethereum.json" \
    bash "$DEPLOY_EVM" --preflight ethereum
refuses 'PAXEER_BRIDGE_DEPLOYMENT_RECORD is required and is not set' \
    'a deployment that records nothing' \
    env "PAXEER_BRIDGE_EVM_CHAINS_ROOT=$ROOT" "PAXEER_BRIDGE_ETHEREUM_RPC_URL=$ENDPOINT" \
    "PAXEER_BRIDGE_ETHEREUM_DEPLOY_KEY=0x$(openssl rand -hex 32)" \
    bash "$DEPLOY_EVM" --preflight ethereum
refuses "$WORK/absent does not exist" 'a deployment record in a directory that does not exist' \
    env "${EVM_ENVIRONMENT[@]}" "PAXEER_BRIDGE_DEPLOYMENT_RECORD=$WORK/absent/ethereum.json" \
    bash "$DEPLOY_EVM" --preflight ethereum

# deploy-evm-chain.sh: the configuration it is handed.
evm_refuses() {
    local needle=$1 label=$2 edit=$3 name=$4 root
    root=$(evm_configuration "$name" ethereum "$edit")
    refuses "$needle" "$label" \
        env "${EVM_ENVIRONMENT[@]}" "PAXEER_BRIDGE_EVM_CHAINS_ROOT=$root" \
        bash "$DEPLOY_EVM" --preflight ethereum
}

AS_BASE_ROOT=$(evm_configuration ethereum-as-base ethereum)
mv "$AS_BASE_ROOT/ethereum" "$AS_BASE_ROOT/base"
refuses 'chain: the file lies in base but names ethereum' \
    'a configuration under the directory of another chain' \
    env "${EVM_ENVIRONMENT[@]}" "PAXEER_BRIDGE_EVM_CHAINS_ROOT=$AS_BASE_ROOT" \
    bash "$DEPLOY_EVM" --preflight base

evm_refuses 'kind: solana is not an EVM chain' 'an EVM chain that declares another kind' \
    '.kind = "solana"' ethereum-wrong-kind
evm_refuses 'chain_id: 0 is not a chain id' 'a chain id of zero' \
    '.chain_id = 0' ethereum-zero-chain-id
evm_refuses 'threshold: 0 is not a threshold above zero' 'a threshold of zero' \
    '.threshold = 0' ethereum-zero-threshold
evm_refuses 'threshold: 6 is above the 5 attestors' 'a threshold above the attestor set' \
    '.threshold = 6' ethereum-threshold-above-set
evm_refuses 'does not follow the attestor before it' 'a descending attestor set' \
    '.attestors = (.attestors | reverse)' ethereum-descending-attestors
evm_refuses 'does not follow the attestor before it' 'an attestor set with a duplicate' \
    '.attestors[1] = .attestors[0]' ethereum-duplicate-attestor
evm_refuses "attestors[0]: the zero address is not a value" 'the zero address as an attestor' \
    ".attestors[0] = \"$ZERO_ADDRESS\"" ethereum-zero-attestor
evm_refuses 'attestors[2]: PLACEHOLDER:attestor-3 is a placeholder' \
    'one placeholder left in a filled attestor set' \
    '.attestors[2] = "PLACEHOLDER:attestor-3"' ethereum-one-placeholder-attestor
evm_refuses 'owner: the zero address is not a value' 'the zero address as the owner' \
    ".owner = \"$ZERO_ADDRESS\"" ethereum-zero-owner
evm_refuses 'assets[0].address' 'an asset list that does not open with the native coin' \
    ".assets[0].address = \"$USDC\" | .assets[0].asset_id = \"$USDC\"" ethereum-no-native-asset
evm_refuses 'assets[0].asset_id' 'an asset whose id is not its address' \
    '.assets[0].asset_id = "0x0000000000000000000000000000000000000001"' ethereum-foreign-asset-id
evm_refuses 'assets[0].per_tx_cap: 0 is not a cap above zero' 'a per-transaction cap of zero' \
    '.assets[0].per_tx_cap = "0"' ethereum-zero-per-tx-cap
evm_refuses 'assets[0].total_cap: 0 is not a cap above zero' 'a total cap of zero' \
    '.assets[0].total_cap = "0"' ethereum-zero-total-cap
evm_refuses 'environment.rpc_url: paxeer_rpc is not an upper snake case environment variable name' \
    'an endpoint variable that is not a variable name' \
    '.environment.rpc_url = "paxeer_rpc"' ethereum-lowercase-variable

ROOT=$(evm_configuration ethereum-two-assets ethereum \
    ".assets += [{symbol: \"USDC\", address: \"$USDC\", asset_id: \"$USDC\", decimals: 6,
                  per_tx_cap: \"250000000000\", total_cap: \"5000000000000\"}]")
accepts '2 assets' 'a filled configuration that carries a token beside the native coin' \
    env "${EVM_ENVIRONMENT[@]}" "PAXEER_BRIDGE_EVM_CHAINS_ROOT=$ROOT" \
    bash "$DEPLOY_EVM" --preflight ethereum

# verify-evm-chain.sh: arguments, the explorer key and the deployment record.
answers_usage 'verify-evm-chain.sh with no chain' bash "$VERIFY_EVM"
answers_usage 'verify-evm-chain.sh with two chains' bash "$VERIFY_EVM" ethereum base
answers_usage 'verify-evm-chain.sh with an unknown option' bash "$VERIFY_EVM" --watch ethereum
refuses 'is not a bridge EVM chain' 'verify-evm-chain.sh with a chain the bridge does not carry' \
    bash "$VERIFY_EVM" --preflight sepolia
refuses 'PAXEER_BRIDGE_ETHEREUM_EXPLORER_KEY is required and is not set' \
    'a verification with no explorer key' \
    env "PAXEER_BRIDGE_EVM_CHAINS_ROOT=$EVM_CHAINS" bash "$VERIFY_EVM" --preflight ethereum

RECORD=$(deployment_record ethereum)
VERIFY_ENVIRONMENT=(
    "PAXEER_BRIDGE_EVM_CHAINS_ROOT=$EVM_CHAINS"
    "PAXEER_BRIDGE_ETHEREUM_EXPLORER_KEY=$(openssl rand -hex 16)"
)
refuses 'PAXEER_BRIDGE_DEPLOYMENT_RECORD is required and is not set' \
    'a verification with no deployment record' \
    env "${VERIFY_ENVIRONMENT[@]}" bash "$VERIFY_EVM" --preflight ethereum
refuses 'deploy the chain before verifying its source' \
    'a verification whose deployment record is not there' \
    env "${VERIFY_ENVIRONMENT[@]}" "PAXEER_BRIDGE_DEPLOYMENT_RECORD=$WORK/records/never.json" \
    bash "$VERIFY_EVM" --preflight ethereum

verify_refuses() {
    local needle=$1 label=$2 edit=$3 name=$4 record
    record=$(deployment_record "$name" "$edit")
    refuses "$needle" "$label" \
        env "${VERIFY_ENVIRONMENT[@]}" "PAXEER_BRIDGE_DEPLOYMENT_RECORD=$record" \
        bash "$VERIFY_EVM" --preflight ethereum
}

verify_refuses 'records base, not ethereum' 'a record of another chain' \
    '.chain = "base"' record-of-base
verify_refuses 'records chain 8453' 'a record of another chain id' \
    '.chain_id = 8453' record-of-another-chain-id
verify_refuses 'records no vault address' 'a record with no vault' \
    'del(.vault)' record-without-vault
verify_refuses 'records no deployer' 'a record with no deployer' \
    'del(.deployer)' record-without-deployer
verify_refuses 'records no threshold' 'a record with no threshold' \
    '.threshold = 0' record-without-threshold
verify_refuses 'records the placeholder attestor PLACEHOLDER:attestor-1' \
    'a record of the placeholder attestor set' \
    '.attestors[0] = "PLACEHOLDER:attestor-1"' record-with-placeholder-attestor
verify_refuses 'records no attestor set' 'a record with an empty attestor set' \
    '.attestors = []' record-without-attestors
accepts "vault $VAULT is ready to submit" 'a verification of a recorded deployment' \
    env "${VERIFY_ENVIRONMENT[@]}" "PAXEER_BRIDGE_DEPLOYMENT_RECORD=$RECORD" \
    bash "$VERIFY_EVM" --preflight ethereum

# deploy-solana-program.sh: arguments and the committed configuration.
answers_usage 'deploy-solana-program.sh with a chain argument' bash "$DEPLOY_SOLANA" solana
answers_usage 'deploy-solana-program.sh with an unknown option' bash "$DEPLOY_SOLANA" --broadcast
SOLANA_ENVIRONMENT=(
    "PAXEER_BRIDGE_SOLANA_RPC_URL=$ENDPOINT"
    "PAXEER_BRIDGE_SOLANA_KEYPAIR_FILE=$WORK/publisher.json"
    "PAXEER_BRIDGE_SOLANA_TOOLCHAIN_BIN=$WORK/toolchain"
    "PAXEER_BRIDGE_DEPLOYMENT_RECORD=$WORK/records/solana.json"
    "PAXEER_BRIDGE_SOLANA_ADMIN_CLI=$ADMIN"
)
refuses 'owner: PLACEHOLDER:owner is a placeholder' 'the committed Solana configuration' \
    env "PAXEER_BRIDGE_SOLANA_CHAINS_ROOT=$SOLANA_CHAINS" "${SOLANA_ENVIRONMENT[@]}" \
    bash "$DEPLOY_SOLANA" --preflight

solana_refuses() {
    local needle=$1 label=$2 edit=$3 name=$4 root
    root=$(solana_configuration "$name" "$edit")
    refuses "$needle" "$label" \
        env "PAXEER_BRIDGE_SOLANA_CHAINS_ROOT=$root" "${SOLANA_ENVIRONMENT[@]}" \
        bash "$DEPLOY_SOLANA" --preflight
}

solana_refuses 'kind: evm is not the Solana chain' 'a Solana configuration of another kind' \
    '.kind = "evm"' solana-wrong-kind
solana_refuses 'chain_id: Solana is chain 91600046870081' 'another chain id for Solana' \
    '.chain_id = 999' solana-wrong-chain-id
solana_refuses 'solana.commitment: processed' 'a commitment that can still be dropped' \
    '.solana.commitment = "processed"' solana-processed-commitment
solana_refuses 'finality_depth: 0 is not a slot depth above zero' 'a finality depth of zero' \
    '.finality_depth = 0' solana-zero-finality-depth
solana_refuses 'owner: PLACEHOLDER:owner is a placeholder' 'a placeholder owner' \
    '.owner = "PLACEHOLDER:owner"' solana-placeholder-owner
solana_refuses 'does not follow the attestor before it' 'a descending attestor set' \
    '.attestors = (.attestors | reverse)' solana-descending-attestors
solana_refuses 'attestors[3]: PLACEHOLDER:attestor-4 is a placeholder' \
    'one placeholder left in a filled attestor set' \
    '.attestors[3] = "PLACEHOLDER:attestor-4"' solana-one-placeholder-attestor
solana_refuses 'assets[0].address: the first asset of Solana is the wrapped SOL mint' \
    'an asset list that does not open with wrapped SOL' \
    ".assets[0].address = \"$SID_MINT\"" solana-no-wrapped-sol
solana_refuses 'assets[1].asset_id' 'an asset id that is not 20 bytes' \
    '.assets[1].asset_id = "0x12"' solana-short-asset-id
solana_refuses 'assets[1].decimals: 0 is not a decimal count between 1 and 18' \
    'an asset with no decimals' '.assets[1].decimals = 0' solana-zero-decimals
solana_refuses 'assets[1].decimals: 19 is not a decimal count between 1 and 18' \
    'an asset with more decimals than a denom carries' \
    '.assets[1].decimals = 19' solana-nineteen-decimals
solana_refuses 'assets[1].total_cap: 0 is not a cap above zero' 'a total cap of zero' \
    '.assets[1].total_cap = "0"' solana-zero-total-cap

# deploy-solana-program.sh: the environment it names.
SOLANA_ROOT=$(solana_configuration solana-filled)
solana_without() {
    local dropped=$1 needle=$2 label=$3 environment=()
    for setting in "${SOLANA_ENVIRONMENT[@]}"; do
        case $setting in
        "$dropped="*) ;;
        *) environment+=("$setting") ;;
        esac
    done
    [ "${#environment[@]}" -eq $((${#SOLANA_ENVIRONMENT[@]} - 1)) ] \
        || fail "$dropped is not one of the variables this check sets"
    refuses "$needle" "$label" \
        env "PAXEER_BRIDGE_SOLANA_CHAINS_ROOT=$SOLANA_ROOT" "${environment[@]}" \
        bash "$DEPLOY_SOLANA" --preflight
}

solana_without PAXEER_BRIDGE_SOLANA_RPC_URL \
    'PAXEER_BRIDGE_SOLANA_RPC_URL is required and is not set' 'a deployment with no endpoint'
solana_without PAXEER_BRIDGE_SOLANA_KEYPAIR_FILE \
    'PAXEER_BRIDGE_SOLANA_KEYPAIR_FILE is required and is not set' \
    'a deployment with no publisher keypair'
solana_without PAXEER_BRIDGE_SOLANA_TOOLCHAIN_BIN \
    'PAXEER_BRIDGE_SOLANA_TOOLCHAIN_BIN is required and is not set' \
    'a deployment with no pinned toolchain'
solana_without PAXEER_BRIDGE_DEPLOYMENT_RECORD \
    'PAXEER_BRIDGE_DEPLOYMENT_RECORD is required and is not set' \
    'a deployment that records nothing'
solana_without PAXEER_BRIDGE_SOLANA_ADMIN_CLI \
    'PAXEER_BRIDGE_SOLANA_ADMIN_CLI is required and is not set' \
    'a deployment with no admin client'

solana_with() {
    local needle=$1 label=$2
    shift 2
    refuses "$needle" "$label" \
        env "PAXEER_BRIDGE_SOLANA_CHAINS_ROOT=$SOLANA_ROOT" "${SOLANA_ENVIRONMENT[@]}" "$@" \
        bash "$DEPLOY_SOLANA" --preflight
}

solana_with 'must carry an http or https endpoint' 'an endpoint that is not a URL' \
    "PAXEER_BRIDGE_SOLANA_RPC_URL=$WORK/publisher.json"
solana_with "$WORK/never.json, which is not readable" 'a publisher keypair that is not there' \
    "PAXEER_BRIDGE_SOLANA_KEYPAIR_FILE=$WORK/never.json"
solana_with "$WORK/absent does not exist" 'a deployment record in a directory that is not there' \
    "PAXEER_BRIDGE_DEPLOYMENT_RECORD=$WORK/absent/solana.json"
solana_with 'it is the bridge/solana admin client' 'an admin client that is not executable' \
    "PAXEER_BRIDGE_SOLANA_ADMIN_CLI=$WORK/publisher.json"
solana_with 'is not a directory' 'a pinned toolchain that is not a directory' \
    "PAXEER_BRIDGE_SOLANA_TOOLCHAIN_BIN=$WORK/publisher.json"

PROGRAM_ROOT=$(solana_configuration solana-program-id ".solana.program_id = \"$PROGRAM\"")
refuses 'PAXEER_BRIDGE_SOLANA_PROGRAM_KEYPAIR_FILE must name the keypair of that program id' \
    'a configuration that names a program id with no keypair to reproduce it' \
    env "PAXEER_BRIDGE_SOLANA_CHAINS_ROOT=$PROGRAM_ROOT" "${SOLANA_ENVIRONMENT[@]}" \
    bash "$DEPLOY_SOLANA" --preflight
refuses 'PAXEER_BRIDGE_SOLANA_PROGRAM_KEYPAIR_FILE is not readable' \
    'a program keypair that is not there' \
    env "PAXEER_BRIDGE_SOLANA_CHAINS_ROOT=$PROGRAM_ROOT" "${SOLANA_ENVIRONMENT[@]}" \
    "PAXEER_BRIDGE_SOLANA_PROGRAM_KEYPAIR_FILE=$WORK/never.json" \
    bash "$DEPLOY_SOLANA" --preflight

# Every configuration and environment check of the Solana run passes here, so
# the run reaches the pinned toolchain and refuses the empty directory this
# check points it at. The toolchain itself is the Solana dry run's ground.
refuses "$WORK/toolchain/solana is not executable" \
    'a Solana deployment whose pinned toolchain is empty' \
    env "PAXEER_BRIDGE_SOLANA_CHAINS_ROOT=$PROGRAM_ROOT" "${SOLANA_ENVIRONMENT[@]}" \
    "PAXEER_BRIDGE_SOLANA_PROGRAM_KEYPAIR_FILE=$WORK/program.json" \
    bash "$DEPLOY_SOLANA" --preflight
refuses 'must hold the pinned Solana toolchain' \
    'a Solana deployment whose pinned toolchain is empty' \
    env "PAXEER_BRIDGE_SOLANA_CHAINS_ROOT=$SOLANA_ROOT" "${SOLANA_ENVIRONMENT[@]}" \
    bash "$DEPLOY_SOLANA" --preflight

printf 'deploy-scripts-check: the %d committed chain configurations are refused while they carry their placeholders, a filled one is ready to deploy, and every argument, placeholder, missing variable and inconsistent record is refused\n' \
    $((${#EVM_CHAIN_NAMES[@]} + 1)) >&2
