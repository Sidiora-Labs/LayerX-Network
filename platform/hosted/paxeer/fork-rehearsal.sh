#!/usr/bin/env bash
# Rehearses the Paxeer X software upgrade ($UPGRADE_NAME, default v6.5) on a disposable
# single-validator fork of mainnet.
#
# State comes from `paxd export` run by the OLD binary over a copy of a stopped, synced node's data
# directory. paxd has no in-place testnet command, so rewriting the validator set of a copied data
# directory would mean editing committed IAVL/SeiDB state by hand; an export is the path the binary
# supports. The exported genesis keeps the mainnet chain id ($COSMOS_CHAIN_ID, EVM chain id $EVM_CHAIN_ID) and
# its height, and is rewritten so one disposable key signs every block:
#   - the first validator in last_validator_powers keeps its operator, tokens and delegations, and
#     its consensus key becomes the disposable key; the other validators are jailed and unbonded and
#     their tokens move from the bonded pool to the not-bonded pool, which staking InitGenesis checks;
#   - the disposable consensus address gets a slashing signing info, which slashing requires;
#   - gov voting periods shrink to --voting-seconds and the oracle miss threshold drops to zero so the
#     lone validator, which runs no price feeder, is not jailed by an oracle slash window mid-rehearsal;
#   - a disposable account is funded (bank supply is recomputed from balances) and, once the chain
#     runs, delegates nine times the kept validator's tokens so its YES vote carries quorum and the
#     unchanged tally thresholds.
# The chain then runs on --old-bin, a $UPGRADE_NAME software-upgrade proposal for current height + N passes,
# the old binary must halt with the UPGRADE NEEDED log line at exactly that height, and --new-bin must
# resume it: height advances, `paxd q upgrade applied $UPGRADE_NAME` reports the height, every module added by
# the $UPGRADE_NAME store upgrade reports a module version, and a view call on each LayerX precompile returns
# without revert.
#
# Every host-specific value (paths, ports) comes from flags or from an untracked env file passed with
# --env-file (or named by $PAXEER_FORK_ENV); nothing here contacts a remote host. Precedence:
# flags > env file > built-in defaults. The env file holds KEY=VALUE lines (blank lines, `#` comments
# and an optional `export ` prefix allowed); keys this script does not read are ignored, so the same
# file can carry the runbook's keys. Keys read:
#   UPGRADE_NAME COSMOS_CHAIN_ID EVM_CHAIN_ID SOURCE_DATA SOURCE_GENESIS OLD_BIN NEW_BIN WORK
#   UPGRADE_HEIGHT_OFFSET PORT_BASE VOTING_SECONDS FEES GAS WAIT_SECONDS
#
# --check validates this file's syntax, the given flags and the required tools, and touches nothing.
set -euo pipefail

UPGRADE_NAME=v6.5
COSMOS_CHAIN_ID=hyperpax_125-1
EVM_CHAIN_ID=125
KEY_NAME=rehearsal
ENV_FILE=${PAXEER_FORK_ENV:-}

SOURCE_DATA=""
SOURCE_GENESIS=""
OLD_BIN=""
NEW_BIN=""
WORK=""
OFFSET=""
CHECK=0
PORT_BASE=36000
VOTING_SECONDS=30
FEES=1000000uhpx
GAS=2000000
WAIT_SECONDS=1800
MODULES=(layerxcustody layerxanchor layerxexchange layerxbridge launchpad)
MODULES_SET=0
declare -A VIEWS=(
    [0x0000000000000000000000000000000000001013]=0x2dfdf0b5 # depositCount()
    [0x0000000000000000000000000000000000001014]=0x42cde4e8 # threshold()
)
PRECOMPILES=(
    0x0000000000000000000000000000000000001013
    0x0000000000000000000000000000000000001014
    0x0000000000000000000000000000000000001015
    0x0000000000000000000000000000000000001016
    0x0000000000000000000000000000000000001017
)

NODE_PID=""
LOG_DIR=""

log() {
    printf '%s fork-rehearsal: %s\n' "$(date -u +%H:%M:%SZ)" "$*" >&2
}

fail() {
    echo "fork-rehearsal: $*" >&2
    exit 1
}

usage() {
    cat >&2 <<'USAGE'
usage: fork-rehearsal.sh --source-data DIR --old-bin PATH --new-bin PATH --work DIR --upgrade-height-offset N [options]
       fork-rehearsal.sh --check [any of the flags above]

required (flag, or the env file key in brackets):
  --source-data DIR           data directory of a STOPPED synced mainnet node (copied, never modified) [SOURCE_DATA]
  --old-bin PATH              paxd currently running mainnet (must not know the upgrade) [OLD_BIN]
  --new-bin PATH              candidate paxd carrying the upgrade handler [NEW_BIN]
  --work DIR                  empty or absent directory for the disposable chain [WORK]
  --upgrade-height-offset N   upgrade height = height at proposal submission + N [UPGRADE_HEIGHT_OFFSET]
options:
  --env-file FILE             untracked KEY=VALUE file supplying any bracketed key (default $PAXEER_FORK_ENV)
  --upgrade-name NAME         upgrade plan name (default v6.5) [UPGRADE_NAME]
  --chain-id ID               cosmos chain id of the export (default hyperpax_125-1) [COSMOS_CHAIN_ID]
  --evm-chain-id N            EVM chain id the resumed node must report (default 125) [EVM_CHAIN_ID]
  --source-genesis FILE       mainnet genesis (default: DIR/../config/genesis.json) [SOURCE_GENESIS]
  --port-base P               listeners at P+657 rpc, P+656 p2p, P+2545 evm, P+3090 grpc (default 36000) [PORT_BASE]
  --voting-seconds S          disposable gov voting period (default 30) [VOTING_SECONDS]
  --fees COINS                fee per rehearsal transaction (default 1000000uhpx) [FEES]
  --gas N                     gas limit per rehearsal transaction (default 2000000) [GAS]
  --wait-seconds S            limit on each wait for the chain (default 1800) [WAIT_SECONDS]
  --module NAME               module the store upgrade must add (repeatable; default
                              layerxcustody layerxanchor layerxexchange layerxbridge launchpad)
  --view ADDR=CALLDATA        view call for a precompile; defaults exist for 0x...1013 (depositCount())
                              and 0x...1014 (threshold()); 0x...1015, 0x...1016 and 0x...1017 need one
  --check                     validate syntax, flags and tools, then exit without touching state
USAGE
}

need_value() {
    [ "$#" -ge 2 ] && [ -n "$2" ] || fail "$1 needs a value"
}

# The env file is read before the flags so that flags override it.
args=("$@")
for ((i = 0; i < ${#args[@]}; i++)); do
    if [ "${args[$i]}" = "--env-file" ]; then
        [ $((i + 1)) -lt ${#args[@]} ] && [ -n "${args[$((i + 1))]}" ] || fail "--env-file needs a value"
        ENV_FILE=${args[$((i + 1))]}
    fi
done

load_env_file() {
    local file=$1 line key value lineno=0
    [ -r "$file" ] || fail "env file $file is not readable"
    while IFS= read -r line || [ -n "$line" ]; do
        lineno=$((lineno + 1))
        line=${line#"${line%%[![:space:]]*}"}
        [ -z "$line" ] || [ "${line:0:1}" = "#" ] && continue
        line=${line#export }
        [[ "$line" =~ ^([A-Z_][A-Z0-9_]*)=(.*)$ ]] || fail "env file $file line $lineno is not KEY=VALUE"
        key=${BASH_REMATCH[1]}
        value=${BASH_REMATCH[2]}
        value=${value%"${value##*[![:space:]]}"}
        if [[ "$value" =~ ^\"(.*)\"$ ]] || [[ "$value" =~ ^\'(.*)\'$ ]]; then
            value=${BASH_REMATCH[1]}
        fi
        case "$key" in
            UPGRADE_NAME) UPGRADE_NAME=$value ;;
            COSMOS_CHAIN_ID) COSMOS_CHAIN_ID=$value ;;
            EVM_CHAIN_ID) EVM_CHAIN_ID=$value ;;
            SOURCE_DATA) SOURCE_DATA=$value ;;
            SOURCE_GENESIS) SOURCE_GENESIS=$value ;;
            OLD_BIN) OLD_BIN=$value ;;
            NEW_BIN) NEW_BIN=$value ;;
            WORK) WORK=$value ;;
            UPGRADE_HEIGHT_OFFSET) OFFSET=$value ;;
            PORT_BASE) PORT_BASE=$value ;;
            VOTING_SECONDS) VOTING_SECONDS=$value ;;
            FEES) FEES=$value ;;
            GAS) GAS=$value ;;
            WAIT_SECONDS) WAIT_SECONDS=$value ;;
            *) ;;
        esac
    done <"$file"
}
[ -z "$ENV_FILE" ] || load_env_file "$ENV_FILE"

while [ "$#" -gt 0 ]; do
    case "$1" in
        --env-file) need_value "$@"; shift 2 ;;
        --upgrade-name) need_value "$@"; UPGRADE_NAME=$2; shift 2 ;;
        --chain-id) need_value "$@"; COSMOS_CHAIN_ID=$2; shift 2 ;;
        --evm-chain-id) need_value "$@"; EVM_CHAIN_ID=$2; shift 2 ;;
        --source-data) need_value "$@"; SOURCE_DATA=$2; shift 2 ;;
        --source-genesis) need_value "$@"; SOURCE_GENESIS=$2; shift 2 ;;
        --old-bin) need_value "$@"; OLD_BIN=$2; shift 2 ;;
        --new-bin) need_value "$@"; NEW_BIN=$2; shift 2 ;;
        --work) need_value "$@"; WORK=$2; shift 2 ;;
        --upgrade-height-offset) need_value "$@"; OFFSET=$2; shift 2 ;;
        --port-base) need_value "$@"; PORT_BASE=$2; shift 2 ;;
        --voting-seconds) need_value "$@"; VOTING_SECONDS=$2; shift 2 ;;
        --fees) need_value "$@"; FEES=$2; shift 2 ;;
        --gas) need_value "$@"; GAS=$2; shift 2 ;;
        --wait-seconds) need_value "$@"; WAIT_SECONDS=$2; shift 2 ;;
        --module)
            need_value "$@"
            if [ "$MODULES_SET" -eq 0 ]; then MODULES=(); MODULES_SET=1; fi
            MODULES+=("$2"); shift 2 ;;
        --view)
            need_value "$@"
            [[ "$2" =~ ^(0x[0-9a-fA-F]{40})=(0x[0-9a-fA-F]{8}([0-9a-fA-F]{2})*)$ ]] \
                || fail "--view takes ADDR=CALLDATA with a 20-byte address and at least a 4-byte selector"
            VIEWS[$(printf '%s' "${BASH_REMATCH[1]}" | tr 'A-F' 'a-f')]=${BASH_REMATCH[2]}
            shift 2 ;;
        --check) CHECK=1; shift ;;
        -h|--help) usage; exit 0 ;;
        *) usage; fail "unknown argument $1" ;;
    esac
done

[[ "$UPGRADE_NAME" =~ ^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$ ]] || fail "--upgrade-name must be letters, digits, '.', '_' or '-'"
[[ "$COSMOS_CHAIN_ID" =~ ^[a-z][a-z0-9_-]*_[1-9][0-9]*-[1-9][0-9]*$ ]] || fail "--chain-id must look like name_EVMID-VERSION"
[[ "$EVM_CHAIN_ID" =~ ^[1-9][0-9]{0,17}$ ]] || fail "--evm-chain-id must be a positive integer"
[ -z "$OFFSET" ] || [[ "$OFFSET" =~ ^[1-9][0-9]{0,5}$ ]] || fail "--upgrade-height-offset must be a positive integer"
[[ "$PORT_BASE" =~ ^[1-9][0-9]{3,4}$ ]] && [ "$PORT_BASE" -le 60000 ] || fail "--port-base must be 1000 through 60000"
[[ "$VOTING_SECONDS" =~ ^[1-9][0-9]{0,4}$ ]] && [ "$VOTING_SECONDS" -ge 10 ] || fail "--voting-seconds must be at least 10"
[[ "$FEES" =~ ^[1-9][0-9]*uhpx$ ]] || fail "--fees must be a positive uhpx amount"
[[ "$GAS" =~ ^[1-9][0-9]{0,9}$ ]] || fail "--gas must be a positive integer"
[[ "$WAIT_SECONDS" =~ ^[1-9][0-9]{0,5}$ ]] || fail "--wait-seconds must be a positive integer"
for module in "${MODULES[@]}"; do
    [[ "$module" =~ ^[a-z][a-z0-9_]*$ ]] || fail "module name $module is not a store key"
done

RPC_PORT=$((PORT_BASE + 657))
P2P_PORT=$((PORT_BASE + 656))
EVM_PORT=$((PORT_BASE + 2545))
EVM_WS_PORT=$((PORT_BASE + 2546))
GRPC_PORT=$((PORT_BASE + 3090))
GRPC_WEB_PORT=$((PORT_BASE + 3091))
NODE=tcp://127.0.0.1:$RPC_PORT

bash -n "${BASH_SOURCE[0]}" || fail "syntax check of ${BASH_SOURCE[0]} failed"
for tool in jq curl sha256sum awk sed grep cp; do
    command -v "$tool" >/dev/null 2>&1 || fail "$tool is not available"
done
if [ -n "$OLD_BIN" ]; then
    command -v "$OLD_BIN" >/dev/null 2>&1 || fail "--old-bin $OLD_BIN is not executable"
else
    command -v paxd >/dev/null 2>&1 || fail "paxd is not available"
fi
if [ -n "$NEW_BIN" ]; then
    command -v "$NEW_BIN" >/dev/null 2>&1 || fail "--new-bin $NEW_BIN is not executable"
fi

if [ "$CHECK" -eq 1 ]; then
    echo "fork-rehearsal: check passed (syntax, flags, tools); no state touched" >&2
    exit 0
fi

[ -n "$SOURCE_DATA" ] || fail "--source-data is required"
[ -n "$OLD_BIN" ] || fail "--old-bin is required"
[ -n "$NEW_BIN" ] || fail "--new-bin is required"
[ -n "$WORK" ] || fail "--work is required"
[ -n "$OFFSET" ] || fail "--upgrade-height-offset is required"
[ -d "$SOURCE_DATA" ] || fail "--source-data $SOURCE_DATA is not a directory"
SOURCE_DATA=$(cd "$SOURCE_DATA" && pwd)
SOURCE_CONFIG=$(dirname "$SOURCE_DATA")/config
[ -n "$SOURCE_GENESIS" ] || SOURCE_GENESIS=$SOURCE_CONFIG/genesis.json
[ -r "$SOURCE_GENESIS" ] || fail "source genesis $SOURCE_GENESIS is not readable"
OLD_BIN=$(command -v "$OLD_BIN")
NEW_BIN=$(command -v "$NEW_BIN")
[ "$(sha256sum < "$OLD_BIN")" != "$(sha256sum < "$NEW_BIN")" ] || fail "--old-bin and --new-bin are the same binary"
for address in "${PRECOMPILES[@]}"; do
    [ -n "${VIEWS[$address]:-}" ] || fail "no view call for $address; pass --view $address=0x<selector+args> from its ABI"
done
if [ -e "$WORK" ]; then
    [ -d "$WORK" ] && [ -z "$(ls -A "$WORK")" ] || fail "--work $WORK must be empty or absent"
fi
mkdir -p "$WORK"
WORK=$(cd "$WORK" && pwd)
SRC_HOME=$WORK/export-home
HOME_DIR=$WORK/node-home
LOG_DIR=$WORK/logs
mkdir -p "$LOG_DIR"

stop_node() {
    if [ -n "$NODE_PID" ] && kill -0 "$NODE_PID" 2>/dev/null; then
        kill -TERM "$NODE_PID" 2>/dev/null || true
        for _ in $(seq 1 30); do
            kill -0 "$NODE_PID" 2>/dev/null || break
            sleep 1
        done
        kill -KILL "$NODE_PID" 2>/dev/null || true
        wait "$NODE_PID" 2>/dev/null || true
    fi
    NODE_PID=""
}
trap stop_node EXIT

start_node() {
    local bin=$1 logfile=$2
    "$bin" start --home "$HOME_DIR" >"$logfile" 2>&1 &
    NODE_PID=$!
    log "started $(basename "$bin") pid $NODE_PID, log $logfile"
}

height() {
    curl -sf --max-time 5 "http://127.0.0.1:$RPC_PORT/status" 2>/dev/null \
        | jq -r '.result.sync_info.latest_block_height // .sync_info.latest_block_height // empty' 2>/dev/null || true
}

wait_height_above() {
    local target=$1 deadline=$((SECONDS + WAIT_SECONDS)) current
    while [ "$SECONDS" -lt "$deadline" ]; do
        current=$(height)
        if [[ "$current" =~ ^[0-9]+$ ]] && [ "$current" -gt "$target" ]; then
            echo "$current"
            return 0
        fi
        [ -z "$NODE_PID" ] || kill -0 "$NODE_PID" 2>/dev/null || fail "node exited before height $((target + 1)); see $LOG_DIR"
        sleep 1
    done
    fail "height did not pass $target within ${WAIT_SECONDS}s; see $LOG_DIR"
}

# Broadcasts with the node's paxd, then waits for the transaction to be committed successfully.
run_tx() {
    local out hash deadline result
    out=$("$CURRENT_BIN" tx "$@" --from "$KEY_NAME" --chain-id "$COSMOS_CHAIN_ID" --keyring-backend test \
        --home "$HOME_DIR" --node "$NODE" --fees "$FEES" --gas "$GAS" -b sync -y -o json 2>&1) \
        || fail "tx $* failed to broadcast: $out"
    out=$(printf '%s\n' "$out" | grep -E '^\{' | tail -n 1)
    [ "$(printf '%s' "$out" | jq -r '.code // 0')" = "0" ] || fail "tx $* rejected: $out"
    hash=$(printf '%s' "$out" | jq -r '.txhash')
    [[ "$hash" =~ ^[0-9A-Fa-f]{64}$ ]] || fail "tx $* returned no hash: $out"
    deadline=$((SECONDS + 120))
    while [ "$SECONDS" -lt "$deadline" ]; do
        if result=$("$CURRENT_BIN" q tx "$hash" --node "$NODE" -o json 2>/dev/null); then
            [ "$(printf '%s' "$result" | jq -r '.code // 0')" = "0" ] \
                || fail "tx $* failed in block: $(printf '%s' "$result" | jq -c '{code, codespace, raw_log}')"
            log "tx $1 $2 committed at height $(printf '%s' "$result" | jq -r '.height')"
            return 0
        fi
        sleep 1
    done
    fail "tx $* ($hash) was not committed within 120s"
}

set_toml() {
    # set_toml FILE SECTION KEY VALUE — replaces KEY inside [SECTION] (or at top level when SECTION is "").
    local file=$1 section=$2 key=$3 value=$4
    if [ -z "$section" ]; then
        sed -i "0,/^\[/ s|^${key} = .*|${key} = ${value}|" "$file"
    else
        sed -i "/^\[${section}\]/,/^\[/ s|^${key} = .*|${key} = ${value}|" "$file"
    fi
}

is_uint() {
    [[ "$1" =~ ^[0-9]{1,17}$ ]]
}

# --- 1. export mainnet state with the old binary ------------------------------------------------
log "copying $SOURCE_DATA into $SRC_HOME (the source node must be stopped)"
"$OLD_BIN" init export-source --chain-id "$COSMOS_CHAIN_ID" --home "$SRC_HOME" --overwrite >/dev/null 2>&1 \
    || fail "old binary could not initialise $SRC_HOME"
cp "$SOURCE_GENESIS" "$SRC_HOME/config/genesis.json"
for file in app.toml config.toml; do
    if [ -r "$SOURCE_CONFIG/$file" ]; then
        cp "$SOURCE_CONFIG/$file" "$SRC_HOME/config/$file"
    fi
done
rm -rf "$SRC_HOME/data"
cp -a --reflink=auto "$SOURCE_DATA" "$SRC_HOME/data"

log "exporting state with $OLD_BIN"
EXPORT_RAW=$WORK/export.raw
"$OLD_BIN" export --home "$SRC_HOME" >"$EXPORT_RAW" 2>&1 || fail "export failed; see $EXPORT_RAW"
# cobra prints the exported document on stderr next to the log lines; it is the one sorted
# JSON line that carries app_state.
EXPORTED=$WORK/exported-genesis.json
grep -E '^\{"app_hash"' "$EXPORT_RAW" | tail -n 1 >"$EXPORTED"
jq -e --arg chain "$COSMOS_CHAIN_ID" '.chain_id == $chain and (.app_state | type == "object")' "$EXPORTED" >/dev/null \
    || fail "export did not produce a $COSMOS_CHAIN_ID genesis; see $EXPORT_RAW"
rm -rf "$SRC_HOME/data"
EXPORT_HEIGHT=$(jq -r '.initial_height' "$EXPORTED")
is_uint "$EXPORT_HEIGHT" || fail "exported initial height '$EXPORT_HEIGHT' is not a height"
log "exported state at height $EXPORT_HEIGHT"

# --- 2. disposable single-validator home --------------------------------------------------------
"$OLD_BIN" init rehearsal --chain-id "$COSMOS_CHAIN_ID" --home "$HOME_DIR" --overwrite >/dev/null 2>&1 \
    || fail "old binary could not initialise $HOME_DIR"
"$OLD_BIN" keys add "$KEY_NAME" --keyring-backend test --home "$HOME_DIR" --output json >/dev/null 2>&1 \
    || fail "could not create the rehearsal key"
KEY_ADDRESS=$("$OLD_BIN" keys show "$KEY_NAME" -a --keyring-backend test --home "$HOME_DIR")
CONS_ADDRESS=$("$OLD_BIN" tendermint show-address --home "$HOME_DIR")
PV_ADDRESS=$(jq -r '.address' "$HOME_DIR/config/priv_validator_key.json")
PV_PUBKEY=$(jq -r '.pub_key.value' "$HOME_DIR/config/priv_validator_key.json")
[ "$(jq -r '.pub_key.type' "$HOME_DIR/config/priv_validator_key.json")" = "tendermint/PubKeyEd25519" ] \
    || fail "rehearsal validator key is not Ed25519"

GENESIS=$HOME_DIR/config/genesis.json
BOND_DENOM=$(jq -r '.app_state.staking.params.bond_denom' "$EXPORTED")
[ "$BOND_DENOM" = "uhpx" ] || fail "unexpected bond denom $BOND_DENOM"
KEPT=$(jq -r '.app_state.staking.last_validator_powers | sort_by(.address) | .[0].address' "$EXPORTED")
KEPT_POWER=$(jq -r --arg v "$KEPT" '.app_state.staking.last_validator_powers[] | select(.address == $v) | .power' "$EXPORTED")
KEPT_TOKENS=$(jq -r --arg v "$KEPT" '.app_state.staking.validators[] | select(.operator_address == $v) | .tokens' "$EXPORTED")
MOVED_TOKENS=$(jq -r --arg v "$KEPT" '[.app_state.staking.validators[]
    | select(.operator_address != $v and .status == "BOND_STATUS_BONDED") | .tokens]
    | if length == 0 then "0" else join("+") end' "$EXPORTED")
BONDED_POOL=$(jq -r '.app_state.auth.accounts[] | select(.name == "bonded_tokens_pool") | .base_account.address' "$EXPORTED")
NOT_BONDED_POOL=$(jq -r '.app_state.auth.accounts[] | select(.name == "not_bonded_tokens_pool") | .base_account.address' "$EXPORTED")
pool_balance() {
    jq -r --arg a "$1" --arg d "$BOND_DENOM" \
        '[.app_state.bank.balances[] | select(.address == $a) | .coins[] | select(.denom == $d) | .amount] | .[0] // "0"' "$EXPORTED"
}
BONDED_BALANCE=$(pool_balance "$BONDED_POOL")
NOT_BONDED_BALANCE=$(pool_balance "$NOT_BONDED_POOL")
MIN_DEPOSIT=$(jq -r --arg d "$BOND_DENOM" '[.app_state.gov.deposit_params.min_deposit[] | select(.denom == $d) | .amount] | .[0] // "0"' "$EXPORTED")
for value in "$KEPT_POWER" "$KEPT_TOKENS" "$BONDED_BALANCE" "$NOT_BONDED_BALANCE" "$MIN_DEPOSIT"; do
    is_uint "$value" || fail "genesis amount '$value' is outside the range this script handles exactly"
done
MOVED=0
IFS=+ read -r -a moved_parts <<<"$MOVED_TOKENS"
for part in "${moved_parts[@]}"; do
    is_uint "$part" || fail "validator tokens '$part' are outside the range this script handles exactly"
    MOVED=$((MOVED + part))
done
[ "$MOVED" -le "$BONDED_BALANCE" ] || fail "bonded pool $BONDED_BALANCE holds less than the unbonded tokens $MOVED"
NEW_BONDED=$((BONDED_BALANCE - MOVED))
NEW_NOT_BONDED=$((NOT_BONDED_BALANCE + MOVED))
DELEGATION=$((KEPT_TOKENS * 9))
FEE_AMOUNT=${FEES%uhpx}
FUNDING=$((DELEGATION + MIN_DEPOSIT + FEE_AMOUNT * 10))
log "keeping validator $KEPT (power $KEPT_POWER) under consensus key $CONS_ADDRESS; unbonding $MOVED uhpx of the others"

jq --arg kept "$KEPT" --arg pv_address "$PV_ADDRESS" --arg pv_pubkey "$PV_PUBKEY" --arg cons "$CONS_ADDRESS" \
    --arg bonded_pool "$BONDED_POOL" --arg not_bonded_pool "$NOT_BONDED_POOL" --arg denom "$BOND_DENOM" \
    --arg new_bonded "$NEW_BONDED" --arg new_not_bonded "$NEW_NOT_BONDED" --arg power "$KEPT_POWER" \
    --arg start "$EXPORT_HEIGHT" --arg voting "${VOTING_SECONDS}s" --arg expedited "$((VOTING_SECONDS / 2))s" '
    def set_pool($address; $amount):
        if any(.app_state.bank.balances[]; .address == $address) then
            .app_state.bank.balances |= map(if .address == $address
                then .coins = ([.coins[] | select(.denom != $denom)] + [{"denom": $denom, "amount": $amount}]
                    | map(select(.amount != "0")) | sort_by(.denom))
                else . end)
        else
            .app_state.bank.balances += [{"address": $address, "coins": [{"denom": $denom, "amount": $amount}]}]
        end;
    .app_state.staking.validators |= map(
        if .operator_address == $kept then .consensus_pubkey.key = $pv_pubkey
        elif .status == "BOND_STATUS_BONDED" then .status = "BOND_STATUS_UNBONDED" | .jailed = true
        else . end)
    | .app_state.staking.last_validator_powers = [{"address": $kept, "power": $power}]
    | .app_state.staking.last_total_power = $power
    | .validators = [{"address": $pv_address, "pub_key": {"type": "tendermint/PubKeyEd25519", "value": $pv_pubkey},
        "power": $power, "name": "rehearsal"}]
    | set_pool($bonded_pool; $new_bonded)
    | set_pool($not_bonded_pool; $new_not_bonded)
    | .app_state.slashing.signing_infos = ([.app_state.slashing.signing_infos[]? | select(.address != $cons)]
        + [{"address": $cons, "validator_signing_info": {"address": $cons, "start_height": $start,
            "index_offset": "0", "jailed_until": "1970-01-01T00:00:00Z", "tombstoned": false,
            "missed_blocks_counter": "0"}}])
    | .app_state.gov.voting_params.voting_period = $voting
    | .app_state.gov.voting_params.expedited_voting_period = $expedited
    | if (.app_state.oracle.params? // null) != null
        then .app_state.oracle.params.min_valid_per_window = "0.000000000000000000" else . end
    | .app_state.bank.supply = []
' "$EXPORTED" >"$GENESIS"
"$OLD_BIN" add-genesis-account "$KEY_ADDRESS" "${FUNDING}${BOND_DENOM}" --home "$HOME_DIR" >/dev/null 2>&1 \
    || fail "could not fund the rehearsal account"
jq '.app_state.bank.supply = []' "$GENESIS" >"$GENESIS.tmp" && mv "$GENESIS.tmp" "$GENESIS"
"$OLD_BIN" validate-genesis --home "$HOME_DIR" >"$LOG_DIR/validate-genesis.log" 2>&1 \
    || fail "rewritten genesis does not validate; see $LOG_DIR/validate-genesis.log"
KEPT_ACCOUNT=$(jq -r --arg v "$KEPT" '.app_state.staking.validators[] | select(.operator_address == $v) | .operator_address' "$GENESIS")
[ "$KEPT_ACCOUNT" = "$KEPT" ] || fail "kept validator vanished from genesis"

CONFIG=$HOME_DIR/config/config.toml
APP=$HOME_DIR/config/app.toml
set_toml "$CONFIG" "" mode '"validator"'
set_toml "$CONFIG" rpc laddr "\"tcp://127.0.0.1:${RPC_PORT}\""
set_toml "$CONFIG" rpc pprof-laddr '""'
set_toml "$CONFIG" p2p laddr "\"tcp://127.0.0.1:${P2P_PORT}\""
set_toml "$CONFIG" p2p external-address '""'
set_toml "$CONFIG" p2p bootstrap-peers '""'
set_toml "$CONFIG" p2p persistent-peers '""'
set_toml "$CONFIG" p2p pex false
set_toml "$CONFIG" instrumentation prometheus false
set_toml "$APP" api enable false
set_toml "$APP" grpc address "\"127.0.0.1:${GRPC_PORT}\""
set_toml "$APP" grpc-web enable false
set_toml "$APP" grpc-web address "\"127.0.0.1:${GRPC_WEB_PORT}\""
set_toml "$APP" evm http_enabled true
set_toml "$APP" evm http_address '"127.0.0.1"'
set_toml "$APP" evm http_port "$EVM_PORT"
set_toml "$APP" evm ws_enabled false
set_toml "$APP" evm ws_port "$EVM_WS_PORT"
grep -q "^laddr = \"tcp://127.0.0.1:${RPC_PORT}\"" "$CONFIG" || fail "config.toml rpc laddr was not set"
grep -q "^http_port = ${EVM_PORT}$" "$APP" || fail "app.toml evm http_port was not set"

# --- 3. run on the old binary and pass the upgrade ----------------------------------------------
CURRENT_BIN=$OLD_BIN
start_node "$OLD_BIN" "$LOG_DIR/old-bin.log"
FIRST=$(wait_height_above "$((EXPORT_HEIGHT + 1))")
T0=$SECONDS
SECOND=$(wait_height_above "$((FIRST + 4))")
ELAPSED=$((SECONDS - T0))
[ "$ELAPSED" -gt 0 ] || ELAPSED=1
MS_PER_BLOCK=$((ELAPSED * 1000 / (SECOND - FIRST)))
log "chain running at height $SECOND, about ${MS_PER_BLOCK}ms per block"

run_tx staking delegate "$KEPT" "${DELEGATION}${BOND_DENOM}"
CURRENT=$(height)
[[ "$CURRENT" =~ ^[0-9]+$ ]] || fail "node RPC stopped answering after the delegation"
UPGRADE_HEIGHT=$((CURRENT + OFFSET))
[ $((OFFSET * MS_PER_BLOCK)) -gt $(((VOTING_SECONDS + 20) * 1000)) ] \
    || fail "offset $OFFSET blocks (~$((OFFSET * MS_PER_BLOCK / 1000))s) does not outlast the ${VOTING_SECONDS}s vote plus margin; raise --upgrade-height-offset"
log "proposing $UPGRADE_NAME at height $UPGRADE_HEIGHT (current $CURRENT + $OFFSET)"
run_tx gov submit-proposal software-upgrade "$UPGRADE_NAME" --upgrade-height "$UPGRADE_HEIGHT" \
    --upgrade-info "fork rehearsal" --title "Paxeer X $UPGRADE_NAME" --description "Paxeer X fork rehearsal" \
    --deposit "${MIN_DEPOSIT}${BOND_DENOM}"
PROPOSAL_ID=$("$OLD_BIN" q gov proposals --node "$NODE" -o json \
    | jq -r --arg name "$UPGRADE_NAME" '[.proposals[] | select(.content.plan.name == $name)] | last | .proposal_id // empty')
[[ "$PROPOSAL_ID" =~ ^[0-9]+$ ]] || fail "could not find the $UPGRADE_NAME proposal"
run_tx gov vote "$PROPOSAL_ID" yes
deadline=$((SECONDS + VOTING_SECONDS + 120))
STATUS=""
while [ "$SECONDS" -lt "$deadline" ]; do
    STATUS=$("$OLD_BIN" q gov proposal "$PROPOSAL_ID" --node "$NODE" -o json | jq -r '.status')
    case "$STATUS" in
        PROPOSAL_STATUS_PASSED) break ;;
        PROPOSAL_STATUS_REJECTED|PROPOSAL_STATUS_FAILED) fail "proposal $PROPOSAL_ID ended $STATUS" ;;
    esac
    sleep 2
done
[ "$STATUS" = "PROPOSAL_STATUS_PASSED" ] || fail "proposal $PROPOSAL_ID did not pass (status $STATUS)"
PLAN_HEIGHT=$("$OLD_BIN" q upgrade plan --node "$NODE" -o json | jq -r '.height')
[ "$PLAN_HEIGHT" = "$UPGRADE_HEIGHT" ] || fail "upgrade plan height $PLAN_HEIGHT is not $UPGRADE_HEIGHT"
CURRENT=$(height)
[[ "$CURRENT" =~ ^[0-9]+$ ]] && [ "$CURRENT" -lt "$UPGRADE_HEIGHT" ] || fail "proposal passed after the upgrade height; raise --upgrade-height-offset"
log "proposal $PROPOSAL_ID passed; waiting for the old binary to halt at $UPGRADE_HEIGHT"

# --- 4. the old binary must halt at the height ---------------------------------------------------
HALT_PATTERN="UPGRADE \\\\?\"${UPGRADE_NAME//./\\.}\\\\?\" NEEDED at height: ${UPGRADE_HEIGHT}([^0-9]|$)"
deadline=$((SECONDS + WAIT_SECONDS))
until grep -Eq "$HALT_PATTERN" "$LOG_DIR/old-bin.log"; do
    [ "$SECONDS" -lt "$deadline" ] || fail "no UPGRADE NEEDED line within ${WAIT_SECONDS}s; see $LOG_DIR/old-bin.log"
    CURRENT=$(height)
    if [[ "$CURRENT" =~ ^[0-9]+$ ]] && [ "$CURRENT" -ge "$UPGRADE_HEIGHT" ]; then
        fail "old binary committed height $CURRENT >= $UPGRADE_HEIGHT without halting; it already knows $UPGRADE_NAME"
    fi
    sleep 1
done
HALTED_AT=$(height)
[ -z "$HALTED_AT" ] || [ "$HALTED_AT" -lt "$UPGRADE_HEIGHT" ] || fail "old binary reports height $HALTED_AT past the upgrade"
log "old binary halted: $(grep -Eo "$HALT_PATTERN" "$LOG_DIR/old-bin.log" | head -n 1)"
stop_node

# --- 5. resume on the new binary and assert ------------------------------------------------------
CURRENT_BIN=$NEW_BIN
start_node "$NEW_BIN" "$LOG_DIR/new-bin.log"
RESUMED=$(wait_height_above "$((UPGRADE_HEIGHT + 2))")
log "new binary advanced to height $RESUMED"

APPLIED=$("$NEW_BIN" q upgrade applied "$UPGRADE_NAME" --node "$NODE" -o json 2>&1) \
    || fail "q upgrade applied $UPGRADE_NAME failed: $APPLIED"
APPLIED_HEIGHT=$(printf '%s\n' "$APPLIED" | jq -r '.header.height')
[ "$APPLIED_HEIGHT" = "$UPGRADE_HEIGHT" ] || fail "$UPGRADE_NAME applied at $APPLIED_HEIGHT, expected $UPGRADE_HEIGHT"
log "q upgrade applied $UPGRADE_NAME = $APPLIED_HEIGHT"

# The LayerX modules register no query service, so a store's presence is read from the module
# version map the upgrade handler writes after InitGenesis of every added module.
for module in "${MODULES[@]}"; do
    VERSION=$("$NEW_BIN" q upgrade module_versions "$module" --node "$NODE" -o json 2>&1) \
        || fail "module_versions $module failed: $VERSION"
    printf '%s\n' "$VERSION" | jq -e --arg m "$module" \
        'any(.module_versions[]?; .name == $m and ((.version | tonumber) > 0))' >/dev/null \
        || fail "module $module is not in the version map after $UPGRADE_NAME: $VERSION"
    log "module $module present (version $(printf '%s\n' "$VERSION" | jq -r '.module_versions[0].version'))"
done

for address in "${PRECOMPILES[@]}"; do
    CALL_BODY=$(jq -cn --arg to "$address" --arg data "${VIEWS[$address]}" \
        '{"jsonrpc":"2.0","id":1,"method":"eth_call","params":[{"to":$to,"data":$data},"latest"]}')
    RESPONSE=$(curl -sf --max-time 10 -H 'content-type: application/json' --data "$CALL_BODY" "http://127.0.0.1:$EVM_PORT") \
        || fail "eth_call to $address got no response from the EVM RPC"
    printf '%s' "$RESPONSE" | jq -e '(.error == null) and (.result | type == "string" and startswith("0x"))' >/dev/null \
        || fail "eth_call to $address reverted: $RESPONSE"
    log "eth_call $address ${VIEWS[$address]} -> $(printf '%s' "$RESPONSE" | jq -r '.result' | cut -c 1-66)"
done
CHAIN_HEX=$(curl -sf --max-time 10 -H 'content-type: application/json' \
    --data '{"jsonrpc":"2.0","id":1,"method":"eth_chainId","params":[]}' "http://127.0.0.1:$EVM_PORT" | jq -r '.result')
[ "$((CHAIN_HEX))" = "$EVM_CHAIN_ID" ] || fail "EVM chain id $CHAIN_HEX is not $EVM_CHAIN_ID"
FINAL=$(wait_height_above "$RESUMED")

stop_node
{
    printf 'upgrade=%s\n' "$UPGRADE_NAME"
    printf 'upgrade_height=%s\n' "$UPGRADE_HEIGHT"
    printf 'export_height=%s\n' "$EXPORT_HEIGHT"
    printf 'final_height=%s\n' "$FINAL"
    printf 'old_bin_sha256=%s\n' "$(sha256sum "$OLD_BIN" | cut -d' ' -f1)"
    printf 'new_bin_sha256=%s\n' "$(sha256sum "$NEW_BIN" | cut -d' ' -f1)"
    printf 'modules=%s\n' "${MODULES[*]}"
} >"$WORK/rehearsal-result.env"
log "rehearsal passed; result in $WORK/rehearsal-result.env"
