#!/usr/bin/env sh

# Input parameters
NODE_ID=${ID:-0}
NUM_ACCOUNTS=${NUM_ACCOUNTS:-5}
echo "Configure and initialize environment"

cp build/paxd "$GOBIN"/

# Prepare shared folders
NODE_DIR="build/generated/node_${NODE_ID}"
mkdir -p build/generated/gentx/
mkdir -p build/generated/exported_keys/
mkdir -p "$NODE_DIR"

# Testing whether paxd works or not
paxd version # Uncomment the below line if there are any dependency issues
# ldd build/paxd

# Initialize validator node
MONIKER="pax-node-$NODE_ID"

paxd init "$MONIKER" --chain-id pax >/dev/null 2>&1

# Copy configs
APP_CONFIG_FILE="$NODE_DIR/app.toml"
TENDERMINT_CONFIG_FILE="$NODE_DIR/config.toml"
cp docker/localnode/config/app.toml "$APP_CONFIG_FILE"
cp docker/localnode/config/config.toml "$TENDERMINT_CONFIG_FILE"


# Set up persistent peers
PAX_NODE_ID=$(paxd tendermint show-node-id)
NODE_IP=$(hostname -i | awk '{print $1}')
P2P_PORT=26656  # Must match [p2p] laddr in config.toml
EVMRPC_PORT=8545  # Must match the EVM RPC HTTP port (evmrpc DefaultConfig HTTPPort).
echo "$PAX_NODE_ID@$NODE_IP:$P2P_PORT" >> build/generated/persistent_peers.txt

# Store autobahn-compatible pubkeys and address for config generation
cp ~/.pax/config/validator_pubkey.txt "$NODE_DIR/" || { echo "ERROR: failed to copy validator_pubkey.txt"; exit 1; }
cp ~/.pax/config/node_pubkey.txt "$NODE_DIR/" || { echo "ERROR: failed to copy node_pubkey.txt"; exit 1; }
echo "$NODE_IP:$P2P_PORT" > "$NODE_DIR/autobahn_address.txt"
echo "http://$NODE_IP:$EVMRPC_PORT" > "$NODE_DIR/evmrpc_url.txt"

# Create a new account
ACCOUNT_NAME="node_admin"
echo "Adding account $ACCOUNT_NAME"
printf "12345678\n12345678\ny\n" | paxd keys add "$ACCOUNT_NAME" >/dev/null 2>&1

# Get genesis account info
GENESIS_ACCOUNT_ADDRESS=$(printf "12345678\n" | paxd keys show "$ACCOUNT_NAME" -a)
echo "$GENESIS_ACCOUNT_ADDRESS" >> build/generated/genesis_accounts.txt

# Add funds to genesis account
paxd add-genesis-account "$GENESIS_ACCOUNT_ADDRESS" 10000000uhpx,10000000uusdc,10000000uatom

# Create gentx
printf "12345678\n" | paxd gentx "$ACCOUNT_NAME" 10000000uhpx --chain-id pax
cp ~/.pax/config/gentx/* build/generated/gentx/

# Creating some testing accounts
echo "Creating $NUM_ACCOUNTS accounts"
python3 loadtest/scripts/populate_genesis_accounts.py "$NUM_ACCOUNTS" loc >/dev/null 2>&1
echo "Finished $NUM_ACCOUNTS accounts creation"

# Set node paxvaloper info
PAXVALOPER_INFO=$(printf "12345678\n" | paxd keys show "$ACCOUNT_NAME" --bech=val -a)
PRIV_KEY=$(printf "12345678\n12345678\n" | paxd keys export "$ACCOUNT_NAME")
echo "$PRIV_KEY" >> build/generated/exported_keys/"$PAXVALOPER_INFO".txt

echo "DONE" >> build/generated/init.complete
