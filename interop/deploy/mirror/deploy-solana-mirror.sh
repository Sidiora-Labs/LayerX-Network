#!/usr/bin/env bash
# Builds interop/contracts/solana-mirror with the pinned Solana platform tools
# and deploys it to one Solana cluster, then writes the deployment record
# solana-deployment.json requires: genesis_hash, program_id,
# publisher_ed25519_public_key, program_data_account, upgradeable_loader_id,
# program_elf_sha256, deployment_signature and rooted_slot.
#
# Inputs (environment variables, all required unless noted):
#   LAYERX_SOLANA_MIRROR_RPC_URL           JSON-RPC endpoint the deployment is broadcast through
#   LAYERX_SOLANA_MIRROR_KEYPAIR_FILE      64-byte publisher keypair that pays for and owns the deploy
#   LAYERX_SOLANA_MIRROR_DEPLOYMENT_RECORD output path of the deployment record
#   LAYERX_SOLANA_MIRROR_TOOLCHAIN_BIN     directory holding the pinned solana, solana-keygen and
#                                          cargo-build-sbf (the toolchain solana-devnet-2026-08-31.json pins)
#   LAYERX_SOLANA_MIRROR_PROGRAM_KEYPAIR_FILE  optional program keypair, so a redeploy keeps its program id
#
# The Solana CLI addresses an endpoint by URL alone: it has no option for an
# Authorization header, so this script cannot reach an endpoint that
# authenticates with a bearer token. Deploy through an endpoint whose URL
# carries its own credential, or supply a record built elsewhere through
# LAYERX_BETA_MIRROR_SOLANA_DEPLOYMENT_FILE.
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
CONTRACT_DIR=$(cd "$SCRIPT_DIR/../../contracts/solana-mirror" && pwd)
UPGRADEABLE_LOADER=BPFLoaderUpgradeab1e11111111111111111111111

fail() { printf 'deploy-solana-mirror: error: %s\n' "$*" >&2; exit 1; }

for variable in LAYERX_SOLANA_MIRROR_RPC_URL LAYERX_SOLANA_MIRROR_KEYPAIR_FILE \
    LAYERX_SOLANA_MIRROR_DEPLOYMENT_RECORD LAYERX_SOLANA_MIRROR_TOOLCHAIN_BIN; do
    [ -n "${!variable:-}" ] || fail "$variable is required"
done
[ -r "$LAYERX_SOLANA_MIRROR_KEYPAIR_FILE" ] || fail "LAYERX_SOLANA_MIRROR_KEYPAIR_FILE is not readable"
[[ $LAYERX_SOLANA_MIRROR_RPC_URL =~ ^https:// ]] \
    || fail "LAYERX_SOLANA_MIRROR_RPC_URL must be an https endpoint"

SOLANA="$LAYERX_SOLANA_MIRROR_TOOLCHAIN_BIN/solana"
KEYGEN="$LAYERX_SOLANA_MIRROR_TOOLCHAIN_BIN/solana-keygen"
BUILD_SBF="$LAYERX_SOLANA_MIRROR_TOOLCHAIN_BIN/cargo-build-sbf"
for tool in "$SOLANA" "$KEYGEN" "$BUILD_SBF"; do
    [ -x "$tool" ] \
        || fail "$tool is not executable; LAYERX_SOLANA_MIRROR_TOOLCHAIN_BIN must hold the pinned Solana toolchain (solana, solana-keygen, cargo-build-sbf)"
done

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
chmod 0700 "$work"

genesis_hash=$("$SOLANA" genesis-hash --url "$LAYERX_SOLANA_MIRROR_RPC_URL") \
    || fail "the RPC endpoint did not answer getGenesisHash"
publisher=$("$KEYGEN" pubkey "$LAYERX_SOLANA_MIRROR_KEYPAIR_FILE") \
    || fail "LAYERX_SOLANA_MIRROR_KEYPAIR_FILE is not a Solana keypair"

"$BUILD_SBF" --manifest-path "$CONTRACT_DIR/Cargo.toml" --sbf-out-dir "$work/deploy" \
    > "$work/build.log" 2>&1 || { cat "$work/build.log" >&2; fail "cargo-build-sbf could not build $CONTRACT_DIR"; }
elf="$work/deploy/layerx_solana_mirror_program.so"
[ -f "$elf" ] || fail "cargo-build-sbf produced no layerx_solana_mirror_program.so in $work/deploy"
elf_sha256=$(sha256sum "$elf" | cut -d ' ' -f 1)
elf_bytes=$(wc -c < "$elf" | tr -d ' ')

deploy=("$SOLANA" program deploy "$elf" --url "$LAYERX_SOLANA_MIRROR_RPC_URL"
    --keypair "$LAYERX_SOLANA_MIRROR_KEYPAIR_FILE"
    --upgrade-authority "$LAYERX_SOLANA_MIRROR_KEYPAIR_FILE" --output json)
if [ -n "${LAYERX_SOLANA_MIRROR_PROGRAM_KEYPAIR_FILE:-}" ]; then
    [ -r "$LAYERX_SOLANA_MIRROR_PROGRAM_KEYPAIR_FILE" ] \
        || fail "LAYERX_SOLANA_MIRROR_PROGRAM_KEYPAIR_FILE is not readable"
    deploy+=(--program-id "$LAYERX_SOLANA_MIRROR_PROGRAM_KEYPAIR_FILE")
fi
"${deploy[@]}" > "$work/deploy.json" 2> "$work/deploy.log" \
    || { cat "$work/deploy.log" >&2; fail "solana program deploy failed"; }

program_id=$(jq -r '.programId // empty' "$work/deploy.json")
deployment_signature=$(jq -r '.signature // empty' "$work/deploy.json")
[ -n "$program_id" ] || fail "solana program deploy reported no program id: $(cat "$work/deploy.json")"
[ -n "$deployment_signature" ] \
    || fail "solana program deploy reported no deployment signature, which solana-deployment.json records: $(cat "$work/deploy.json")"

"$SOLANA" program show "$program_id" --url "$LAYERX_SOLANA_MIRROR_RPC_URL" --output json \
    > "$work/show.json" 2> "$work/show.log" \
    || { cat "$work/show.log" >&2; fail "solana program show $program_id failed"; }
program_data=$(jq -r '.programdataAddress // empty' "$work/show.json")
authority=$(jq -r '.authority // empty' "$work/show.json")
deployed_slot=$(jq -r '.lastDeploySlot // empty' "$work/show.json")
[ -n "$program_data" ] || fail "solana program show reported no program data account for $program_id"
[ "$authority" = "$publisher" ] \
    || fail "the deployed program upgrade authority $authority is not the publisher $publisher"
[[ $deployed_slot =~ ^[0-9]+$ ]] || fail "solana program show reported no deploy slot for $program_id"

rooted_slot=$("$SOLANA" slot --url "$LAYERX_SOLANA_MIRROR_RPC_URL" --commitment finalized) \
    || fail "the RPC endpoint did not answer getSlot at the finalized commitment"
[[ $rooted_slot =~ ^[0-9]+$ ]] || fail "the endpoint reported no finalized slot"
[ "$rooted_slot" -ge "$deployed_slot" ] \
    || fail "the finalized slot $rooted_slot is behind the deploy slot $deployed_slot; the deployment is not rooted yet"

umask 077
jq -n --arg program layerx-solana-mirror-program \
    --arg source interop/contracts/solana-mirror \
    --arg genesis_hash "$genesis_hash" \
    --arg program_id "$program_id" \
    --arg publisher "$publisher" \
    --arg program_data_account "$program_data" \
    --arg upgradeable_loader_id "$UPGRADEABLE_LOADER" \
    --arg program_elf_sha256 "$elf_sha256" \
    --argjson program_elf_bytes "$elf_bytes" \
    --arg deployment_signature "$deployment_signature" \
    --argjson deployment_finalized_slot "$deployed_slot" \
    --argjson rooted_slot "$rooted_slot" \
    --arg upgrade_authority "$authority" \
    '{program: $program, source: $source, genesis_hash: $genesis_hash, program_id: $program_id,
      publisher_ed25519_public_key: $publisher, program_data_account: $program_data_account,
      upgradeable_loader_id: $upgradeable_loader_id, program_elf_sha256: $program_elf_sha256,
      program_elf_bytes: $program_elf_bytes, deployment_signature: $deployment_signature,
      deployment_finalized_slot: $deployment_finalized_slot, rooted_slot: $rooted_slot,
      upgrade_authority: $upgrade_authority}' > "$LAYERX_SOLANA_MIRROR_DEPLOYMENT_RECORD"

printf 'deploy-solana-mirror: %s (program data %s) deployed by %s, rooted at slot %s\n' \
    "$program_id" "$program_data" "$publisher" "$rooted_slot" >&2
