#!/usr/bin/env bash
# Initialises a single-validator Paxeer chain for the LayerX beta.
#
# The cosmos chain id hyperpax_125-1 is the only identifier paxd maps to the EVM chain id 125
# (paxeer-network/modules/evm/config/config.go ChainIDMapping), so the EVM chain id is fixed by
# that mapping and the script refuses any other value. Genesis funds the deployer's cast
# address, seeds the beta USDL token code at the address the contracts pin
# (contracts/libraries/Constants.sol USDL_TOKEN) with the deployer as its owner, and binds the
# Tendermint, gRPC and API listeners to loopback. The EVM JSON-RPC listener is served by paxd on
# port ${LAYERX_PAXEER_EVM_PORT} and is reached only through the boundary container.
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
USDL_ADDRESS=0x85FcD13735F4309833A503EE804ea32395851479
SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
USDL_RUNTIME=${LAYERX_PAXEER_USDL_RUNTIME:-$SCRIPT_DIR/contracts/BetaUsdl.runtime.hex}
EVM_PORT=${LAYERX_PAXEER_EVM_PORT:-8545}
EVM_WS_PORT=${LAYERX_PAXEER_EVM_WS_PORT:-8546}
RPC_PORT=${LAYERX_PAXEER_RPC_PORT:-26657}
P2P_PORT=${LAYERX_PAXEER_P2P_PORT:-26656}
GRPC_PORT=${LAYERX_PAXEER_GRPC_PORT:-9090}
GRPC_WEB_PORT=${LAYERX_PAXEER_GRPC_WEB_PORT:-9091}
MARKER="$HOME_DIR/config/.layerx-beta-initialised"

fail() {
    echo "init-chain: $*" >&2
    exit 1
}

if [ "$CHAIN_ID" != "125" ]; then
    fail "paxd derives EVM chain id 125 only from hyperpax_125-1; LAYERX_PAXEER_CHAIN_ID=$CHAIN_ID is not mapped"
fi
COSMOS_CHAIN_ID="hyperpax_${CHAIN_ID}-1"

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

command -v "$PAXD" >/dev/null 2>&1 || fail "paxd binary $PAXD is not available"
command -v "$JQ" >/dev/null 2>&1 || fail "jq is not available"
[ -r "$USDL_RUNTIME" ] || fail "USDL runtime bytecode $USDL_RUNTIME is not readable"

if [ -f "$MARKER" ]; then
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
"$PAXD" gentx "$VALIDATOR_KEY" "$VALIDATOR_STAKE" --chain-id "$COSMOS_CHAIN_ID" --keyring-backend test \
    --home "$HOME_DIR" --moniker "$MONIKER" --ip 127.0.0.1 --p2p-port "$P2P_PORT" >/dev/null 2>&1

GENESIS="$HOME_DIR/config/genesis.json"
VALIDATOR_PUBKEY=$("$JQ" -c '.pub_key' "$HOME_DIR/config/priv_validator_key.json")
"$JQ" --argjson key "$VALIDATOR_PUBKEY" --arg power "$VALIDATOR_POWER" \
    --arg usdl "$USDL_ADDRESS" --arg code "$USDL_CODE_B64" --arg slot "$ZERO_SLOT_B64" --arg owner "$OWNER_WORD_B64" \
    --arg deployer "$DEPLOYER_ADDRESS" --arg deployer_cast "$DEPLOYER_CAST" '
    .validators = [{"power": $power, "pub_key": $key}]
    | .app_state.staking.params.max_voting_power_ratio = "1.000000000000000000"
    | .app_state.evm.codes = [{"address": $usdl, "code": $code}]
    | .app_state.evm.states = [{"address": $usdl, "key": $slot, "value": $owner}]
    | .app_state.evm.address_associations = ((.app_state.evm.address_associations // [])
        | map(select(.eth_address != $deployer)) + [{"eth_address": $deployer, "pax_address": $deployer_cast}])
    | .consensus_params.block.max_gas = "35000000"
    | .app_state.bank.denom_metadata = [{"denom_units": [{"denom": "uhpx", "exponent": 0, "aliases": ["UHPX"]}],
        "base": "uhpx", "display": "uhpx", "name": "UHPX", "symbol": "UHPX"}]
' "$GENESIS" > "$GENESIS.tmp"
mv "$GENESIS.tmp" "$GENESIS"
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

{
    printf 'cosmos_chain_id=%s\n' "$COSMOS_CHAIN_ID"
    printf 'evm_chain_id=%s\n' "$CHAIN_ID"
    printf 'deployer=%s\n' "$DEPLOYER_ADDRESS"
    printf 'deployer_cast=%s\n' "$DEPLOYER_CAST"
    printf 'usdl=%s\n' "$USDL_ADDRESS"
    printf 'evm_port=%s\n' "$EVM_PORT"
} > "$MARKER"
echo "init-chain: initialised $COSMOS_CHAIN_ID at $HOME_DIR (deployer $DEPLOYER_ADDRESS, cast $DEPLOYER_CAST)" >&2
