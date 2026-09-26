#!/usr/bin/env bash
# Builds the Paxeer X Network bridge custody program in bridge/solana with the
# pinned Solana platform tools, deploys it to one cluster, initialises it with
# the owner, the attestor set and the threshold its configuration names,
# registers every asset the configuration names, and writes the deployment
# record.
#
# Usage:
#   deploy-solana-program.sh [--preflight]
#
# --preflight runs every check that needs no cluster - the configuration, the
# environment variables it names, the key file, the program's admin client and
# the pinned toolchain - and stops before the first call to the cluster. The
# program's own sources are checked where they are built, when the run deploys.
#
# Inputs, all through environment variables and never through a literal here or
# in the configuration:
#   the variable the configuration names in environment.rpc_url        the endpoint
#   the variable the configuration names in environment.deploy_key     the publisher keypair file
#   the variable the configuration names in environment.toolchain_bin  the pinned toolchain directory
#                                       holding solana, solana-keygen and cargo-build-sbf
#   PAXEER_BRIDGE_DEPLOYMENT_RECORD     where the deployment record is written
#   PAXEER_BRIDGE_SOLANA_ADMIN_CLI      the program's admin client, which encodes the
#                                       initialise and register-asset instructions
#   PAXEER_BRIDGE_SOLANA_PROGRAM_KEYPAIR_FILE  optional program keypair, so a redeploy keeps its id
#   PAXEER_BRIDGE_SOLANA_CHAINS_ROOT    optional chains root, for a run-local configuration
#
# The Solana CLI addresses an endpoint by URL alone and has no option for an
# Authorization header, so this script cannot reach an endpoint that
# authenticates with a bearer token; deploy through an endpoint whose URL
# carries its own credential.
#
# The configuration's solana.program_id is the id the program is deployed to
# once it exists. A placeholder is the first deployment: the run deploys, writes
# the deployed program id and its vault authority into the deployment record and
# stops before the initialise step, naming solana.program_id, because the admin
# client initialises only the program the configuration names. A filled id must
# be reproduced by the program keypair the run deploys with and stops the run if
# the deployment lands elsewhere.
#
# The program is built with the platform tools release PLATFORM_TOOLS_VERSION
# names, passed to cargo-build-sbf as --tools-version, and not with the release
# the Solana toolchain installs by default, whose cargo rejects the dependency
# manifests that declare edition 2024. cargo-build-sbf fetches that release on
# its first use; the toolchain directory still supplies solana, solana-keygen and
# cargo-build-sbf itself.
#
# The vault authority is the PDA of the seed the program declares as VAULT_SEED
# in bridge/solana/src/state.rs, and its handle is the vault Paxeer registers.
#
# Every asset id is checked against the handle the chain derives for its mint
# before the cluster is reached, and Sidiora's fixed id is accepted for Sidiora's
# mint alone, because registering an asset writes that mapping into the program
# permanently. bridge/deploy/chainconfig says the same of a committed
# configuration; this says it of the configuration the deployment is handed,
# whichever chains root it came from.
#
# Every placeholder, every missing variable, every unreadable key file, an
# endpoint that answers no genesis hash, an upgrade authority that is not the
# publisher, a deployment that is not rooted and a deployed ELF that is not the
# built one stops the run. There is no default endpoint, key, owner or address.
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd "$SCRIPT_DIR/../.." && pwd)
PROGRAM_DIR="$REPO_ROOT/bridge/solana"
PLACEHOLDER_PREFIX='PLACEHOLDER:'
SOLANA_CHAIN_ID=91600046870081
WRAPPED_SOL_MINT=So11111111111111111111111111111111111111112
WRAPPED_SOL_DECIMALS=9
SIDIORA_MINT=5w3wVdJaESaJKyLmStM6Hv9UyUkmZ1b9DLQquAqqpump
SIDIORA_ASSET_ID=0x21f7b20a555199fa73a238b1a91fd0f549068fee
SIDIORA_DECIMALS=6
UPGRADEABLE_LOADER=BPFLoaderUpgradeab1e11111111111111111111111
VAULT_AUTHORITY_SEED=vault-authority
PLATFORM_TOOLS_VERSION=v1.56

fail() {
    printf 'deploy-solana-program: error: %s\n' "$*" >&2
    exit 1
}

usage() {
    printf 'usage: deploy-solana-program.sh [--preflight]\n' >&2
    exit 2
}

preflight_only=0
case ${1:-} in
--preflight)
    preflight_only=1
    shift
    ;;
"") ;;
*)
    usage
    ;;
esac
[ $# -eq 0 ] || usage

for tool in jq python3 cast sha256sum; do
    command -v "$tool" > /dev/null 2>&1 || fail "$tool is required and is not on the PATH"
done

chains_root=${PAXEER_BRIDGE_SOLANA_CHAINS_ROOT:-$PROGRAM_DIR/chains}
config="$chains_root/solana/config.json"
[ -r "$config" ] || fail "$config is not readable"

refuse() { fail "$config: $*"; }

jq -e . "$config" > /dev/null 2>&1 || refuse "the file is not JSON"

kind=$(jq -r '.kind // empty' "$config")
[ "$kind" = solana ] || refuse "kind: $kind is not the Solana chain"
declared=$(jq -r '.chain // empty' "$config")
[ "$declared" = solana ] || refuse "chain: the file names $declared"
chain_id=$(jq -r '.chain_id // empty' "$config")
[ "$chain_id" = "$SOLANA_CHAIN_ID" ] \
    || refuse "chain_id: Solana is chain $SOLANA_CHAIN_ID on the Paxeer side, not $chain_id"
commitment=$(jq -r '.solana.commitment // empty' "$config")
case $commitment in
confirmed | finalized) ;;
*)
    refuse "solana.commitment: $commitment would read state that can still be dropped"
    ;;
esac
finality_depth=$(jq -r '.finality_depth // empty' "$config")
[[ $finality_depth =~ ^[1-9][0-9]*$ ]] \
    || refuse "finality_depth: $finality_depth is not a slot depth above zero"

lower() { printf '%s' "$1" | tr '[:upper:]' '[:lower:]'; }

require_no_placeholder() {
    local field=$1 value=$2
    case $value in
    "$PLACEHOLDER_PREFIX"*)
        refuse "$field: $value is a placeholder; fill in the real value before deploying"
        ;;
    esac
}

require_pubkey() {
    local field=$1 value=$2
    require_no_placeholder "$field" "$value"
    [[ $value =~ ^[1-9A-HJ-NP-Za-km-z]{32,44}$ ]] || refuse "$field: $value is not a base58 Solana key"
    [ "$value" != 11111111111111111111111111111111 ] || refuse "$field: the zero pubkey is not a value"
}

require_asset_id() {
    local field=$1 value=$2
    [[ $value =~ ^0x[0-9a-fA-F]{40}$ ]] || refuse "$field: $value is not a 20-byte asset id"
    [ "$(lower "$value")" != 0x0000000000000000000000000000000000000000 ] \
        || refuse "$field: the zero id is not an asset id"
}

require_amount() {
    local field=$1 value=$2
    [[ $value =~ ^[1-9][0-9]*$ ]] || refuse "$field: $value is not a cap above zero"
}

owner=$(jq -r '.owner // empty' "$config")
require_pubkey owner "$owner"

threshold=$(jq -r '.threshold // empty' "$config")
[[ $threshold =~ ^[1-9][0-9]*$ ]] || refuse "threshold: $threshold is not a threshold above zero"
mapfile -t attestors < <(jq -r '.attestors[]' "$config")
[ "${#attestors[@]}" -gt 0 ] || refuse "attestors: the attestor set is empty"
[ "$threshold" -le "${#attestors[@]}" ] \
    || refuse "threshold: $threshold is above the ${#attestors[@]} attestors of the set"
previous=""
for index in "${!attestors[@]}"; do
    attestor=${attestors[index]}
    require_no_placeholder "attestors[$index]" "$attestor"
    [[ $attestor =~ ^0x[0-9a-fA-F]{40}$ ]] \
        || refuse "attestors[$index]: $attestor is not a 20-byte secp256k1 address"
    current=$(lower "$attestor")
    [ "$current" != 0x0000000000000000000000000000000000000000 ] \
        || refuse "attestors[$index]: the zero address is not an attestor"
    if [ -n "$previous" ] && [[ ! $previous < $current ]]; then
        refuse "attestors[$index]: $attestor does not follow the attestor before it; the set is strictly ascending"
    fi
    previous=$current
done

base58_to_hex() {
    python3 - "$1" << 'PY'
import sys

ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
value = sys.argv[1]
number = 0
for character in value:
    position = ALPHABET.find(character)
    if position < 0:
        raise SystemExit("%s carries %r, which is not a base58 character" % (value, character))
    number = number * 58 + position
raw = number.to_bytes((number.bit_length() + 7) // 8, "big")
raw = b"\x00" * (len(value) - len(value.lstrip("1"))) + raw
if len(raw) != 32:
    raise SystemExit("%s decodes to %d bytes, not 32" % (value, len(raw)))
print(raw.hex())
PY
}

handle_of() {
    local key=$1 raw digest
    raw=$(base58_to_hex "$key") || fail "$key is not a 32-byte base58 key"
    digest=$(cast keccak "0x$raw") || fail "the handle of $key is not derivable"
    printf '0x%s' "${digest: -40}"
}

mapfile -t asset_mints < <(jq -r '.assets[].address' "$config")
mapfile -t asset_symbols < <(jq -r '.assets[].symbol' "$config")
mapfile -t asset_ids < <(jq -r '.assets[].asset_id' "$config")
mapfile -t asset_decimals < <(jq -r '.assets[].decimals' "$config")
mapfile -t asset_per_tx < <(jq -r '.assets[].per_tx_cap' "$config")
mapfile -t asset_total < <(jq -r '.assets[].total_cap' "$config")
[ "${#asset_mints[@]}" -gt 0 ] || refuse "assets: the asset list is empty"
[ "${asset_mints[0]}" = "$WRAPPED_SOL_MINT" ] \
    || refuse "assets[0].address: the first asset of Solana is the wrapped SOL mint $WRAPPED_SOL_MINT"
for index in "${!asset_mints[@]}"; do
    require_pubkey "assets[$index].address" "${asset_mints[index]}"
    require_asset_id "assets[$index].asset_id" "${asset_ids[index]}"
    decimals=${asset_decimals[index]}
    if ! [[ $decimals =~ ^[1-9][0-9]?$ ]] || [ "$decimals" -gt 18 ]; then
        refuse "assets[$index].decimals: $decimals is not a decimal count between 1 and 18"
    fi
    require_amount "assets[$index].per_tx_cap" "${asset_per_tx[index]}"
    require_amount "assets[$index].total_cap" "${asset_total[index]}"
    id=$(lower "${asset_ids[index]}")
    if [ "${asset_mints[index]}" = "$SIDIORA_MINT" ]; then
        [ "$id" = "$SIDIORA_ASSET_ID" ] \
            || refuse "assets[$index].asset_id: Sidiora's mint enters the digests as $SIDIORA_ASSET_ID, not as $id"
        [ "$decimals" -eq "$SIDIORA_DECIMALS" ] \
            || refuse "assets[$index].decimals: Sidiora carries $SIDIORA_DECIMALS decimals, not $decimals"
    else
        [ "$id" != "$SIDIORA_ASSET_ID" ] \
            || refuse "assets[$index].asset_id: $SIDIORA_ASSET_ID is the id the chain fixed for Sidiora's mint $SIDIORA_MINT, not for ${asset_mints[index]}"
        derived=$(lower "$(handle_of "${asset_mints[index]}")")
        [ "$id" = "$derived" ] \
            || refuse "assets[$index].asset_id: ${asset_mints[index]} enters the digests as its derived handle $derived, not as $id"
    fi
done
[ "${asset_decimals[0]}" -eq "$WRAPPED_SOL_DECIMALS" ] \
    || refuse "assets[0].decimals: wrapped SOL carries $WRAPPED_SOL_DECIMALS decimals, not ${asset_decimals[0]}"

rpc_variable=$(jq -r '.environment.rpc_url // empty' "$config")
key_variable=$(jq -r '.environment.deploy_key // empty' "$config")
toolchain_variable=$(jq -r '.environment.toolchain_bin // empty' "$config")
require_variable_name() {
    local field=$1 name=$2
    [ -n "$name" ] || refuse "environment.$field is required"
    [[ $name =~ ^[A-Z][A-Z0-9]*(_[A-Z0-9]+)*$ ]] \
        || refuse "environment.$field: $name is not an upper snake case environment variable name"
}
require_variable_name rpc_url "$rpc_variable"
require_variable_name deploy_key "$key_variable"
require_variable_name toolchain_bin "$toolchain_variable"

for variable in "$rpc_variable" "$key_variable" "$toolchain_variable" \
    PAXEER_BRIDGE_DEPLOYMENT_RECORD PAXEER_BRIDGE_SOLANA_ADMIN_CLI; do
    [ -n "${!variable:-}" ] || fail "$variable is required and is not set"
done

rpc=${!rpc_variable}
keypair=${!key_variable}
toolchain=${!toolchain_variable}
record=$PAXEER_BRIDGE_DEPLOYMENT_RECORD
admin=$PAXEER_BRIDGE_SOLANA_ADMIN_CLI

[[ $rpc =~ ^https?:// ]] || fail "$rpc_variable must carry an http or https endpoint"
[ -r "$keypair" ] || fail "$key_variable names $keypair, which is not readable"
record_directory=$(dirname "$record")
[ -d "$record_directory" ] || fail "$record_directory does not exist, so the deployment record cannot be written"
[ -w "$record_directory" ] || fail "$record_directory is not writable, so the deployment record cannot be written"

if [ -n "${PAXEER_BRIDGE_SOLANA_PROGRAM_KEYPAIR_FILE:-}" ]; then
    [ -r "$PAXEER_BRIDGE_SOLANA_PROGRAM_KEYPAIR_FILE" ] \
        || fail "PAXEER_BRIDGE_SOLANA_PROGRAM_KEYPAIR_FILE is not readable"
fi

configured_program_id=$(jq -r '.solana.program_id // empty' "$config")
[ -n "$configured_program_id" ] || refuse "solana.program_id is required"
case $configured_program_id in
"$PLACEHOLDER_PREFIX"*) ;;
*)
    require_pubkey solana.program_id "$configured_program_id"
    [ -n "${PAXEER_BRIDGE_SOLANA_PROGRAM_KEYPAIR_FILE:-}" ] \
        || fail "$config names program $configured_program_id, so PAXEER_BRIDGE_SOLANA_PROGRAM_KEYPAIR_FILE must name the keypair of that program id"
    ;;
esac

[ -x "$admin" ] \
    || fail "PAXEER_BRIDGE_SOLANA_ADMIN_CLI names $admin, which is not executable; it is the bridge/solana admin client that encodes the initialise and register-asset instructions"
SOLANA="$toolchain/solana"
KEYGEN="$toolchain/solana-keygen"
BUILD_SBF="$toolchain/cargo-build-sbf"
[ -d "$toolchain" ] || fail "$toolchain_variable names $toolchain, which is not a directory"
for tool in "$SOLANA" "$KEYGEN" "$BUILD_SBF"; do
    [ -x "$tool" ] \
        || fail "$tool is not executable; $toolchain_variable must hold the pinned Solana toolchain (solana, solana-keygen, cargo-build-sbf)"
done

if [ "$preflight_only" -eq 1 ]; then
    printf 'deploy-solana-program: solana (chain %s) is ready to deploy: owner %s, %d attestors at threshold %s, %d assets, commitment %s\n' \
        "$chain_id" "$owner" "${#attestors[@]}" "$threshold" "${#asset_mints[@]}" "$commitment" >&2
    exit 0
fi

[ -r "$PROGRAM_DIR/Cargo.toml" ] || fail "$PROGRAM_DIR/Cargo.toml is missing, so there is no program to build"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
chmod 0700 "$work"

genesis_hash=$("$SOLANA" genesis-hash --url "$rpc") \
    || fail "$rpc_variable did not answer getGenesisHash"
[ -n "$genesis_hash" ] || fail "$rpc_variable answered an empty genesis hash"
publisher=$("$KEYGEN" pubkey "$keypair") || fail "$key_variable does not name a Solana keypair"

"$BUILD_SBF" --tools-version "$PLATFORM_TOOLS_VERSION" \
    --manifest-path "$PROGRAM_DIR/Cargo.toml" --sbf-out-dir "$work/deploy" \
    > "$work/build.log" 2>&1 || {
    cat "$work/build.log" >&2
    fail "cargo-build-sbf could not build $PROGRAM_DIR with platform tools $PLATFORM_TOOLS_VERSION"
}
mapfile -t built < <(find "$work/deploy" -maxdepth 1 -name '*.so' -type f | sort)
[ "${#built[@]}" -eq 1 ] \
    || fail "cargo-build-sbf produced ${#built[@]} shared objects in $work/deploy, and a deployment needs exactly one"
elf=${built[0]}
elf_sha256=$(sha256sum "$elf" | cut -d ' ' -f 1)
elf_bytes=$(wc -c < "$elf" | tr -d ' ')

deploy=("$SOLANA" program deploy "$elf" --url "$rpc" --keypair "$keypair"
    --upgrade-authority "$keypair" --output json)
if [ -n "${PAXEER_BRIDGE_SOLANA_PROGRAM_KEYPAIR_FILE:-}" ]; then
    deploy+=(--program-id "$PAXEER_BRIDGE_SOLANA_PROGRAM_KEYPAIR_FILE")
fi
"${deploy[@]}" > "$work/deploy.json" 2> "$work/deploy.log" || {
    cat "$work/deploy.log" >&2
    fail "solana program deploy failed"
}
program_id=$(jq -r '.programId // empty' "$work/deploy.json")
deployment_signature=$(jq -r '.signature // empty' "$work/deploy.json")
[ -n "$program_id" ] || fail "solana program deploy reported no program id"
case $configured_program_id in
"$PLACEHOLDER_PREFIX"*) ;;
*)
    [ "$configured_program_id" = "$program_id" ] \
        || fail "$config names program $configured_program_id and this deployment is $program_id"
    ;;
esac
[ -n "$deployment_signature" ] || fail "solana program deploy reported no deployment signature"

"$SOLANA" program show "$program_id" --url "$rpc" --output json > "$work/show.json" 2> "$work/show.log" \
    || {
        cat "$work/show.log" >&2
        fail "solana program show $program_id failed"
    }
program_data=$(jq -r '.programdataAddress // empty' "$work/show.json")
authority=$(jq -r '.authority // empty' "$work/show.json")
deployed_slot=$(jq -r '.lastDeploySlot // empty' "$work/show.json")
[ -n "$program_data" ] || fail "solana program show reported no program data account for $program_id"
[ "$authority" = "$publisher" ] \
    || fail "the upgrade authority of $program_id is $authority, not the publisher $publisher"
[[ $deployed_slot =~ ^[0-9]+$ ]] || fail "solana program show reported no deploy slot for $program_id"

"$SOLANA" program dump "$program_id" "$work/deployed.so" --url "$rpc" > "$work/dump.log" 2>&1 \
    || {
        cat "$work/dump.log" >&2
        fail "solana program dump $program_id failed"
    }
deployed_sha256=$(head -c "$elf_bytes" "$work/deployed.so" | sha256sum | cut -d ' ' -f 1)
[ "$deployed_sha256" = "$elf_sha256" ] \
    || fail "the program at $program_id hashes to $deployed_sha256 and the built ELF hashes to $elf_sha256"

rooted_slot=$("$SOLANA" slot --url "$rpc" --commitment finalized) \
    || fail "$rpc_variable did not answer getSlot at the finalized commitment"
[[ $rooted_slot =~ ^[0-9]+$ ]] || fail "$rpc_variable reported no finalized slot"
[ "$rooted_slot" -ge "$deployed_slot" ] \
    || fail "the finalized slot $rooted_slot is behind the deploy slot $deployed_slot; the deployment is not rooted yet"

vault_authority=$("$SOLANA" find-program-derived-address "$program_id" "string:$VAULT_AUTHORITY_SEED" \
    2> "$work/pda.log" | head -n 1) || {
    cat "$work/pda.log" >&2
    fail "the vault-authority PDA of $program_id is not derivable"
}
[ -n "$vault_authority" ] || fail "the vault-authority PDA of $program_id is empty"
vault_handle=$(handle_of "$vault_authority")

case $configured_program_id in
"$PLACEHOLDER_PREFIX"*)
    umask 077
    jq -n --arg chain solana --argjson chain_id "$chain_id" --arg kind solana \
        --arg configuration "${config#"$REPO_ROOT"/}" \
        --arg genesis_hash "$genesis_hash" --arg program_id "$program_id" \
        --arg program_data_account "$program_data" --arg upgradeable_loader_id "$UPGRADEABLE_LOADER" \
        --arg program_elf_sha256 "$elf_sha256" --argjson program_elf_bytes "$elf_bytes" \
        --arg deployment_signature "$deployment_signature" --argjson deployment_slot "$deployed_slot" \
        --argjson rooted_slot "$rooted_slot" --arg publisher "$publisher" \
        --arg upgrade_authority "$authority" --arg commitment "$commitment" \
        --arg vault_authority "$vault_authority" --arg vault_handle "$vault_handle" \
        '{chain: $chain, chain_id: $chain_id, kind: $kind, configuration: $configuration,
          genesis_hash: $genesis_hash, program_id: $program_id,
          program_data_account: $program_data_account, upgradeable_loader_id: $upgradeable_loader_id,
          program_elf_sha256: $program_elf_sha256, program_elf_bytes: $program_elf_bytes,
          deployment_signature: $deployment_signature, deployment_slot: $deployment_slot,
          rooted_slot: $rooted_slot, publisher: $publisher, upgrade_authority: $upgrade_authority,
          commitment: $commitment, vault_authority: $vault_authority, vault_handle: $vault_handle}' \
        > "$record"
    printf 'deploy-solana-program: %s deployed and recorded in %s, vault authority %s, handle %s\n' \
        "$program_id" "$record" "$vault_authority" "$vault_handle" >&2
    refuse "solana.program_id: $configured_program_id is still a placeholder, so the run stops before the initialise step; set solana.program_id to $program_id, the program this run deployed"
    ;;
esac

attestor_list=$(
    IFS=,
    printf '%s' "${attestors[*]}"
)
"$admin" initialise --url "$rpc" --keypair "$keypair" --program-id "$program_id" \
    --commitment "$commitment" --owner "$owner" --attestors "$attestor_list" \
    --threshold "$threshold" > "$work/initialise.log" 2>&1 || {
    cat "$work/initialise.log" >&2
    fail "$admin could not initialise $program_id"
}

registered=$work/assets.json
printf '[]' > "$registered"
for index in "${!asset_mints[@]}"; do
    "$admin" register-asset --url "$rpc" --keypair "$keypair" --program-id "$program_id" \
        --commitment "$commitment" --mint "${asset_mints[index]}" --asset-id "${asset_ids[index]}" \
        --decimals "${asset_decimals[index]}" --per-tx-cap "${asset_per_tx[index]}" \
        --total-cap "${asset_total[index]}" > "$work/register-$index.log" 2>&1 || {
        cat "$work/register-$index.log" >&2
        fail "$admin could not register ${asset_symbols[index]} (${asset_mints[index]}) with $program_id"
    }
    jq --arg symbol "${asset_symbols[index]}" --arg mint "${asset_mints[index]}" \
        --arg asset_id "${asset_ids[index]}" --argjson decimals "${asset_decimals[index]}" \
        --arg per_tx "${asset_per_tx[index]}" --arg total "${asset_total[index]}" \
        '. + [{symbol: $symbol, mint: $mint, asset_id: $asset_id, decimals: $decimals,
               per_tx_cap: $per_tx, total_cap: $total}]' "$registered" > "$registered.next"
    mv "$registered.next" "$registered"
done

umask 077
jq -n --arg chain solana --argjson chain_id "$chain_id" --arg kind solana \
    --arg configuration "${config#"$REPO_ROOT"/}" \
    --arg genesis_hash "$genesis_hash" --arg program_id "$program_id" \
    --arg program_data_account "$program_data" --arg upgradeable_loader_id "$UPGRADEABLE_LOADER" \
    --arg program_elf_sha256 "$elf_sha256" --argjson program_elf_bytes "$elf_bytes" \
    --arg deployment_signature "$deployment_signature" --argjson deployment_slot "$deployed_slot" \
    --argjson rooted_slot "$rooted_slot" --arg publisher "$publisher" \
    --arg upgrade_authority "$authority" --arg owner "$owner" \
    --argjson threshold "$threshold" --arg commitment "$commitment" \
    --arg vault_authority "$vault_authority" --arg vault_handle "$vault_handle" \
    --argjson attestors "$(jq -n --arg list "$attestor_list" '$list | split(",")')" \
    --slurpfile assets "$registered" \
    '{chain: $chain, chain_id: $chain_id, kind: $kind, configuration: $configuration,
      genesis_hash: $genesis_hash, program_id: $program_id,
      program_data_account: $program_data_account, upgradeable_loader_id: $upgradeable_loader_id,
      program_elf_sha256: $program_elf_sha256, program_elf_bytes: $program_elf_bytes,
      deployment_signature: $deployment_signature, deployment_slot: $deployment_slot,
      rooted_slot: $rooted_slot, publisher: $publisher, upgrade_authority: $upgrade_authority,
      owner: $owner, threshold: $threshold, commitment: $commitment,
      vault_authority: $vault_authority, vault_handle: $vault_handle,
      attestors: $attestors, assets: $assets[0]}' > "$record"

printf 'deploy-solana-program: %s (program data %s) deployed by %s, rooted at slot %s\n' \
    "$program_id" "$program_data" "$publisher" "$rooted_slot" >&2
printf 'deploy-solana-program: vault authority %s, handle %s, which governance registers as Solana vault\n' \
    "$vault_authority" "$vault_handle" >&2
printf 'deploy-solana-program: record written to %s\n' "$record" >&2
