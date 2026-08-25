#!/bin/bash
# require success for commands
set -e

# Parse command line arguments
MOCK_BALANCES=${MOCK_BALANCES:-true}
GIGA_EXECUTOR=${GIGA_EXECUTOR:-false}
GIGA_OCC=${GIGA_OCC:-false}
BENCHMARK_TXS_PER_BATCH=${BENCHMARK_TXS_PER_BATCH:-1000}
DISABLE_INDEXER=${DISABLE_INDEXER:-true}
# Debug mode - if true, prints all log output without filtering
DEBUG=${DEBUG:-false}

# DB_BACKEND options:
#   goleveldb - default, pure Go, can have compaction stalls under heavy write load
#   memdb     - in-memory only, fastest (no disk I/O), data lost on restart
#   cleveldb  - C LevelDB, faster than goleveldb, requires CGO
#   rocksdb   - best compaction control, requires CGO and rocksdb libs
DB_BACKEND=${DB_BACKEND:-goleveldb}

# --- New env vars for phase gating / multi-instance support ---
# Final chain data dir. If != ~/.pax, init in ~/.pax then mv.
PAX_HOME=${PAX_HOME:-"$HOME/.pax"}
# Added to all ports (RPC, P2P, pprof, gRPC, etc.)
PORT_OFFSET=${PORT_OFFSET:-0}
# Pre-built binary path. If set, skip build + copy to ~/go/bin/paxd.
PAXD_BIN=${PAXD_BIN:-""}
# Phase control: "init" (build+init+configure), "start" (run node), "all" (both)
BENCHMARK_PHASE=${BENCHMARK_PHASE:-all}
# Redirect paxd output to file
LOG_FILE=${LOG_FILE:-""}

# Portable path resolution
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Benchmark scenario config (path to JSON file, see benchmark/scenarios/)
BENCHMARK_CONFIG=${BENCHMARK_CONFIG:-"$SCRIPT_DIR/scenarios/evm.json"}

# Use python3 as default, but fall back to python if python3 doesn't exist
PYTHON_CMD=python3
if ! command -v $PYTHON_CMD &> /dev/null
then
    PYTHON_CMD=python
fi

# set key name
keyname=admin

# Determine which paxd binary to use
if [ -n "$PAXD_BIN" ]; then
  PAXD="$PAXD_BIN"
else
  PAXD="$HOME/go/bin/paxd"
fi

# ============================================================
# Phase: init (build + init + configure)
# ============================================================
if [ "$BENCHMARK_PHASE" = "init" ] || [ "$BENCHMARK_PHASE" = "all" ]; then

# Display configuration
echo "=== Benchmark Configuration ==="
echo "  MOCK_BALANCES:           $MOCK_BALANCES"
echo "  GIGA_EXECUTOR:           $GIGA_EXECUTOR"
echo "  GIGA_OCC:                $GIGA_OCC"
echo "  DB_BACKEND:              $DB_BACKEND"
echo "  BENCHMARK_TXS_PER_BATCH: $BENCHMARK_TXS_PER_BATCH"
echo "  DISABLE_INDEXER:         $DISABLE_INDEXER"
echo "  DEBUG:                   $DEBUG"
echo "  BENCHMARK_CONFIG:        ${BENCHMARK_CONFIG:-(default: EVMTransfer)}"
echo "  PAX_HOME:                $PAX_HOME"
echo "  PORT_OFFSET:             $PORT_OFFSET"
echo "  PAXD_BIN:                ${PAXD_BIN:-(build from source)}"
echo "  BENCHMARK_PHASE:         $BENCHMARK_PHASE"
echo ""
echo "Available scenarios in $SCRIPT_DIR/scenarios/:"
ls -1 "$SCRIPT_DIR"/scenarios/*.json 2>/dev/null | sed 's/^/    /' || echo "    (none found)"
echo "================================"

# clean up old pax directory
rm -rf ~/.pax
echo "Building..."

if [ -n "$PAXD_BIN" ]; then
  # Use pre-built binary: copy to ~/go/bin/paxd so downstream tools find it
  echo "Using pre-built binary: $PAXD_BIN"
  mkdir -p ~/go/bin
  cp "$PAXD_BIN" ~/go/bin/paxd
else
  # Determine build options based on DB_BACKEND
  BUILD_TAGS=""
  case "$DB_BACKEND" in
    cleveldb)
      echo "Building with cleveldb support (C LevelDB - faster)..."
      BUILD_TAGS="cleveldb"
      ;;
    rocksdb)
      echo "Building with rocksdb support (best compaction control)..."
      BUILD_TAGS="rocksdb"
      ;;
    goleveldb|memdb)
      echo "Building with default goleveldb support..."
      ;;
    *)
      echo "ERROR: Unknown DB_BACKEND '$DB_BACKEND'. Valid options: goleveldb, memdb, cleveldb, rocksdb"
      exit 1
      ;;
  esac

  # install paxd with benchmark support (includes mock_balances)
  echo "Building with benchmark and mock balances support enabled..."
  if [ -n "$BUILD_TAGS" ]; then
    COSMOS_BUILD_OPTIONS="$BUILD_TAGS" make -C "$REPO_ROOT" install-bench
  else
    make -C "$REPO_ROOT" install-bench
  fi
fi

# initialize chain with chain ID and add the first key
"$PAXD" init demo --chain-id pax-chain --overwrite
"$PAXD" keys add $keyname --keyring-backend test
# add the key as a genesis account with massive balances of several different tokens
"$PAXD" add-genesis-account $("$PAXD" keys show $keyname -a --keyring-backend test) 100000000000000000000uhpx,100000000000000000000uusdc,100000000000000000000uatom --keyring-backend test
# gentx for account
"$PAXD" gentx $keyname 7000000000000000uhpx --chain-id pax-chain --keyring-backend test
# add validator information to genesis file
KEY=$(jq '.pub_key' ~/.pax/config/priv_validator_key.json -c)
jq '.validators = [{}]' ~/.pax/config/genesis.json > ~/.pax/config/tmp_genesis.json
jq '.validators[0] += {"power":"7000000000"}' ~/.pax/config/tmp_genesis.json > ~/.pax/config/tmp_genesis_2.json
jq '.validators[0] += {"pub_key":'$KEY'}' ~/.pax/config/tmp_genesis_2.json > ~/.pax/config/tmp_genesis_3.json
mv ~/.pax/config/tmp_genesis_3.json ~/.pax/config/genesis.json && rm ~/.pax/config/tmp_genesis.json && rm ~/.pax/config/tmp_genesis_2.json

echo "Creating Accounts"
# create 10 test accounts + fund them
python3  "$REPO_ROOT/loadtest/scripts/populate_genesis_accounts.py" 20 loc

"$PAXD" collect-gentxs
# update some params in genesis file for easier use of the chain localls (make gov props faster)
cat ~/.pax/config/genesis.json | jq '.app_state["gov"]["deposit_params"]["max_deposit_period"]="60s"' > ~/.pax/config/tmp_genesis.json && mv ~/.pax/config/tmp_genesis.json ~/.pax/config/genesis.json
cat ~/.pax/config/genesis.json | jq '.app_state["gov"]["voting_params"]["voting_period"]="30s"' > ~/.pax/config/tmp_genesis.json && mv ~/.pax/config/tmp_genesis.json ~/.pax/config/genesis.json
cat ~/.pax/config/genesis.json | jq '.app_state["gov"]["voting_params"]["expedited_voting_period"]="10s"' > ~/.pax/config/tmp_genesis.json && mv ~/.pax/config/tmp_genesis.json ~/.pax/config/genesis.json
cat ~/.pax/config/genesis.json | jq '.app_state["oracle"]["params"]["vote_period"]="2"' > ~/.pax/config/tmp_genesis.json && mv ~/.pax/config/tmp_genesis.json ~/.pax/config/genesis.json
cat ~/.pax/config/genesis.json | jq '.app_state["evm"]["params"]["target_gas_used_per_block"]="1000000000000"' > ~/.pax/config/tmp_genesis.json && mv ~/.pax/config/tmp_genesis.json ~/.pax/config/genesis.json
cat ~/.pax/config/genesis.json | jq '.app_state["oracle"]["params"]["whitelist"]=[{"name": "ueth"},{"name": "ubtc"},{"name": "uusdc"},{"name": "uusdt"},{"name": "uosmo"},{"name": "uatom"},{"name": "uhpx"}]' > ~/.pax/config/tmp_genesis.json && mv ~/.pax/config/tmp_genesis.json ~/.pax/config/genesis.json
cat ~/.pax/config/genesis.json | jq '.app_state["distribution"]["params"]["community_tax"]="0.000000000000000000"' > ~/.pax/config/tmp_genesis.json && mv ~/.pax/config/tmp_genesis.json ~/.pax/config/genesis.json
cat ~/.pax/config/genesis.json | jq '.consensus_params["block"]["max_gas"]="100000000"' > ~/.pax/config/tmp_genesis.json && mv ~/.pax/config/tmp_genesis.json ~/.pax/config/genesis.json
cat ~/.pax/config/genesis.json | jq '.consensus_params["block"]["min_txs_in_block"]="2"' > ~/.pax/config/tmp_genesis.json && mv ~/.pax/config/tmp_genesis.json ~/.pax/config/genesis.json
cat ~/.pax/config/genesis.json | jq '.consensus_params["block"]["max_gas_wanted"]="150000000"' > ~/.pax/config/tmp_genesis.json && mv ~/.pax/config/tmp_genesis.json ~/.pax/config/genesis.json
cat ~/.pax/config/genesis.json | jq '.app_state["staking"]["params"]["max_voting_power_ratio"]="1.000000000000000000"' > ~/.pax/config/tmp_genesis.json && mv ~/.pax/config/tmp_genesis.json ~/.pax/config/genesis.json
cat ~/.pax/config/genesis.json | jq '.app_state["bank"]["denom_metadata"]=[{"denom_units":[{"denom":"uhpx","exponent":0,"aliases":["UHPX"]}],"base":"uhpx","display":"uhpx","name":"UHPX","symbol":"UHPX"}]' > ~/.pax/config/tmp_genesis.json && mv ~/.pax/config/tmp_genesis.json ~/.pax/config/genesis.json

# Use the Python command to get the dates
START_DATE=$($PYTHON_CMD -c "from datetime import datetime; print(datetime.now().strftime('%Y-%m-%d'))")
END_DATE_3DAYS=$($PYTHON_CMD -c "from datetime import datetime, timedelta; print((datetime.now() + timedelta(days=3)).strftime('%Y-%m-%d'))")
END_DATE_5DAYS=$($PYTHON_CMD -c "from datetime import datetime, timedelta; print((datetime.now() + timedelta(days=5)).strftime('%Y-%m-%d'))")

cat ~/.pax/config/genesis.json | jq --arg start_date "$START_DATE" --arg end_date "$END_DATE_3DAYS" '.app_state["mint"]["params"]["token_release_schedule"]=[{"start_date": $start_date, "end_date": $end_date, "token_release_amount": "999999999999"}]' > ~/.pax/config/tmp_genesis.json && mv ~/.pax/config/tmp_genesis.json ~/.pax/config/genesis.json
cat ~/.pax/config/genesis.json | jq --arg start_date "$END_DATE_3DAYS" --arg end_date "$END_DATE_5DAYS" '.app_state["mint"]["params"]["token_release_schedule"] += [{"start_date": $start_date, "end_date": $end_date, "token_release_amount": "999999999999"}]' > ~/.pax/config/tmp_genesis.json && mv ~/.pax/config/tmp_genesis.json ~/.pax/config/genesis.json

if [ ! -z "$2" ]; then
  APP_TOML_PATH="$2"
else
  APP_TOML_PATH="$HOME/.pax/config/app.toml"
fi
# Enable OCC and PaxDB
sed -i.bak -e 's/# concurrency-workers = .*/concurrency-workers = 500/' "$APP_TOML_PATH"
sed -i.bak -e 's/occ-enabled = .*/occ-enabled = true/' "$APP_TOML_PATH"
sed -i.bak -e 's/sc-enable = .*/sc-enable = true/' "$APP_TOML_PATH"
sed -i.bak -e 's/ss-enable = .*/ss-enable = true/' "$APP_TOML_PATH"

# Enable Giga Executor if requested
if [ "$GIGA_EXECUTOR" = true ]; then
  echo "Enabling Giga Executor..."
  if grep -q "\[giga_executor\]" "$APP_TOML_PATH"; then
    # If the section exists, update enabled to true
    if [[ "$OSTYPE" == "darwin"* ]]; then
      sed -i '' '/\[giga_executor\]/,/^\[/ s/enabled = false/enabled = true/' "$APP_TOML_PATH"
    else
      sed -i '/\[giga_executor\]/,/^\[/ s/enabled = false/enabled = true/' "$APP_TOML_PATH"
    fi
  else
    # If section doesn't exist, append it
    echo "" >> "$APP_TOML_PATH"
    echo "[giga_executor]" >> "$APP_TOML_PATH"
    echo "enabled = true" >> "$APP_TOML_PATH"
    echo "occ_enabled = false" >> "$APP_TOML_PATH"
  fi

  # Set OCC based on GIGA_OCC flag
  if [ "$GIGA_OCC" = true ]; then
    echo "Enabling OCC for Giga Executor..."
    if [[ "$OSTYPE" == "darwin"* ]]; then
      sed -i '' 's/occ_enabled = false/occ_enabled = true/' "$APP_TOML_PATH"
    else
      sed -i 's/occ_enabled = false/occ_enabled = true/' "$APP_TOML_PATH"
    fi
  else
    echo "Disabling OCC for Giga Executor (sequential mode)..."
    if [[ "$OSTYPE" == "darwin"* ]]; then
      sed -i '' 's/occ_enabled = true/occ_enabled = false/' "$APP_TOML_PATH"
    else
      sed -i 's/occ_enabled = true/occ_enabled = false/' "$APP_TOML_PATH"
    fi
  fi
fi

# set block time to 2s
if [ ! -z "$1" ]; then
  CONFIG_PATH="$1"
else
  CONFIG_PATH="$HOME/.pax/config/config.toml"
fi

if [ ! -z "$2" ]; then
  APP_PATH="$2"
else
  APP_PATH="$HOME/.pax/config/app.toml"
fi

if [[ "$OSTYPE" == "linux-gnu"* ]]; then
  sed -i 's/mode = "full"/mode = "validator"/g' "$CONFIG_PATH"
  if [ "$DISABLE_INDEXER" = true ]; then
    sed -i 's/indexer = \["kv"\]/indexer = \["null"\]/g' "$CONFIG_PATH"
    echo "Indexer disabled"
  fi
  sed -i 's/skip_timeout_commit =.*/skip_timeout_commit = false/g' "$CONFIG_PATH"
  sed -i 's/pprof-laddr = ""/pprof-laddr = ":6060"/g' "$CONFIG_PATH"
  # Set the DB backend
  sed -i "s/db-backend = \"goleveldb\"/db-backend = \"$DB_BACKEND\"/g" "$CONFIG_PATH"
  echo "DB backend set to: $DB_BACKEND"
elif [[ "$OSTYPE" == "darwin"* ]]; then
  sed -i '' 's/mode = "full"/mode = "validator"/g' "$CONFIG_PATH"
  if [ "$DISABLE_INDEXER" = true ]; then
    sed -i '' 's/indexer = \["kv"\]/indexer = \["null"\]/g' "$CONFIG_PATH"
    echo "Indexer disabled"
  fi
  sed -i '' 's/pprof-laddr = ""/pprof-laddr = ":6060"/g' "$CONFIG_PATH"
  # Set the DB backend
  sed -i '' "s/db-backend = \"goleveldb\"/db-backend = \"$DB_BACKEND\"/g" "$CONFIG_PATH"
  echo "DB backend set to: $DB_BACKEND"
else
  printf "Platform not supported, please ensure that the following values are set in your config.toml:\n"
  printf "###         Consensus Configuration Options         ###\n"
  printf "\t timeout_prevote = \"2000ms\"\n"
  printf "\t timeout_precommit = \"2000ms\"\n"
  printf "\t timeout_commit = \"2000ms\"\n"
  printf "\t skip_timeout_commit = false\n"
  exit 1
fi

# Apply port offsets if non-zero
if [ "$PORT_OFFSET" -ne 0 ] 2>/dev/null; then
  RPC=$((26657 + PORT_OFFSET))
  P2P=$((26656 + PORT_OFFSET))
  PPROF=$((6060 + PORT_OFFSET))
  GRPC=$((9090 + PORT_OFFSET))
  GRPCWEB=$((9091 + PORT_OFFSET))
  API=$((1317 + PORT_OFFSET))
  ROSETTA=$((8080 + PORT_OFFSET))
  EVM_HTTP=$((8545 + PORT_OFFSET))
  EVM_WS=$((8546 + PORT_OFFSET))

  if [[ "$OSTYPE" == "darwin"* ]]; then
    sed -i '' "s|pprof-laddr = .*|pprof-laddr = \"localhost:${PPROF}\"|g" "$CONFIG_PATH"
    sed -i '' "s|laddr = \"tcp://127.0.0.1:26657\"|laddr = \"tcp://127.0.0.1:${RPC}\"|g" "$CONFIG_PATH"
    sed -i '' "s|laddr = \"tcp://127.0.0.1:26656\"|laddr = \"tcp://127.0.0.1:${P2P}\"|g" "$CONFIG_PATH"
    sed -i '' "s|address = \"0.0.0.0:9090\"|address = \"0.0.0.0:${GRPC}\"|g" "$APP_PATH"
    sed -i '' "s|address = \"0.0.0.0:9091\"|address = \"0.0.0.0:${GRPCWEB}\"|g" "$APP_PATH"
    sed -i '' "s|address = \"tcp://0.0.0.0:1317\"|address = \"tcp://0.0.0.0:${API}\"|g" "$APP_PATH"
    sed -i '' "s|address = \":8080\"|address = \":${ROSETTA}\"|g" "$APP_PATH"
    sed -i '' "s|http_port = 8545|http_port = ${EVM_HTTP}|g" "$APP_PATH"
    sed -i '' "s|ws_port = 8546|ws_port = ${EVM_WS}|g" "$APP_PATH"
  else
    sed -i "s|pprof-laddr = .*|pprof-laddr = \"localhost:${PPROF}\"|g" "$CONFIG_PATH"
    sed -i "s|laddr = \"tcp://127.0.0.1:26657\"|laddr = \"tcp://127.0.0.1:${RPC}\"|g" "$CONFIG_PATH"
    sed -i "s|laddr = \"tcp://127.0.0.1:26656\"|laddr = \"tcp://127.0.0.1:${P2P}\"|g" "$CONFIG_PATH"
    sed -i "s|address = \"0.0.0.0:9090\"|address = \"0.0.0.0:${GRPC}\"|g" "$APP_PATH"
    sed -i "s|address = \"0.0.0.0:9091\"|address = \"0.0.0.0:${GRPCWEB}\"|g" "$APP_PATH"
    sed -i "s|address = \"tcp://0.0.0.0:1317\"|address = \"tcp://0.0.0.0:${API}\"|g" "$APP_PATH"
    sed -i "s|address = \":8080\"|address = \":${ROSETTA}\"|g" "$APP_PATH"
    sed -i "s|http_port = 8545|http_port = ${EVM_HTTP}|g" "$APP_PATH"
    sed -i "s|ws_port = 8546|ws_port = ${EVM_WS}|g" "$APP_PATH"
  fi
fi

"$PAXD" config keyring-backend test

# If PAX_HOME is not ~/.pax, move the data dir
if [ "$PAX_HOME" != "$HOME/.pax" ]; then
  mkdir -p "$(dirname "$PAX_HOME")"
  mv ~/.pax "$PAX_HOME"
fi

fi # end BENCHMARK_PHASE=init

# ============================================================
# Phase: start (run the node)
# ============================================================
if [ "$BENCHMARK_PHASE" = "start" ] || [ "$BENCHMARK_PHASE" = "all" ]; then

PPROF_PORT=$((6060 + PORT_OFFSET))

# start the chain with log tracing
# Benchmark mode is enabled via build tag, no --benchmark flag needed
echo ""
echo "=== pprof enabled at http://localhost:${PPROF_PORT}/debug/pprof ==="
echo "To capture 30s CPU profile during benchmark:"
echo "  go tool pprof http://localhost:${PPROF_PORT}/debug/pprof/profile?seconds=30"
echo "To capture heap profile:"
echo "  go tool pprof http://localhost:${PPROF_PORT}/debug/pprof/heap"
echo "============================================================"
echo ""

# Build the paxd command args
PAXD_ARGS=(start --chain-id pax-chain)
if [ "$PAX_HOME" != "$HOME/.pax" ]; then
  PAXD_ARGS+=(--home "$PAX_HOME")
fi

if [ -n "$LOG_FILE" ]; then
  # Redirect all output to log file
  BENCHMARK_CONFIG=$BENCHMARK_CONFIG BENCHMARK_TXS_PER_BATCH=$BENCHMARK_TXS_PER_BATCH "$PAXD" "${PAXD_ARGS[@]}" > "$LOG_FILE" 2>&1
elif [ "$DEBUG" = true ]; then
  # Debug mode: print all output
  BENCHMARK_CONFIG=$BENCHMARK_CONFIG BENCHMARK_TXS_PER_BATCH=$BENCHMARK_TXS_PER_BATCH "$PAXD" "${PAXD_ARGS[@]}"
else
  # Normal mode: filter to benchmark-related output only
  BENCHMARK_CONFIG=$BENCHMARK_CONFIG BENCHMARK_TXS_PER_BATCH=$BENCHMARK_TXS_PER_BATCH "$PAXD" "${PAXD_ARGS[@]}" 2>&1 | grep -E "(benchmark|Benchmark|deployed|transitioning)"
fi

fi # end BENCHMARK_PHASE=start
