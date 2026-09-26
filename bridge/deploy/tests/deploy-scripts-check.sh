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
# No endpoint, explorer or cluster is reached: the endpoint variables carry the
# reserved .invalid domain, and the keys and identities are generated into a
# private temporary directory and thrown away with it. The Solana run is driven
# as far as the pinned toolchain, which this check points at an empty directory:
# that refusal is the proof that every configuration and environment check before
# it passed. The Solana first deployment is then driven past --preflight against
# the cluster recorded in fixtures/solana-deploy, whose toolchain answers only the
# invocations the script makes and only for a .invalid endpoint: the vault
# authority the script records must be the PDA of the seed the program declares
# as VAULT_SEED, pinned by the bridge/vectors Solana vector, a script that names
# any other seed is caught, and a placeholder solana.program_id stops the run
# before the initialise step. The platform tools release the Solana script
# declares must be no older than the first whose cargo accepts edition 2024, it
# must reach cargo-build-sbf as --tools-version, and every cargo build-sbf the
# bridge workflow runs must name the same release. A deployment against a real
# cluster is the Solana dry run's job, not this check's.
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
SOLANA_FIXTURES="$SCRIPT_DIR/fixtures/solana-deploy"
REPLAY_TOOLCHAIN="$SOLANA_FIXTURES/toolchain"
SOLANA_KEYS="$SOLANA_FIXTURES/solana_keys.py"
VAULT_FIXTURE="$SOLANA_FIXTURES/vault-authority.json"
PROGRAM_STATE="$REPO_ROOT/bridge/solana/src/state.rs"
SOLANA_VECTORS="$REPO_ROOT/bridge/vectors/solana.go"
WORKFLOW="$REPO_ROOT/.github/workflows/bridge-test.yml"
OLDEST_PLATFORM_TOOLS=v1.52

fail() {
    printf 'deploy-scripts-check: error: %s\n' "$*" >&2
    exit 1
}

for tool in jq cast openssl python3 sha256sum; do
    command -v "$tool" > /dev/null 2>&1 || fail "$tool is required and is not on the PATH"
done
for script in "$DEPLOY_EVM" "$VERIFY_EVM" "$DEPLOY_SOLANA"; do
    [ -r "$script" ] || fail "$script is missing"
done
for fixture in "$SOLANA_KEYS" "$VAULT_FIXTURE" "$SOLANA_FIXTURES/cluster.json" \
    "$PROGRAM_STATE" "$SOLANA_VECTORS" "$WORKFLOW"; do
    [ -r "$fixture" ] || fail "$fixture is missing"
done
for tool in solana solana-keygen cargo-build-sbf; do
    [ -x "$REPLAY_TOOLCHAIN/$tool" ] || fail "$REPLAY_TOOLCHAIN/$tool is missing or not executable"
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

# The asset id a mint enters the digests as, which register-asset writes into the
# program permanently, and the decimals the two registered mints carry.
solana_refuses "assets[1].asset_id: Sidiora's mint enters the digests as" \
    "Sidiora's mint carrying an asset id the chain did not fix for it" \
    '.assets[1].asset_id = "0xcf996523B5d068A26f0aa8a116602fE5033Ee3A1"' solana-sidiora-foreign-id
solana_refuses "is the id the chain fixed for Sidiora's mint" \
    "another mint carrying the id the chain fixed for Sidiora" \
    '.assets[0].asset_id = "0x21f7b20a555199fa73A238B1a91FD0f549068fEe"' solana-sidiora-id-elsewhere
solana_refuses 'enters the digests as its derived handle' \
    'a mint carrying an asset id that is not its derived handle' \
    '.assets[0].asset_id = "0x1111111111111111111111111111111111111111"' solana-foreign-handle
solana_refuses 'Sidiora carries 6 decimals, not 9' 'Sidiora carrying other decimals' \
    '.assets[1].decimals = 9' solana-sidiora-nine-decimals
solana_refuses 'wrapped SOL carries 9 decimals, not 8' 'wrapped SOL carrying other decimals' \
    '.assets[0].decimals = 8' solana-wrapped-sol-eight-decimals

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

# deploy-solana-program.sh: the vault authority is the PDA of the program's own
# seed. The program declares it as VAULT_SEED, the recorded fixture pins the PDA
# and handle of the bridge/vectors program id under it, and the arithmetic that
# derives it here is checked against that record before it judges the script.
PROGRAM_SEED=$(sed -n 's/^pub const VAULT_SEED: &\[u8\] = b"\([^"]*\)";$/\1/p' "$PROGRAM_STATE")
[ -n "$PROGRAM_SEED" ] || fail "$PROGRAM_STATE declares no VAULT_SEED"
VAULT_PROGRAM=$(jq -r .program_id "$VAULT_FIXTURE")
VAULT_AUTHORITY=$(jq -r .vault_authority "$VAULT_FIXTURE")
VAULT_HANDLE=$(jq -r .vault_handle "$VAULT_FIXTURE")
[ "$(jq -r .seed "$VAULT_FIXTURE")" = "$PROGRAM_SEED" ] \
    || fail "$VAULT_FIXTURE records the seed $(jq -r .seed "$VAULT_FIXTURE"), and the program declares $PROGRAM_SEED"
for pinned in "VectorProgramID = mustKeyHex(\"$(jq -r .program_id_hex "$VAULT_FIXTURE")\")" \
    "VectorVaultAuthority = mustKeyHex(\"$(jq -r .vault_authority_hex "$VAULT_FIXTURE")\")" \
    "VectorVaultHandle = mustAddress(\"${VAULT_HANDLE#0x}\")" \
    "VectorVaultAuthorityBump uint8 = $(jq -r .bump "$VAULT_FIXTURE")"; do
    grep -qF -- "$pinned" "$SOLANA_VECTORS" || fail "$SOLANA_VECTORS does not pin $pinned"
done
[ "$(python3 "$SOLANA_KEYS" hex "$VAULT_PROGRAM")" = "$(jq -r .program_id_hex "$VAULT_FIXTURE")" ] \
    || fail "$VAULT_FIXTURE: program_id is not program_id_hex"
[ "$(python3 "$SOLANA_KEYS" hex "$VAULT_AUTHORITY")" = "$(jq -r .vault_authority_hex "$VAULT_FIXTURE")" ] \
    || fail "$VAULT_FIXTURE: vault_authority is not vault_authority_hex"
DERIVED=$(python3 "$SOLANA_KEYS" pda "$VAULT_PROGRAM" "string:$PROGRAM_SEED")
[ "$DERIVED" = "$VAULT_AUTHORITY $(jq -r .bump "$VAULT_FIXTURE")" ] \
    || fail "the seed $PROGRAM_SEED derives $DERIVED under $VAULT_PROGRAM, and the fixture records $VAULT_AUTHORITY"
DIGEST=$(cast keccak "0x$(python3 "$SOLANA_KEYS" hex "$VAULT_AUTHORITY")")
[ "0x${DIGEST: -40}" = "$VAULT_HANDLE" ] \
    || fail "the handle of $VAULT_AUTHORITY is 0x${DIGEST: -40}, and the fixture records $VAULT_HANDLE"
[ "$(jq -r .program_id "$SOLANA_FIXTURES/cluster.json")" = "$VAULT_PROGRAM" ] \
    || fail "the recorded cluster deploys another program than the vault fixture's"

# A deployment script names the program's seed when its VAULT_AUTHORITY_SEED is
# that seed and every PDA it derives is derived from VAULT_AUTHORITY_SEED.
names_program_seed() {
    local script=$1 seed derivations
    seed=$(sed -n 's/^VAULT_AUTHORITY_SEED=//p' "$script")
    if [ "$seed" != "$PROGRAM_SEED" ]; then
        printf 'VAULT_AUTHORITY_SEED is %s, and the program derives its vault authority from %s\n' \
            "${seed:-nothing}" "$PROGRAM_SEED"
        return 1
    fi
    derivations=$(grep -c 'find-program-derived-address' "$script" || true)
    # shellcheck disable=SC2016 # the literal text the script derives its PDA from
    if [ "$derivations" -ne 1 ] \
        || [ "$(grep -o 'string:[^"[:space:]]*' "$script" | sort -u)" != 'string:$VAULT_AUTHORITY_SEED' ]; then
        printf 'the script derives an address from a seed other than VAULT_AUTHORITY_SEED\n'
        return 1
    fi
}

# A deployment record carries the vault when its program, vault authority and
# handle are the recorded fixture's.
records_program_vault() {
    local record=$1 field expected found
    for field in program_id vault_authority vault_handle; do
        expected=$(jq -r --arg field "$field" '.[$field]' "$VAULT_FIXTURE")
        found=$(jq -r --arg field "$field" '.[$field] // "nothing"' "$record")
        if [ "$found" != "$expected" ]; then
            printf '%s records %s as %s, and the program derives %s\n' "$record" "$field" "$found" "$expected"
            return 1
        fi
    done
}

REASON=$(names_program_seed "$DEPLOY_SOLANA") || fail "$DEPLOY_SOLANA: $REASON"

# A platform tools release as a number that orders releases; the patch level
# does not move a release across the edition 2024 line.
tools_release_number() {
    [[ $1 =~ ^v([0-9]+)\.([0-9]+)(\.[0-9]+)?$ ]] || return 1
    printf '%d\n' $((10#${BASH_REMATCH[1]} * 1000 + 10#${BASH_REMATCH[2]}))
}

# A deployment script and the workflow pin the same platform tools when the
# script declares a release no older than OLDEST_PLATFORM_TOOLS, hands it to every
# cargo-build-sbf it runs as --tools-version, and every cargo build-sbf the
# workflow runs names that release.
pins_platform_tools() {
    local script=$1 workflow=$2 version number line named
    local -a builds steps
    version=$(sed -n 's/^PLATFORM_TOOLS_VERSION=//p' "$script")
    if [ -z "$version" ]; then
        printf 'the script declares no PLATFORM_TOOLS_VERSION\n'
        return 1
    fi
    if ! number=$(tools_release_number "$version"); then
        printf 'PLATFORM_TOOLS_VERSION is %s, which names no platform tools release\n' "$version"
        return 1
    fi
    if [ "$number" -lt "$(tools_release_number "$OLDEST_PLATFORM_TOOLS")" ]; then
        printf 'PLATFORM_TOOLS_VERSION is %s, older than %s, the first platform tools release whose cargo accepts edition 2024\n' \
            "$version" "$OLDEST_PLATFORM_TOOLS"
        return 1
    fi
    # shellcheck disable=SC2016 # the literal text the script runs its build with
    mapfile -t builds < <(grep -F '"$BUILD_SBF" ' "$script" || true)
    if [ "${#builds[@]}" -eq 0 ]; then
        printf 'the script runs no cargo-build-sbf\n'
        return 1
    fi
    for line in "${builds[@]}"; do
        # shellcheck disable=SC2016 # the literal text the script passes
        if ! grep -qF -- '"$BUILD_SBF" --tools-version "$PLATFORM_TOOLS_VERSION" ' <<< "$line"; then
            printf 'the script runs cargo-build-sbf without --tools-version "$PLATFORM_TOOLS_VERSION": %s\n' "$line"
            return 1
        fi
    done
    mapfile -t steps < <(grep -E 'cargo build-sbf( |$)' "$workflow" | grep -vE '^[[:space:]]*(- )?name:' || true)
    if [ "${#steps[@]}" -eq 0 ]; then
        printf '%s runs no cargo build-sbf\n' "$workflow"
        return 1
    fi
    for line in "${steps[@]}"; do
        named=$(grep -oE -- '--tools-version[ =][^[:space:]]+' <<< "$line" | sed -E 's/^--tools-version[ =]//' || true)
        if [ -z "$named" ]; then
            printf '%s runs cargo build-sbf without --tools-version, and the script declares %s: %s\n' \
                "$workflow" "$version" "${line#"${line%%[![:space:]]*}"}"
            return 1
        fi
        if [ "$named" != "$version" ]; then
            printf '%s runs cargo build-sbf with platform tools %s, and the script declares %s\n' \
                "$workflow" "$named" "$version"
            return 1
        fi
    done
}

REASON=$(pins_platform_tools "$DEPLOY_SOLANA" "$WORKFLOW") || fail "$DEPLOY_SOLANA: $REASON"
PLATFORM_TOOLS=$(sed -n 's/^PLATFORM_TOOLS_VERSION=//p' "$DEPLOY_SOLANA")

# Each way of losing the pin is caught: a workflow step that names no release or
# another one, a script that declares a release older than the edition 2024 line,
# and a script whose cargo-build-sbf is not handed the release it declares.
PINNED_PATTERN=${PLATFORM_TOOLS//./\\.}
mkdir -p "$WORK/unpinned"
UNPINNED_SCRIPT="$WORK/unpinned/deploy-solana-program.sh"
UNPINNED_WORKFLOW="$WORK/unpinned/bridge-test.yml"
# shellcheck disable=SC2016 # sed expressions matching the files' literal text
for mutation in "workflow|s/ --tools-version $PINNED_PATTERN//|without --tools-version" \
    "workflow|s/--tools-version $PINNED_PATTERN/--tools-version v1.55/|with platform tools v1.55" \
    "both|s/$PINNED_PATTERN/v1.51/|older than $OLDEST_PLATFORM_TOOLS" \
    'script|s/ --tools-version "\$PLATFORM_TOOLS_VERSION"//|without --tools-version "$PLATFORM_TOOLS_VERSION"'; do
    IFS='|' read -r target expression reason <<< "$mutation"
    cp "$DEPLOY_SOLANA" "$UNPINNED_SCRIPT"
    cp "$WORKFLOW" "$UNPINNED_WORKFLOW"
    case $target in
    workflow) sed -i -e "$expression" "$UNPINNED_WORKFLOW" ;;
    script) sed -i -e "$expression" "$UNPINNED_SCRIPT" ;;
    both) sed -i -e "$expression" "$UNPINNED_SCRIPT" "$UNPINNED_WORKFLOW" ;;
    esac
    if cmp -s "$DEPLOY_SOLANA" "$UNPINNED_SCRIPT" && cmp -s "$WORKFLOW" "$UNPINNED_WORKFLOW"; then
        fail "the mutation $expression changed nothing in $DEPLOY_SOLANA or $WORKFLOW"
    fi
    if REASON=$(pins_platform_tools "$UNPINNED_SCRIPT" "$UNPINNED_WORKFLOW"); then
        fail "a $target changed by $expression was taken to pin platform tools $PLATFORM_TOOLS"
    fi
    grep -qF -- "$reason" <<< "$REASON" \
        || fail "a $target changed by $expression was refused without naming: $reason ($REASON)"
done

# The recorded cluster's toolchain, behind a cargo-build-sbf that answers only a
# build asked for the declared platform tools, so the first deployment below
# proves the script hands its cargo-build-sbf the pinned release.
PINNED_TOOLCHAIN="$WORK/pinned-toolchain"
mkdir -p "$PINNED_TOOLCHAIN"
for tool in solana solana-keygen; do
    printf '#!/usr/bin/env bash\nexec %q "$@"\n' "$REPLAY_TOOLCHAIN/$tool" > "$PINNED_TOOLCHAIN/$tool"
done
# shellcheck disable=SC2016 # the stand-in is written as literal shell text
{
    printf '#!/usr/bin/env bash\n'
    printf 'if [ "${1:-}" != --tools-version ] || [ "${2:-}" != %q ]; then\n' "$PLATFORM_TOOLS"
    printf '    printf %q "$*" >&2\n' "cargo-build-sbf was not asked for platform tools $PLATFORM_TOOLS: %s\n"
    printf '    exit 64\nfi\nshift 2\n'
    printf 'exec %q "$@"\n' "$REPLAY_TOOLCHAIN/cargo-build-sbf"
} > "$PINNED_TOOLCHAIN/cargo-build-sbf"
chmod 0755 "$PINNED_TOOLCHAIN"/*

# deploy-solana-program.sh: a first deployment against the recorded cluster. The
# committed solana.program_id placeholder is kept, so the run deploys, records
# the program and its vault, and stops before the initialise step naming the
# field. The admin client stand-in fails any instruction it is handed, so a run
# that went on to initialise would be refused for that instead.
first_deployment() {
    local script=$1 name=$2 root state
    root=$(solana_configuration "$name")
    state="$WORK/replay-$name"
    mkdir -p "$state"
    attempt env "PAXEER_BRIDGE_SOLANA_CHAINS_ROOT=$root" \
        "PAXEER_BRIDGE_SOLANA_RPC_URL=$ENDPOINT" \
        "PAXEER_BRIDGE_SOLANA_KEYPAIR_FILE=$WORK/publisher.json" \
        "PAXEER_BRIDGE_SOLANA_TOOLCHAIN_BIN=$PINNED_TOOLCHAIN" \
        "PAXEER_BRIDGE_DEPLOYMENT_RECORD=$WORK/records/$name.json" \
        "PAXEER_BRIDGE_SOLANA_ADMIN_CLI=$ADMIN" \
        "DEPLOY_CHECK_REPLAY_STATE=$state" \
        bash "$script"
}

accepts 'is ready to deploy' 'a first Solana deployment against the recorded cluster, at --preflight' \
    env "PAXEER_BRIDGE_SOLANA_CHAINS_ROOT=$SOLANA_ROOT" "PAXEER_BRIDGE_SOLANA_RPC_URL=$ENDPOINT" \
    "PAXEER_BRIDGE_SOLANA_KEYPAIR_FILE=$WORK/publisher.json" \
    "PAXEER_BRIDGE_SOLANA_TOOLCHAIN_BIN=$PINNED_TOOLCHAIN" \
    "PAXEER_BRIDGE_DEPLOYMENT_RECORD=$WORK/records/solana.json" \
    "PAXEER_BRIDGE_SOLANA_ADMIN_CLI=$ADMIN" \
    bash "$DEPLOY_SOLANA" --preflight

first_deployment "$DEPLOY_SOLANA" solana-first-deployment
FIRST_RECORD="$WORK/records/solana-first-deployment.json"
if grep -qF 'was not asked for platform tools' "$WORK/last.log"; then
    quote
    fail "a first Solana deployment ran cargo-build-sbf without platform tools $PLATFORM_TOOLS"
fi
[ "$STATUS" -ne 0 ] || {
    quote
    fail 'a first Solana deployment with a placeholder solana.program_id went on to the initialise step'
}
for needle in "solana.program_id: PLACEHOLDER:program-id is still a placeholder" \
    'stops before the initialise step' "set solana.program_id to $VAULT_PROGRAM"; do
    grep -qF -- "$needle" "$WORK/last.log" || {
        quote
        fail "a first Solana deployment was refused without naming: $needle"
    }
done
if grep -qF 'could not initialise' "$WORK/last.log"; then
    quote
    fail 'a first Solana deployment reached the admin client before refusing its placeholder'
fi
[ -r "$FIRST_RECORD" ] || {
    quote
    fail 'a first Solana deployment wrote no deployment record'
}
REASON=$(records_program_vault "$FIRST_RECORD") || fail "a first Solana deployment: $REASON"
for field in owner attestors threshold assets; do
    [ "$(jq --arg field "$field" 'has($field)' "$FIRST_RECORD")" = false ] \
        || fail "a first Solana deployment records $field, which only an initialised program has"
done
[ "$(jq -r .program_data_account "$FIRST_RECORD")" = "$(python3 "$SOLANA_KEYS" pda \
    BPFLoaderUpgradeab1e11111111111111111111111 "pubkey:$VAULT_PROGRAM" | cut -d ' ' -f 1)" ] \
    || fail 'a first Solana deployment records a program data account the loader does not derive'

# A deployment script that names any other seed is caught twice: by its source,
# and by the vault its first deployment records. The seed the program declared
# before it settled on its own is the first such seed tried.
mkdir -p "$WORK/other-seed/bridge/deploy"
ln -s "$REPO_ROOT/bridge/solana" "$WORK/other-seed/bridge/solana"
OTHER_SEED_SCRIPT="$WORK/other-seed/bridge/deploy/deploy-solana-program.sh"
# shellcheck disable=SC2016 # sed expressions matching the script's literal text
for mutation in 's/^VAULT_AUTHORITY_SEED=.*/VAULT_AUTHORITY_SEED=vault/' \
    's/"string:\$VAULT_AUTHORITY_SEED"/"string:vault"/'; do
    sed -e "$mutation" "$DEPLOY_SOLANA" > "$OTHER_SEED_SCRIPT"
    ! cmp -s "$DEPLOY_SOLANA" "$OTHER_SEED_SCRIPT" || fail "the mutation $mutation changed nothing in $DEPLOY_SOLANA"
    if names_program_seed "$OTHER_SEED_SCRIPT" > /dev/null; then
        fail "a deployment script changed by $mutation was taken to name the program's seed"
    fi
    rm -f "$WORK/records/solana-other-seed.json"
    first_deployment "$OTHER_SEED_SCRIPT" solana-other-seed
    [ -r "$WORK/records/solana-other-seed.json" ] || {
        quote
        fail "a deployment script changed by $mutation wrote no record to compare"
    }
    if records_program_vault "$WORK/records/solana-other-seed.json" > /dev/null; then
        fail "a deployment script changed by $mutation recorded the program's own vault"
    fi
done

printf 'deploy-scripts-check: the %d committed chain configurations are refused while they carry their placeholders, a filled one is ready to deploy, every argument, placeholder, missing variable and inconsistent record is refused, a first Solana deployment builds with platform tools %s, which the workflow names too, records the vault the program derives from %s and stops before the initialise step\n' \
    $((${#EVM_CHAIN_NAMES[@]} + 1)) "$PLATFORM_TOOLS" "$PROGRAM_SEED" >&2
