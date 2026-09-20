#!/usr/bin/env sh

# Set up GO PATH
echo "Configure and initialize environment"

# Testing whether paxd works or not
paxd version # Uncomment the below line if there are any dependency issues
# ldd build/paxd

# Initialize validator node
MONIKER="pax-rpc-node"
paxd init --chain-id pax "$MONIKER"

# Copy configs
cp docker/rpcnode/config/app.toml ~/.pax/config/app.toml
cp docker/rpcnode/config/config.toml ~/.pax/config/config.toml
cp build/generated/genesis.json ~/.pax/config/genesis.json

# Apply Giga Storage overrides so the RPC node's app hash matches the validators.
GIGA_STORAGE=${GIGA_STORAGE:-false}
GIGA_FLATKV_ONLY=${GIGA_FLATKV_ONLY:-false}
if [ "$GIGA_STORAGE" = "true" ] && [ "$GIGA_FLATKV_ONLY" != "true" ]; then
  # Default receipt backend to pebble when giga storage is on; callers may
  # still override via an explicit RECEIPT_BACKEND env var.
  RECEIPT_BACKEND=${RECEIPT_BACKEND:-pebble}
  echo "Enabling Giga Storage for RPC node..."

  # SC layer: must match validators (test_only_dual_write)
  if grep -q '^sc-write-mode[[:space:]]*=' ~/.pax/config/app.toml; then
    sed -i 's/^sc-write-mode[[:space:]]*=.*/sc-write-mode = "test_only_dual_write"/' ~/.pax/config/app.toml
  else
    sed -i '/^\[state-store\]/i sc-write-mode = "test_only_dual_write"' ~/.pax/config/app.toml
  fi

  # SS layer: enable EVM split
  sed -i 's/^evm-ss-split[[:space:]]*=.*/evm-ss-split = true/' ~/.pax/config/app.toml
fi

if [ "$GIGA_FLATKV_ONLY" = "true" ]; then
  echo "Booting RPC node in flatkv_only mode..."
  if grep -q '^sc-write-mode[[:space:]]*=' ~/.pax/config/app.toml; then
    sed -i 's/^sc-write-mode[[:space:]]*=.*/sc-write-mode = "flatkv_only"/' ~/.pax/config/app.toml
  else
    sed -i '/^\[state-store\]/i sc-write-mode = "flatkv_only"' ~/.pax/config/app.toml
  fi
  sed -i 's/^evm-ss-split[[:space:]]*=.*/evm-ss-split = false/' ~/.pax/config/app.toml
fi

# Apply receipt backend override if requested
RECEIPT_BACKEND=${RECEIPT_BACKEND:-}
if [ -n "$RECEIPT_BACKEND" ]; then
  echo "Setting receipt store backend to '$RECEIPT_BACKEND' for RPC node..."
  if grep -q "\[receipt-store\]" ~/.pax/config/app.toml; then
    sed -i "s/rs-backend = .*/rs-backend = \"$RECEIPT_BACKEND\"/" ~/.pax/config/app.toml
  else
    echo "" >> ~/.pax/config/app.toml
    echo "[receipt-store]" >> ~/.pax/config/app.toml
    echo "rs-backend = \"$RECEIPT_BACKEND\"" >> ~/.pax/config/app.toml
  fi
fi

# Override state sync configs
STATE_SYNC_RPC="192.168.10.10:26657"
STATE_SYNC_PEER="2f9846450b7a3dcf4af1ac0082e3279c16744df8@172.31.9.18:26656,ec98c4a28a2023f4f976828c8a8e7127bfef4e1b@172.31.4.96:26656,b03014d67384fb0ef6ad992c77cefe4f9d2c1640@172.31.4.219:26656"
curl "$STATE_SYNC_RPC"/net_info |jq -r '.peers[] | .url' |sed -e 's#mconn://##' >> build/generated/PEERS
STATE_SYNC_PEER=$(paste -s -d ',' build/generated/PEERS)
LATEST_HEIGHT=$(curl -s $STATE_SYNC_RPC/block | jq -r .block.header.height)
SYNC_BLOCK_HEIGHT=$LATEST_HEIGHT
SYNC_BLOCK_HASH=$(curl -s "$STATE_SYNC_RPC/block?height=$SYNC_BLOCK_HEIGHT" | jq -r .block_id.hash)
sed -i.bak -e "s|^enable *=.*|enable = true|" ~/.pax/config/config.toml
sed -i.bak -e "s|^rpc-servers *=.*|rpc-servers = \"$STATE_SYNC_RPC,$STATE_SYNC_RPC\"|" ~/.pax/config/config.toml
sed -i.bak -e "s|^trust-height *=.*|trust-height = $SYNC_BLOCK_HEIGHT|" ~/.pax/config/config.toml
sed -i.bak -e "s|^trust-hash *=.*|trust-hash = \"$SYNC_BLOCK_HASH\"|" ~/.pax/config/config.toml
sed -i.bak -e "s|^persistent-peers *=.*|persistent-peers = \"$STATE_SYNC_PEER\"|" ~/.pax/config/config.toml
