#!/usr/bin/env bash
# Initialises a single-validator Paxeer chain for the LayerX beta.
#
# The cosmos chain id hyperpax_125-1 is the only identifier paxd maps to the EVM chain id 125
# (modules/evm/config/config.go ChainIDMapping), so the EVM chain id is fixed by
# that mapping and the script refuses any other value. Genesis funds the deployer's cast
# address, seeds the beta USDL token code at the address the contracts pin
# (contracts/libraries/Constants.sol USDL_TOKEN) with the deployer as its owner, and binds the
# Tendermint, gRPC and API listeners to loopback. The EVM JSON-RPC listener is served by paxd on
# port ${LAYERX_PAXEER_EVM_PORT} and is reached only through the boundary container.
#
# LayerX custody is the native layerxcustody module behind the precompile at
# 0x0000000000000000000000000000000000001013; no custody contract is deployed. Its network id,
# sequencer authorization, payout delays and asset map are genesis state:
# LAYERX_PAXEER_CUSTODY_GENESIS_FILE names the section written by custody-genesis.py, and
# paxd validate-genesis runs the module's own validation over it. Without the file the module
# keeps its default genesis, which maps no asset and therefore accepts no deposit.
#
# That section must also carry the deposit-root authority: the Ed25519 public key the guarantor's
# checkpoint authority signs every deposit-root registration with. Nothing sets it after genesis in
# this bring-up, and the module refuses registerDepositRoot with "no deposit root authority" while
# it is empty, so a custody genesis without one is refused here rather than on the first deposit.
# LAYERX_PAXEER_DEPOSIT_ROOT_AUTHORITY (or ..._FILE) names that public key as 0x and 64 lowercase
# hexadecimal characters, and the custody genesis section must carry exactly it.
#
# Checkpoint settlement and the guarantor bond are the native layerxanchor module behind the
# precompile at 0x0000000000000000000000000000000000001014; no checkpoint or bond contract is
# deployed. LAYERX_PAXEER_ANCHOR_GENESIS_FILE names the section written by anchor-genesis.py. Its
# authority must be the deployer's cast account and its paxeer_chain_id this chain's EVM chain id,
# the two values guarantors and the bring-up depend on. Without the file the module keeps its
# default genesis, which authorizes no sequencer and therefore accepts no checkpoint.
#
# The guarantor submits every checkpoint to that precompile from its own account, which pays its
# own gas and is never the deployer. LAYERX_PAXEER_CHECKPOINT_SUBMITTER_ADDRESS (or
# ..._ADDRESS_FILE) names it, and genesis funds its cast account with
# LAYERX_PAXEER_CHECKPOINT_SUBMITTER_FUNDING and binds the association the same way the deployer's
# is bound, so submitCheckpoint works from the first batch without a manual transfer.
set -euo pipefail

PAXD=${PAXD:-paxd}
JQ=${JQ:-jq}
HOME_DIR=${LAYERX_PAXEER_HOME:-/var/lib/paxeer}
CHAIN_ID=${LAYERX_PAXEER_CHAIN_ID:-125}
MONIKER=${LAYERX_PAXEER_MONIKER:-paxeer-beta}
VALIDATOR_KEY=${LAYERX_PAXEER_VALIDATOR_KEY_NAME:-validator}
VALIDATOR_FUNDING=${LAYERX_PAXEER_VALIDATOR_FUNDING:-100000000000000000000uhpx}
VALIDATOR_STAKE=${LAYERX_PAXEER_VALIDATOR_STAKE:-7000000000000000uhpx}
VALIDATOR_POWER=${LAYERX_PAXEER_VALIDATOR_POWER:-7000000000}
DEPLOYER_FUNDING=${LAYERX_PAXEER_DEPLOYER_FUNDING:-1000000000000000000000000uhpx}
# Gas only: one uhpx is 1e12 wei, so this is 1e24 wei for the guarantor's checkpoint submissions.
SUBMITTER_FUNDING=${LAYERX_PAXEER_CHECKPOINT_SUBMITTER_FUNDING:-1000000000000uhpx}
USDL_ADDRESS=0x85FcD13735F4309833A503EE804ea32395851479
SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
USDL_RUNTIME=${LAYERX_PAXEER_USDL_RUNTIME:-$SCRIPT_DIR/contracts/BetaUsdl.runtime.hex}
EVM_PORT=${LAYERX_PAXEER_EVM_PORT:-8545}
EVM_WS_PORT=${LAYERX_PAXEER_EVM_WS_PORT:-8546}
RPC_PORT=${LAYERX_PAXEER_RPC_PORT:-26657}
P2P_PORT=${LAYERX_PAXEER_P2P_PORT:-26656}
GRPC_PORT=${LAYERX_PAXEER_GRPC_PORT:-9090}
GRPC_WEB_PORT=${LAYERX_PAXEER_GRPC_WEB_PORT:-9091}
COMMIT_TIMEOUT_NANOSECONDS=${LAYERX_PAXEER_COMMIT_TIMEOUT_NANOSECONDS:-}
CUSTODY_GENESIS=${LAYERX_PAXEER_CUSTODY_GENESIS_FILE:-}
ANCHOR_GENESIS=${LAYERX_PAXEER_ANCHOR_GENESIS_FILE:-}
DEPOSIT_ROOT_AUTHORITY=${LAYERX_PAXEER_DEPOSIT_ROOT_AUTHORITY:-}
MARKER="$HOME_DIR/config/.layerx-beta-initialised"

fail() {
    echo "init-chain: $*" >&2
    exit 1
}

if [ "$CHAIN_ID" != "125" ]; then
    fail "paxd derives EVM chain id 125 only from hyperpax_125-1; LAYERX_PAXEER_CHAIN_ID=$CHAIN_ID is not mapped"
fi
COSMOS_CHAIN_ID="hyperpax_${CHAIN_ID}-1"

if [ -n "$COMMIT_TIMEOUT_NANOSECONDS" ]; then
    [[ "$COMMIT_TIMEOUT_NANOSECONDS" =~ ^[1-9][0-9]{0,10}$ ]] \
        && [ "$COMMIT_TIMEOUT_NANOSECONDS" -le 60000000000 ] \
        || fail "commit timeout must be canonical nanoseconds from 1 through 60000000000"
fi

if [ -n "${LAYERX_PAXEER_DEPLOYER_ADDRESS_FILE:-}" ]; then
    DEPLOYER_ADDRESS=$(tr -d '\r\n' < "$LAYERX_PAXEER_DEPLOYER_ADDRESS_FILE")
else
    DEPLOYER_ADDRESS=${LAYERX_PAXEER_DEPLOYER_ADDRESS:-}
fi
case "$DEPLOYER_ADDRESS" in
    0x[0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F]) ;;
    *) fail "LAYERX_PAXEER_DEPLOYER_ADDRESS must be a 0x-prefixed 20-byte EVM address" ;;
esac
DEPLOYER_HEX=$(printf '%s' "${DEPLOYER_ADDRESS#0x}" | tr 'A-F' 'a-f')

if [ -n "${LAYERX_PAXEER_CHECKPOINT_SUBMITTER_ADDRESS_FILE:-}" ]; then
    [ -r "$LAYERX_PAXEER_CHECKPOINT_SUBMITTER_ADDRESS_FILE" ] \
        || fail "checkpoint submitter address $LAYERX_PAXEER_CHECKPOINT_SUBMITTER_ADDRESS_FILE is not readable"
    SUBMITTER_ADDRESS=$(tr -d '\r\n' < "$LAYERX_PAXEER_CHECKPOINT_SUBMITTER_ADDRESS_FILE")
else
    SUBMITTER_ADDRESS=${LAYERX_PAXEER_CHECKPOINT_SUBMITTER_ADDRESS:-}
fi
SUBMITTER_HEX=""
if [ -n "$SUBMITTER_ADDRESS" ]; then
    case "$SUBMITTER_ADDRESS" in
        0x[0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F][0-9a-fA-F]) ;;
        *) fail "LAYERX_PAXEER_CHECKPOINT_SUBMITTER_ADDRESS must be a 0x-prefixed 20-byte EVM address" ;;
    esac
    SUBMITTER_HEX=$(printf '%s' "${SUBMITTER_ADDRESS#0x}" | tr 'A-F' 'a-f')
    [ "$SUBMITTER_HEX" != "$DEPLOYER_HEX" ] \
        || fail "the checkpoint submitter must differ from the deployer"
    [[ "$SUBMITTER_FUNDING" =~ ^[1-9][0-9]*uhpx$ ]] \
        || fail "LAYERX_PAXEER_CHECKPOINT_SUBMITTER_FUNDING must be a positive uhpx amount"
fi

if [ -n "${LAYERX_PAXEER_DEPOSIT_ROOT_AUTHORITY_FILE:-}" ]; then
    [ -r "$LAYERX_PAXEER_DEPOSIT_ROOT_AUTHORITY_FILE" ] \
        || fail "deposit-root authority $LAYERX_PAXEER_DEPOSIT_ROOT_AUTHORITY_FILE is not readable"
    DEPOSIT_ROOT_AUTHORITY=$(tr -d '[:space:]' < "$LAYERX_PAXEER_DEPOSIT_ROOT_AUTHORITY_FILE")
fi
if [ -n "$DEPOSIT_ROOT_AUTHORITY" ]; then
    DEPOSIT_ROOT_AUTHORITY=$(printf '%s' "${DEPOSIT_ROOT_AUTHORITY#0x}" | tr 'A-F' 'a-f')
    [[ "$DEPOSIT_ROOT_AUTHORITY" =~ ^[0-9a-f]{64}$ ]] \
        || fail "the deposit-root authority must be a 32-byte Ed25519 public key"
    [ "$DEPOSIT_ROOT_AUTHORITY" != "$(printf '%064d' 0)" ] \
        || fail "the deposit-root authority must be nonzero"
    [ -n "$CUSTODY_GENESIS" ] \
        || fail "a deposit-root authority was given without LAYERX_PAXEER_CUSTODY_GENESIS_FILE"
fi

command -v "$PAXD" >/dev/null 2>&1 || fail "paxd binary $PAXD is not available"
command -v "$JQ" >/dev/null 2>&1 || fail "jq is not available"
[ -r "$USDL_RUNTIME" ] || fail "USDL runtime bytecode $USDL_RUNTIME is not readable"
if [ -n "$CUSTODY_GENESIS" ]; then
    [ -r "$CUSTODY_GENESIS" ] || fail "custody genesis $CUSTODY_GENESIS is not readable"
    "$JQ" -e '(.params.network_id | type == "number" and . > 0)
        and (.params.sequencer_authorizations | type == "array" and length > 0)
        and (.assets | type == "array" and length > 0)' "$CUSTODY_GENESIS" >/dev/null \
        || fail "custody genesis must carry a network id, a sequencer authorization and an asset"
    # Nothing sets this parameter after genesis, and the module refuses every deposit-root
    # registration while it is empty, so an absent authority is a genesis failure.
    CUSTODY_AUTHORITY=$("$JQ" -r '.params.deposit_root_authority // ""' "$CUSTODY_GENESIS")
    [[ "$CUSTODY_AUTHORITY" =~ ^[0-9a-f]{64}$ ]] && [ "$CUSTODY_AUTHORITY" != "$(printf '%064d' 0)" ] \
        || fail "custody genesis must carry a nonzero 32-byte deposit_root_authority; the guarantor checkpoint authority public key is required before genesis"
    if [ -n "$DEPOSIT_ROOT_AUTHORITY" ]; then
        [ "$CUSTODY_AUTHORITY" = "$DEPOSIT_ROOT_AUTHORITY" ] \
            || fail "custody genesis deposit_root_authority $CUSTODY_AUTHORITY differs from the requested $DEPOSIT_ROOT_AUTHORITY"
    else
        DEPOSIT_ROOT_AUTHORITY=$CUSTODY_AUTHORITY
    fi
fi
if [ -n "$ANCHOR_GENESIS" ]; then
    [ -r "$ANCHOR_GENESIS" ] || fail "anchor genesis $ANCHOR_GENESIS is not readable"
    "$JQ" -e --argjson chain "$CHAIN_ID" '(.params.network_id | type == "number" and . > 0)
        and .params.paxeer_chain_id == $chain
        and .params.settlement_contract == "0000000000000000000000000000000000001014"
        and (.params.threshold | type == "number" and . > 0)
        and (.sequencers | type == "array" and length > 0)' "$ANCHOR_GENESIS" >/dev/null \
        || fail "anchor genesis must carry a network id, this EVM chain id, the anchor address, a threshold and a sequencer authorization"
fi

if [ -f "$MARKER" ]; then
    if [ -n "$CUSTODY_GENESIS" ]; then
        "$JQ" -e --slurpfile custody "$CUSTODY_GENESIS" \
            '.app_state.layerxcustody.params.network_id == $custody[0].params.network_id
             and .app_state.layerxcustody.params.deposit_root_authority == $custody[0].params.deposit_root_authority
             and ([.app_state.layerxcustody.assets[].asset_id] == [$custody[0].assets[].asset_id])' \
            "$HOME_DIR/config/genesis.json" >/dev/null \
            || fail "requested custody genesis differs from initialised genesis"
    fi
    if [ -n "$DEPOSIT_ROOT_AUTHORITY" ]; then
        "$JQ" -e --arg authority "$DEPOSIT_ROOT_AUTHORITY" \
            '.app_state.layerxcustody.params.deposit_root_authority == $authority' \
            "$HOME_DIR/config/genesis.json" >/dev/null \
            || fail "requested deposit-root authority $DEPOSIT_ROOT_AUTHORITY is not the one in the initialised genesis"
    fi
    if [ -n "$ANCHOR_GENESIS" ]; then
        "$JQ" -e --slurpfile anchor "$ANCHOR_GENESIS" \
            '.app_state.layerxanchor.params == $anchor[0].params
             and .app_state.layerxanchor.sequencers == $anchor[0].sequencers' \
            "$HOME_DIR/config/genesis.json" >/dev/null \
            || fail "requested anchor genesis differs from initialised genesis"
    fi
    if [ -n "$COMMIT_TIMEOUT_NANOSECONDS" ]; then
        "$JQ" -e --arg commit_timeout "$COMMIT_TIMEOUT_NANOSECONDS" \
            '.consensus_params.timeout.commit == $commit_timeout' \
            "$HOME_DIR/config/genesis.json" >/dev/null \
            || fail "requested commit timeout differs from initialised genesis"
    fi
    if [ -n "$SUBMITTER_HEX" ]; then
        "$JQ" -e --arg submitter "$SUBMITTER_ADDRESS" \
            'any(.app_state.evm.address_associations[]?; (.eth_address | ascii_downcase) == ($submitter | ascii_downcase))' \
            "$HOME_DIR/config/genesis.json" >/dev/null \
            || fail "requested checkpoint submitter $SUBMITTER_ADDRESS is not funded in the initialised genesis"
    fi
    echo "init-chain: $HOME_DIR already initialised for $COSMOS_CHAIN_ID" >&2
    exit 0
fi
if [ -e "$HOME_DIR/config/genesis.json" ]; then
    fail "$HOME_DIR holds a partial initialisation; remove it before re-running"
fi

hex_to_base64() {
    local hex=$1 escaped
    escaped=$(printf '%s' "$hex" | sed 's/../\\x&/g')
    # shellcheck disable=SC2059
    printf "$escaped" | base64 | tr -d '\n'
}

USDL_HEX=$(tr -d '\r\n' < "$USDL_RUNTIME")
USDL_HEX=${USDL_HEX#0x}
case "$USDL_HEX" in
    *[!0-9a-fA-F]*|"") fail "USDL runtime bytecode is not hex" ;;
esac
USDL_CODE_B64=$(hex_to_base64 "$USDL_HEX")
ZERO_SLOT_B64=$(hex_to_base64 "0000000000000000000000000000000000000000000000000000000000000000")
OWNER_WORD_B64=$(hex_to_base64 "000000000000000000000000${DEPLOYER_HEX}")

mkdir -p "$HOME_DIR"
"$PAXD" init "$MONIKER" --chain-id "$COSMOS_CHAIN_ID" --home "$HOME_DIR" --overwrite >/dev/null 2>&1
"$PAXD" keys add "$VALIDATOR_KEY" --keyring-backend test --home "$HOME_DIR" --output json >/dev/null 2>&1
"$PAXD" add-genesis-account "$VALIDATOR_KEY" "$VALIDATOR_FUNDING" --keyring-backend test --home "$HOME_DIR"
DEPLOYER_CAST=$("$PAXD" debug addr "$DEPLOYER_HEX" --home "$HOME_DIR" 2>/dev/null | sed -n 's/^Bech32 Acc: //p')
case "$DEPLOYER_CAST" in
    pax1*) ;;
    *) fail "paxd could not derive the deployer cast address" ;;
esac
"$PAXD" add-genesis-account "$DEPLOYER_CAST" "$DEPLOYER_FUNDING" --home "$HOME_DIR"
SUBMITTER_CAST=""
if [ -n "$SUBMITTER_HEX" ]; then
    SUBMITTER_CAST=$("$PAXD" debug addr "$SUBMITTER_HEX" --home "$HOME_DIR" 2>/dev/null | sed -n 's/^Bech32 Acc: //p')
    case "$SUBMITTER_CAST" in
        pax1*) ;;
        *) fail "paxd could not derive the checkpoint submitter cast address" ;;
    esac
    [ "$SUBMITTER_CAST" != "$DEPLOYER_CAST" ] || fail "the checkpoint submitter must differ from the deployer"
    "$PAXD" add-genesis-account "$SUBMITTER_CAST" "$SUBMITTER_FUNDING" --home "$HOME_DIR"
fi
"$PAXD" gentx "$VALIDATOR_KEY" "$VALIDATOR_STAKE" --chain-id "$COSMOS_CHAIN_ID" --keyring-backend test \
    --home "$HOME_DIR" --moniker "$MONIKER" --ip 127.0.0.1 --p2p-port "$P2P_PORT" >/dev/null 2>&1

GENESIS="$HOME_DIR/config/genesis.json"
VALIDATOR_PUBKEY=$("$JQ" -c '.pub_key' "$HOME_DIR/config/priv_validator_key.json")
"$JQ" --argjson key "$VALIDATOR_PUBKEY" --arg power "$VALIDATOR_POWER" \
    --arg usdl "$USDL_ADDRESS" --arg code "$USDL_CODE_B64" --arg slot "$ZERO_SLOT_B64" --arg owner "$OWNER_WORD_B64" \
    --arg deployer "$DEPLOYER_ADDRESS" --arg deployer_cast "$DEPLOYER_CAST" \
    --arg commit_timeout "$COMMIT_TIMEOUT_NANOSECONDS" '
    .validators = [{"power": $power, "pub_key": $key}]
    | .app_state.staking.params.max_voting_power_ratio = "1.000000000000000000"
    | .app_state.evm.codes = [{"address": $usdl, "code": $code}]
    | .app_state.evm.states = [{"address": $usdl, "key": $slot, "value": $owner}]
    | .app_state.evm.address_associations = ((.app_state.evm.address_associations // [])
        | map(select(.eth_address != $deployer)) + [{"eth_address": $deployer, "pax_address": $deployer_cast}])
    | .consensus_params.block.max_gas = "35000000"
    | if $commit_timeout == "" then . else .consensus_params.timeout.commit = $commit_timeout end
    | .app_state.bank.denom_metadata = [{"denom_units": [{"denom": "uhpx", "exponent": 0, "aliases": ["UHPX"]}],
        "base": "uhpx", "display": "uhpx", "name": "UHPX", "symbol": "UHPX"}]
' "$GENESIS" > "$GENESIS.tmp"
mv "$GENESIS.tmp" "$GENESIS"
if [ -n "$SUBMITTER_HEX" ]; then
    "$JQ" --arg submitter "$SUBMITTER_ADDRESS" --arg submitter_cast "$SUBMITTER_CAST" '
        .app_state.evm.address_associations = ((.app_state.evm.address_associations // [])
            | map(select(.eth_address != $submitter)) + [{"eth_address": $submitter, "pax_address": $submitter_cast}])
    ' "$GENESIS" > "$GENESIS.tmp"
    mv "$GENESIS.tmp" "$GENESIS"
fi
if [ -n "$CUSTODY_GENESIS" ]; then
    "$JQ" --slurpfile custody "$CUSTODY_GENESIS" '.app_state.layerxcustody = $custody[0]' "$GENESIS" > "$GENESIS.tmp"
    mv "$GENESIS.tmp" "$GENESIS"
    "$JQ" -e --arg authority "$DEPOSIT_ROOT_AUTHORITY" \
        '.app_state.layerxcustody.params.deposit_root_authority == $authority' "$GENESIS" >/dev/null \
        || fail "genesis does not carry the deposit-root authority $DEPOSIT_ROOT_AUTHORITY"
fi
if [ -n "$ANCHOR_GENESIS" ]; then
    "$JQ" -e --arg authority "$DEPLOYER_CAST" '.params.authority == $authority' "$ANCHOR_GENESIS" >/dev/null \
        || fail "anchor genesis authority is not the deployer cast account $DEPLOYER_CAST"
    "$JQ" --slurpfile anchor "$ANCHOR_GENESIS" '.app_state.layerxanchor = $anchor[0]' "$GENESIS" > "$GENESIS.tmp"
    mv "$GENESIS.tmp" "$GENESIS"
fi
if [ -n "$ANCHOR_GENESIS" ] && "$JQ" -e '(.guarantors // []) | length > 0' "$ANCHOR_GENESIS" >/dev/null; then
    # layerxanchor refuses to initialise unless its module account holds exactly the bonds its genesis
    # records, so a genesis guarantor set brings its escrow with it.
    "$JQ" -e '[.guarantors[].bond | tonumber] | all(. > 0 and . < 9007199254740992)' "$ANCHOR_GENESIS" >/dev/null \
        || fail "anchor genesis bond is outside the range this script can total"
    ESCROW_TOTAL=$("$JQ" -r '[.guarantors[].bond | tonumber] | add' "$ANCHOR_GENESIS")
    ESCROW_DENOM=$("$JQ" -r '.params.bond_denom' "$ANCHOR_GENESIS")
    ESCROW_HEX=$(printf 'layerxanchor' | sha256sum | cut -c 1-40)
    ESCROW_ACCOUNT=$("$PAXD" debug addr "$ESCROW_HEX" --home "$HOME_DIR" 2>/dev/null | sed -n 's/^Bech32 Acc: //p')
    case "$ESCROW_ACCOUNT" in
        pax1*) ;;
        *) fail "paxd could not derive the anchor module account" ;;
    esac
    "$JQ" --arg address "$ESCROW_ACCOUNT" --arg denom "$ESCROW_DENOM" --arg amount "$ESCROW_TOTAL" '
        if any(.app_state.bank.balances[]; .address == $address) then error("anchor module account already funded") else . end
        | .app_state.bank.balances = (.app_state.bank.balances + [{"address": $address, "coins": [{"denom": $denom, "amount": $amount}]}]
            | sort_by(.address))
    ' "$GENESIS" > "$GENESIS.tmp"
    mv "$GENESIS.tmp" "$GENESIS"
fi
"$PAXD" collect-gentxs --home "$HOME_DIR" >/dev/null 2>&1
"$PAXD" validate-genesis --home "$HOME_DIR" >/dev/null

CONFIG="$HOME_DIR/config/config.toml"
APP="$HOME_DIR/config/app.toml"
sed -i "s/^mode = .*/mode = \"validator\"/" "$CONFIG"
sed -i "/^\[rpc\]/,/^\[/ s|^laddr = .*|laddr = \"tcp://127.0.0.1:${RPC_PORT}\"|" "$CONFIG"
sed -i "/^\[p2p\]/,/^\[/ s|^laddr = .*|laddr = \"tcp://127.0.0.1:${P2P_PORT}\"|" "$CONFIG"
sed -i "/^\[p2p\]/,/^\[/ s|^external-address = .*|external-address = \"\"|" "$CONFIG"
sed -i "/^\[p2p\]/,/^\[/ s|^pex = .*|pex = false|" "$CONFIG"
sed -i "/^\[api\]/,/^\[/ s|^enable = .*|enable = false|" "$APP"
sed -i "/^\[grpc\]/,/^\[/ s|^address = .*|address = \"127.0.0.1:${GRPC_PORT}\"|" "$APP"
sed -i "/^\[grpc-web\]/,/^\[/ s|^enable = .*|enable = false|" "$APP"
sed -i "/^\[grpc-web\]/,/^\[/ s|^address = .*|address = \"127.0.0.1:${GRPC_WEB_PORT}\"|" "$APP"
sed -i "/^\[evm\]/,/^\[/ s|^http_enabled = .*|http_enabled = true|" "$APP"
sed -i "/^\[evm\]/,/^\[/ s|^http_port = .*|http_port = ${EVM_PORT}|" "$APP"
sed -i "/^\[evm\]/,/^\[/ s|^http_address = .*|http_address = \"127.0.0.1\"|" "$APP"
sed -i "/^\[evm\]/,/^\[/ s|^ws_enabled = .*|ws_enabled = false|" "$APP"
sed -i "/^\[evm\]/,/^\[/ s|^ws_port = .*|ws_port = ${EVM_WS_PORT}|" "$APP"
sed -i "/^\[evm\]/,/^\[/ s|^enable_test_api = .*|enable_test_api = false|" "$APP"
sed -i "s|^minimum-gas-prices = .*|minimum-gas-prices = \"0.01uhpx\"|" "$APP"

grep -q '^http_address = "127.0.0.1"' "$APP" || fail "paxd lacks the loopback RPC bind setting; rebuild the current source"
grep -q '^mode = "validator"' "$CONFIG" || fail "config.toml mode was not set"
grep -q "^http_port = ${EVM_PORT}$" "$APP" || fail "app.toml evm http_port was not set"
"$JQ" -e --arg usdl "$USDL_ADDRESS" '.app_state.evm.codes[0].address == $usdl and (.validators | length) == 1' "$GENESIS" >/dev/null \
    || fail "genesis does not carry the USDL code and the validator"
if [ -n "$SUBMITTER_HEX" ]; then
    "$JQ" -e --arg address "$SUBMITTER_CAST" --arg amount "${SUBMITTER_FUNDING%uhpx}" \
        'any(.app_state.bank.balances[]; .address == $address
            and any(.coins[]; .denom == "uhpx" and .amount == $amount))' "$GENESIS" >/dev/null \
        || fail "genesis does not fund the checkpoint submitter $SUBMITTER_ADDRESS"
fi

{
    printf 'cosmos_chain_id=%s\n' "$COSMOS_CHAIN_ID"
    printf 'evm_chain_id=%s\n' "$CHAIN_ID"
    printf 'deployer=%s\n' "$DEPLOYER_ADDRESS"
    printf 'deployer_cast=%s\n' "$DEPLOYER_CAST"
    printf 'usdl=%s\n' "$USDL_ADDRESS"
    printf 'custody=0x0000000000000000000000000000000000001013\n'
    if [ -n "$DEPOSIT_ROOT_AUTHORITY" ]; then
        printf 'deposit_root_authority=0x%s\n' "$DEPOSIT_ROOT_AUTHORITY"
    fi
    printf 'anchor=0x0000000000000000000000000000000000001014\n'
    printf 'evm_port=%s\n' "$EVM_PORT"
    if [ -n "$SUBMITTER_HEX" ]; then
        printf 'checkpoint_submitter=%s\n' "$SUBMITTER_ADDRESS"
        printf 'checkpoint_submitter_cast=%s\n' "$SUBMITTER_CAST"
    fi
} > "$MARKER"
echo "init-chain: initialised $COSMOS_CHAIN_ID at $HOME_DIR (deployer $DEPLOYER_ADDRESS, cast $DEPLOYER_CAST)" >&2
