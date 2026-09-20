#!/usr/bin/env bash
# Registers the LayerX guarantor set in the native layerxanchor module through the precompile at
# 0x0000000000000000000000000000000000001014 and records the beta settlement domain.
#
# The LayerX node generates the guarantor identities after the Paxeer chain has started, so they
# are not Paxeer genesis state. Each bond controller calls registerGuarantor(bytes32,address) with
# the minimum bond as value (one bond unit of the base denom is 1e12 wei); the deployer, the
# module authority named in the anchor genesis, then calls activateGuarantor(bytes32). The script
# reads back guarantor(bytes32) and threshold() and refuses any record that differs.
#
# The settlement domain names 0x…1014 as both the settlement contract guarantors sign and the
# bond holder, and carries the minimum bond and the maximum attestation delay of the anchor
# genesis: the precompile has no view for those two parameters.
#
#   LAYERX_PAXEER_BOUNDARY_URL         https URL of the Paxeer boundary
#   LAYERX_PAXEER_BOUNDARY_CA_DER      DER certificate that issued the boundary certificate
#   LAYERX_PAXEER_CHAIN_ID             EVM chain id (default 125)
#   LAYERX_PAXEER_DEPLOYER_KEY_FILE    deployer key; the anchor authority
#   LAYERX_PAXEER_ANCHOR_GENESIS       anchor genesis section written by anchor-genesis.py
#   LAYERX_PAXEER_GUARANTORS           guarantor set: [{guarantor_id, signer, public_key, bond_controller}]
#   LAYERX_PAXEER_GUARANTOR_KEYS_DIR   directory of <guarantor_id>.controller.key files
#   LAYERX_PAXEER_SETTLEMENT_JSON      settlement document to update
#   LAYERX_PAXEER_SETTLEMENT_DOMAIN    domain name to write (default beta)
#   LAYERX_PAXEER_PROTOCOL_VERSION    settlement protocol version of the domain (default 3)
#   LAYERX_PAXEER_FOUNDRY_BIN          directory holding cast
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
HELPER="$ROOT/platform/hosted/paxeer/settlement-domain.py"
ANCHOR=0x0000000000000000000000000000000000001014
UNIT_WEI=1000000000000
BOUNDARY_URL=${LAYERX_PAXEER_BOUNDARY_URL:-}
BOUNDARY_CA=${LAYERX_PAXEER_BOUNDARY_CA_DER:-}
CHAIN_ID=${LAYERX_PAXEER_CHAIN_ID:-125}
DEPLOYER_KEY_FILE=${LAYERX_PAXEER_DEPLOYER_KEY_FILE:-}
ANCHOR_GENESIS=${LAYERX_PAXEER_ANCHOR_GENESIS:-}
GUARANTORS=${LAYERX_PAXEER_GUARANTORS:-}
KEYS_DIR=${LAYERX_PAXEER_GUARANTOR_KEYS_DIR:-}
SETTLEMENT_JSON=${LAYERX_PAXEER_SETTLEMENT_JSON:-}
DOMAIN=${LAYERX_PAXEER_SETTLEMENT_DOMAIN:-beta}
CAST="${LAYERX_PAXEER_FOUNDRY_BIN:-/root/.foundry/bin}/cast"
PROTOCOL_VERSION=${LAYERX_PAXEER_PROTOCOL_VERSION:-3}
GAS_WEI=${LAYERX_PAXEER_CONTROLLER_GAS_WEI:-1000000000000000000}

fail() {
    echo "anchor-guarantors: $*" >&2
    exit 1
}

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
umask 077

[ -x "$CAST" ] || fail "cast is not installed at $CAST"
for tool in jq openssl python3; do
    command -v "$tool" >/dev/null 2>&1 || fail "$tool is required"
done
case "$BOUNDARY_URL" in
    https://*) ;;
    *) fail "LAYERX_PAXEER_BOUNDARY_URL must be an https URL" ;;
esac
[ -r "$BOUNDARY_CA" ] || fail "LAYERX_PAXEER_BOUNDARY_CA_DER must name the issuing certificate"
openssl x509 -inform DER -in "$BOUNDARY_CA" -out "$WORK/ca.pem" >/dev/null 2>&1 || fail "boundary CA is not a DER certificate"
export SSL_CERT_FILE="$WORK/ca.pem"
[ -r "$DEPLOYER_KEY_FILE" ] || fail "LAYERX_PAXEER_DEPLOYER_KEY_FILE is required"
[ -r "$ANCHOR_GENESIS" ] || fail "LAYERX_PAXEER_ANCHOR_GENESIS is required"
[ -r "$GUARANTORS" ] || fail "LAYERX_PAXEER_GUARANTORS is required"
[ -d "$KEYS_DIR" ] || fail "LAYERX_PAXEER_GUARANTOR_KEYS_DIR is required"
[ -w "$SETTLEMENT_JSON" ] || fail "LAYERX_PAXEER_SETTLEMENT_JSON is required"

DEPLOYER_KEY=$(tr -d '\r\n' < "$DEPLOYER_KEY_FILE")
DEPLOYER=$("$CAST" wallet address --private-key "$DEPLOYER_KEY") || fail "invalid deployer key"
[ "$("$CAST" chain-id --rpc-url "$BOUNDARY_URL")" = "$CHAIN_ID" ] || fail "the boundary does not serve chain $CHAIN_ID"
jq -e --argjson chain "$CHAIN_ID" '.params.paxeer_chain_id == $chain
    and .params.settlement_contract == "0000000000000000000000000000000000001014"' "$ANCHOR_GENESIS" >/dev/null \
    || fail "the anchor genesis names another chain or settlement contract"
MIN_BOND=$(jq -er '.params.min_bond' "$ANCHOR_GENESIS")
THRESHOLD=$(jq -er '.params.threshold' "$ANCHOR_GENESIS")
NETWORK_ID=$(jq -er '.params.network_id' "$ANCHOR_GENESIS")
MAX_DELAY_MS=$(jq -er '.params.max_attestation_delay_ms' "$ANCHOR_GENESIS")
[[ $MIN_BOND =~ ^[1-9][0-9]*$ ]] || fail "the anchor genesis carries no positive minimum bond"
BOND_WEI=$(python3 -c 'import sys; print(int(sys.argv[1]) * int(sys.argv[2]))' "$MIN_BOND" "$UNIT_WEI")
FUNDING_WEI=$(python3 -c 'import sys; print(int(sys.argv[1]) + int(sys.argv[2]))' "$BOND_WEI" "$GAS_WEI")
COUNT=$(jq -er 'length' "$GUARANTORS")
[ "$COUNT" -ge "$THRESHOLD" ] || fail "the guarantor set cannot meet the anchor threshold $THRESHOLD"
[ "$("$CAST" call --rpc-url "$BOUNDARY_URL" "$ANCHOR" 'threshold()(uint32)')" = "$THRESHOLD" ] \
    || fail "the anchor threshold differs from the anchor genesis"

send() {
    local key=$1
    shift
    "$CAST" send --rpc-url "$BOUNDARY_URL" --chain "$CHAIN_ID" --timeout 120 --private-key "$key" --json "$@" > "$WORK/receipt.json"
    [ "$(jq -r '.status' "$WORK/receipt.json")" = "0x1" ] || fail "transaction failed: $(jq -c . "$WORK/receipt.json")"
}

# Prints "signer operator bond status eligible" of guarantor(bytes32), or nothing for an unknown id.
record() {
    "$CAST" call --rpc-url "$BOUNDARY_URL" --json "$ANCHOR" \
        'guarantor(bytes32)(bytes32,address,address,uint256,uint256,uint8,bool)' "$1" \
        | jq -r 'select(.[5] != "0" and .[5] != 0) | [.[1], .[2], (.[3] | tostring), (.[5] | tostring), (.[6] | tostring)] | join(" ")'
}

for index in $(seq 0 $((COUNT - 1))); do
    id=$(jq -er ".[$index].guarantor_id" "$GUARANTORS")
    signer=$(jq -er ".[$index].signer" "$GUARANTORS")
    controller=$(jq -er ".[$index].bond_controller" "$GUARANTORS")
    [ -r "$KEYS_DIR/$id.controller.key" ] || fail "controller key for $id is missing"
    key=$(tr -d '\r\n' < "$KEYS_DIR/$id.controller.key")
    [ "$("$CAST" wallet address --private-key "$key")" = "$("$CAST" to-check-sum-address "$controller")" ] \
        || fail "controller key for $id does not match $controller"
    read -r seen_signer seen_operator seen_bond seen_status seen_eligible <<< "$(record "$id")" || true
    if [ -z "${seen_status:-}" ]; then
        balance=$("$CAST" balance --rpc-url "$BOUNDARY_URL" "$controller")
        if [ "$(printf '%s\n' "$balance" "$FUNDING_WEI" | sort -n | head -1)" != "$FUNDING_WEI" ]; then
            send "$DEPLOYER_KEY" --value "$FUNDING_WEI" "$controller"
        fi
        send "$key" --value "$BOND_WEI" "$ANCHOR" 'registerGuarantor(bytes32,address)' "$id" "$signer"
        read -r seen_signer seen_operator seen_bond seen_status seen_eligible <<< "$(record "$id")"
    fi
    if [ "$seen_status" = 1 ]; then
        send "$DEPLOYER_KEY" "$ANCHOR" 'activateGuarantor(bytes32)' "$id"
        read -r seen_signer seen_operator seen_bond seen_status seen_eligible <<< "$(record "$id")"
    fi
    [ "${seen_signer,,}" = "${signer,,}" ] && [ "${seen_operator,,}" = "${controller,,}" ] \
        || fail "anchor guarantor $id is registered with another signer or operator"
    [ "$seen_status" = 2 ] && [ "$seen_eligible" = true ] \
        && [ "$(printf '%s\n' "$seen_bond" "$MIN_BOND" | sort -n | head -1)" = "$MIN_BOND" ] \
        || fail "anchor guarantor $id is not active and eligible with the minimum bond"
done

jq -c --arg protocol "$PROTOCOL_VERSION" --arg chain "$CHAIN_ID" --arg network "$NETWORK_ID" --arg anchor "$ANCHOR" --arg bond "$MIN_BOND" --arg delay "$MAX_DELAY_MS" '{
    protocol_version: ($protocol | tonumber), paxeer_chain_id: ($chain | tonumber), network_id: ($network | tonumber),
    settlement_contract: $anchor, guarantor_bond: $anchor, minimum_bond: ($bond | tonumber),
    maximum_attestation_delay_ms: ($delay | tonumber), guarantor_set: map({guarantor_id, signer, public_key})
}' "$GUARANTORS" | python3 "$HELPER" write "$SETTLEMENT_JSON" "$DOMAIN"
echo "anchor-guarantors: $COUNT guarantors active in the anchor module under authority $DEPLOYER; $DOMAIN domain written to $SETTLEMENT_JSON" >&2
