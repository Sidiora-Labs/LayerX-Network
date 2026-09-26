#!/usr/bin/env bash
# Offline check of bridge/deploy/checklist.sh, the post-deploy checklist of the
# Paxeer X Network bridge.
#
# The checklist is driven against recorded JSON-RPC answers only: a replay server
# on the loopback interface answers each request from the fixtures under
# bridge/deploy/tests/fixtures/rpc by its method and parameters, and refuses
# anything it has no recording of. No public network is dialled. A fully agreeing
# Ethereum deployment and a fully agreeing Solana deployment pass; a disagreeing
# cap, an unregistered or uncapped native coin, a paused vault, a chain Paxeer
# carries no registration for, a placeholder still in place, a Solana asset
# account pairing Sidiora's asset id with another mint or Sidiora's mint with
# another id, and a Sidiora pair the upgrade handler has not registered all fail,
# naming the value at fault as the first mismatch. The replay log proves that the
# checklist only ever called read methods.
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
DEPLOY_DIR=$(cd "$SCRIPT_DIR/.." && pwd)
REPO_ROOT=$(cd "$DEPLOY_DIR/../.." && pwd)
CHECKLIST="$DEPLOY_DIR/checklist.sh"
FIXTURES="$SCRIPT_DIR/fixtures"
GOVERNANCE_AUTHORITY=pax10d07y265gmmuvt4z0w9aw880jnsr700jxdwa9m
PRECOMPILE=0x0000000000000000000000000000000000001016
ETHEREUM_VAULT=0x7a3e5c81b04d296f8e1a7c35d92b6f04e8c1a37d
SOLANA_CHAIN_ID=91600046870081
WRAPPED_SOL_ID=0xcf996523b5d068a26f0aa8a116602fe5033ee3a1
SIDIORA_ID=0x21f7b20a555199fa73a238b1a91fd0f549068fee
ZERO_ADDRESS=0x0000000000000000000000000000000000000000
READ_METHODS='eth_chainId eth_getCode eth_call getGenesisHash getAccountInfo'

fail() {
    printf 'checklist-check: error: %s\n' "$*" >&2
    exit 1
}

for tool in jq curl cast go base64 od python3; do
    command -v "$tool" > /dev/null 2>&1 || fail "$tool is required and is not on the PATH"
done
[ -r "$CHECKLIST" ] || fail "$CHECKLIST is missing"
for fixture in attestors.json chains/ethereum/config.json chains/solana/config.json \
    records/ethereum.json records/solana.json rpc/ethereum.json rpc/solana.json \
    rpc/solana-sidiora-pairings.json; do
    [ -r "$FIXTURES/$fixture" ] || fail "the fixture $FIXTURES/$fixture is missing"
done

# Nothing an operator happens to have exported may reach the checklist: every
# endpoint below is the loopback replay server.
unset "${!PAXEER_BRIDGE_@}"

WORK=$(mktemp -d)
SERVER_PID=""
cleanup() {
    [ -z "$SERVER_PID" ] || kill "$SERVER_PID" 2> /dev/null || true
    rm -rf "$WORK"
}
trap cleanup EXIT
chmod 0700 "$WORK"

# The fixture configurations are the committed ones with only the values an
# operator fills in at deploy time set: the owner, the attestor set and the
# Solana program id. Anything else drifting would make the fixtures describe a
# chain the bridge does not carry.
drift() {
    jq -S 'del(.owner, .attestors, .solana.program_id)' "$1"
}
for pair in "ethereum:$REPO_ROOT/bridge/evm/chains/ethereum/config.json" \
    "solana:$REPO_ROOT/bridge/solana/chains/solana/config.json"; do
    chain=${pair%%:*}
    committed=${pair#*:}
    [ "$(drift "$committed")" = "$(drift "$FIXTURES/chains/$chain/config.json")" ] \
        || fail "the $chain fixture configuration has drifted from $committed"
done

# The replay server re-reads the fixture file on every request, so each case
# below only rewrites that one file.
RPC="$WORK/rpc.json"
CALLS="$WORK/calls.log"
cat > "$WORK/replay.py" << 'PY'
import http.server
import json
import sys

fixture, calls, port_file = sys.argv[1], sys.argv[2], sys.argv[3]


class Replay(http.server.BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers.get("Content-Length", "0"))))
        endpoint = self.path.strip("/")
        with open(calls, "a") as log:
            log.write("%s %s\n" % (endpoint, body.get("method")))
        with open(fixture) as handle:
            recorded = json.load(handle)["endpoints"].get(endpoint, {}).get("*", [])
        answer = {"jsonrpc": "2.0", "id": body.get("id")}
        for entry in recorded:
            if entry["method"] == body.get("method") and entry["params"] == body.get("params"):
                answer["result"] = entry["result"]
                break
        else:
            answer["error"] = {"code": -32601, "message": "no recording of %s %s" % (
                body.get("method"), json.dumps(body.get("params")))}
        raw = json.dumps(answer).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)


server = http.server.HTTPServer(("127.0.0.1", 0), Replay)
with open(port_file + ".tmp", "w") as handle:
    handle.write(str(server.server_address[1]))
import os
os.rename(port_file + ".tmp", port_file)
server.serve_forever()
PY
echo '{"endpoints": {}}' > "$RPC"
python3 "$WORK/replay.py" "$RPC" "$CALLS" "$WORK/port" &
SERVER_PID=$!
for _ in $(seq 1 100); do
    [ -s "$WORK/port" ] && break
    kill -0 "$SERVER_PID" 2> /dev/null || fail "the replay server did not start"
    sleep 0.1
done
[ -s "$WORK/port" ] || fail "the replay server did not report its port"
ENDPOINT="http://127.0.0.1:$(cat "$WORK/port")"

STATUS=0
attempt() {
    STATUS=0
    : > "$CALLS"
    "$@" > "$WORK/last.log" 2>&1 || STATUS=$?
}

quote() { sed -e 's/^/    /' "$WORK/last.log" >&2; }

only_reads() {
    local label=$1 method
    while read -r _ method; do
        case " $READ_METHODS " in
        *" $method "*) ;;
        *) quote; fail "$label called $method, which is not a read" ;;
        esac
    done < "$CALLS"
}

passes() {
    local label=$1
    shift
    attempt "$@"
    only_reads "$label"
    if [ "$STATUS" -ne 0 ]; then
        quote
        fail "$label did not pass (exit $STATUS)"
    fi
    grep -q 'passes all [0-9]* checks' "$WORK/last.log" || {
        quote
        fail "$label passed without its summary"
    }
    if grep -q ' FAIL ' "$WORK/last.log"; then
        quote
        fail "$label passed with a failing check"
    fi
}

# fails asserts a run exits 1 and that its summary names the expected first
# mismatch.
fails() {
    local needle=$1 label=$2
    shift 2
    attempt "$@"
    only_reads "$label"
    if [ "$STATUS" -ne 1 ]; then
        quote
        fail "$label exited $STATUS instead of failing"
    fi
    grep -F 'first mismatch: ' "$WORK/last.log" | grep -qF -- "$needle" || {
        quote
        fail "$label did not name as its first mismatch: $needle"
    }
}

refuses() {
    local needle=$1 label=$2
    shift 2
    attempt "$@"
    if [ "$STATUS" -eq 0 ] || [ "$STATUS" -eq 2 ]; then
        quote
        fail "$label exited $STATUS instead of refusing"
    fi
    grep -qF -- "$needle" "$WORK/last.log" || {
        quote
        fail "$label was refused without naming: $needle"
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

encode() { cast abi-encode "$@"; }
calldata() { cast calldata "$@"; }

# with_call rewrites the recorded answer to one eth_call of an endpoint.
with_call() {
    local file=$1 endpoint=$2 to=$3 data=$4 result=$5
    jq --arg endpoint "$endpoint" --arg to "$to" --arg data "$data" --arg result "$result" '
        (.endpoints[$endpoint]["*"][]
            | select(.method == "eth_call" and .params[0].to == $to and .params[0].data == $data)
            | .result) = $result' "$file" > "$file.next"
    cmp -s "$file" "$file.next" && fail "no recorded eth_call of $data at $to on $endpoint to rewrite"
    mv "$file.next" "$file"
}

# with_account rewrites the recorded data of one Solana account.
with_account() {
    local file=$1 account=$2 data=$3
    jq --arg account "$account" --arg data "$data" '
        (.endpoints.solana["*"][]
            | select(.method == "getAccountInfo" and .params[0] == $account)
            | .result.value.data[0]) = $data' "$file" > "$file.next"
    cmp -s "$file" "$file.next" && fail "no recorded account $account to rewrite"
    mv "$file.next" "$file"
}

fixture() {
    cp "$FIXTURES/rpc/$1.json" "$RPC"
    printf '%s' "$RPC"
}

EVM_ENVIRONMENT=(
    "PAXEER_BRIDGE_EVM_CHAINS_ROOT=$FIXTURES/chains"
    "PAXEER_BRIDGE_ETHEREUM_RPC_URL=$ENDPOINT/ethereum"
    "PAXEER_BRIDGE_PAXEER_RPC_URL=$ENDPOINT/paxeer"
    "PAXEER_BRIDGE_DEPLOYMENT_RECORD=$FIXTURES/records/ethereum.json"
    "PAXEER_BRIDGE_GOVERNANCE_AUTHORITY=$GOVERNANCE_AUTHORITY"
    "PAXEER_BRIDGE_ATTESTOR_MANIFEST=$FIXTURES/attestors.json"
)
SOLANA_ENVIRONMENT=(
    "PAXEER_BRIDGE_SOLANA_CHAINS_ROOT=$FIXTURES/chains"
    "PAXEER_BRIDGE_SOLANA_RPC_URL=$ENDPOINT/solana"
    "PAXEER_BRIDGE_PAXEER_RPC_URL=$ENDPOINT/paxeer"
    "PAXEER_BRIDGE_DEPLOYMENT_RECORD=$FIXTURES/records/solana.json"
    "PAXEER_BRIDGE_GOVERNANCE_AUTHORITY=$GOVERNANCE_AUTHORITY"
    "PAXEER_BRIDGE_ATTESTOR_MANIFEST=$FIXTURES/attestors.json"
)
evm() { env "${EVM_ENVIRONMENT[@]}" "$@" bash "$CHECKLIST" ethereum; }
solana() { env "${SOLANA_ENVIRONMENT[@]}" "$@" bash "$CHECKLIST" solana; }

# Arguments, tools and environment.
answers_usage 'the checklist with no chain' bash "$CHECKLIST"
answers_usage 'the checklist with two chains' bash "$CHECKLIST" ethereum solana
answers_usage 'the checklist with an option' bash "$CHECKLIST" --broadcast
refuses 'is not a chain name' 'the checklist with a path for a chain' bash "$CHECKLIST" ../../etc
refuses 'is not a bridge chain' 'the checklist of a chain the bridge does not carry' \
    env "PAXEER_BRIDGE_EVM_CHAINS_ROOT=$FIXTURES/chains" bash "$CHECKLIST" sepolia
mkdir -p "$WORK/bin"
for tool in bash env jq curl go base64 od sed tr grep cat mktemp chmod rm find wc tail dirname; do
    ln -s "$(command -v "$tool")" "$WORK/bin/$tool"
done
refuses 'cast is required and is not on the PATH' 'the checklist without cast' \
    env "PATH=$WORK/bin" "${EVM_ENVIRONMENT[@]}" bash "$CHECKLIST" ethereum
fixture ethereum > /dev/null
for variable in PAXEER_BRIDGE_ETHEREUM_RPC_URL PAXEER_BRIDGE_PAXEER_RPC_URL \
    PAXEER_BRIDGE_DEPLOYMENT_RECORD PAXEER_BRIDGE_GOVERNANCE_AUTHORITY; do
    environment=()
    for assignment in "${EVM_ENVIRONMENT[@]}"; do
        [ "${assignment%%=*}" = "$variable" ] || environment+=("$assignment")
    done
    refuses "$variable is required and is not set" "the checklist without $variable" \
        env "${environment[@]}" bash "$CHECKLIST" ethereum
    [ ! -s "$CALLS" ] || fail "the checklist without $variable reached an endpoint"
done

# A fully agreeing deployment on each side of the bridge passes.
fixture ethereum > /dev/null
passes 'an agreeing Ethereum deployment' evm
first=$(grep -E '^checklist: ethereum (ok|FAIL) ' "$WORK/last.log" \
    | grep -vE ' (ok|FAIL) (placeholders in |deployment record |governance bodies:|chain id:)' | head -n 1)
case $first in
'checklist: ethereum ok ETH registered on the vault:'*) ;;
*) quote; fail "the Ethereum checklist did not check the native coin first: $first" ;;
esac
fixture solana > /dev/null
passes 'an agreeing Solana deployment' solana
grep -qF 'ok Sidiora registration of' "$WORK/last.log" \
    || { quote; fail 'the Solana checklist did not read the Sidiora registration back'; }

# A placeholder stops the run before any endpoint is dialled.
PLACEHOLDER_ROOT="$WORK/placeholder"
mkdir -p "$PLACEHOLDER_ROOT/ethereum"
jq '.owner = "PLACEHOLDER:owner"' "$FIXTURES/chains/ethereum/config.json" > "$PLACEHOLDER_ROOT/ethereum/config.json"
fixture ethereum > /dev/null
fails 'owner=PLACEHOLDER:owner' 'a configuration with its placeholder owner' \
    evm "PAXEER_BRIDGE_EVM_CHAINS_ROOT=$PLACEHOLDER_ROOT"
[ ! -s "$CALLS" ] || fail 'a configuration with a placeholder reached an endpoint'

# A disagreeing cap names the native coin's cap.
file=$(fixture ethereum)
with_call "$file" ethereum "$ETHEREUM_VAULT" "$(calldata 'caps(address)' "$ZERO_ADDRESS")" \
    "$(encode 'f(uint256,uint256)' 250000000000000000001 5000000000000000000000)"
fails 'ETH per-transaction cap on the vault: expected 250000000000000000000, found 250000000000000000001' \
    'a vault whose per-transaction cap disagrees' evm

file=$(fixture ethereum)
with_call "$file" paxeer "$PRECOMPILE" "$(calldata 'getCap(uint64,address)' 1 "$ZERO_ADDRESS")" \
    "$(encode 'f(string,uint256,uint256,uint256)' \
        factory/pax1dzfx9mk4fl9kl2mysjmtvk2xp75ljumk6nynhf/lxb4a29c166774217d0aff9c3170e9ce351277be1c2 \
        4000000000000000000000 250000000000000000000 0)"
fails 'ETH total cap on Paxeer: expected 5000000000000000000000, found 4000000000000000000000' \
    'a Paxeer total cap that disagrees' evm

# An uncapped native coin.
file=$(fixture ethereum)
with_call "$file" ethereum "$ETHEREUM_VAULT" "$(calldata 'caps(address)' "$ZERO_ADDRESS")" \
    "$(encode 'f(uint256,uint256)' 0 0)"
fails 'ETH per-transaction cap on the vault: expected 250000000000000000000, found 0' \
    'an uncapped native coin' evm

# An unregistered native coin, on the vault and on Paxeer.
file=$(fixture ethereum)
with_call "$file" ethereum "$ETHEREUM_VAULT" "$(calldata 'registered(address)' "$ZERO_ADDRESS")" \
    "$(encode 'f(bool)' false)"
fails 'ETH registered on the vault: expected true, found false' 'a native coin the vault has not registered' evm
file=$(fixture ethereum)
with_call "$file" paxeer "$PRECOMPILE" "$(calldata 'getCap(uint64,address)' 1 "$ZERO_ADDRESS")" \
    "$(encode 'f(string,uint256,uint256,uint256)' '' 0 0 0)"
fails 'ETH registration on Paxeer: expected factory/' 'a native coin Paxeer has not registered' evm

# A paused vault.
file=$(fixture ethereum)
with_call "$file" ethereum "$ETHEREUM_VAULT" "$(calldata 'paused()')" "$(encode 'f(bool)' true)"
fails 'vault paused: expected false, found true' 'a paused vault' evm

# A chain Paxeer carries no registration for.
file=$(fixture ethereum)
with_call "$file" paxeer "$PRECOMPILE" "$(calldata 'getChain(uint64)' 1)" \
    "$(encode 'f(bool,address,uint64,bool)' false "$ZERO_ADDRESS" 0 false)"
fails 'Paxeer registration of chain 1: expected registered, found none' \
    'a chain with no Paxeer registration' evm

# A deployed vault whose code is not the recorded code.
file=$(fixture ethereum)
jq '(.endpoints.ethereum["*"][] | select(.method == "eth_getCode") | .result) = "0x6080"' "$file" > "$file.next"
mv "$file.next" "$file"
fails 'vault code hash: expected' 'a vault whose code is not the recorded code' evm

# Solana: Sidiora's asset id is bound to its own mint, and its mint to its id.
PAIRINGS="$FIXTURES/rpc/solana-sidiora-pairings.json"
file=$(fixture solana)
with_account "$file" "$(jq -r '.sidiora_id_on_wrapped_sol.account' "$PAIRINGS")" \
    "$(jq -r '.sidiora_id_on_wrapped_sol.data' "$PAIRINGS")"
fails "Sidiora's asset id $SIDIORA_ID in SOL's asset account" \
    "an asset account pairing Sidiora's id with the wrapped SOL mint" solana
file=$(fixture solana)
with_account "$file" "$(jq -r '.sidiora_mint_under_its_handle.account' "$PAIRINGS")" \
    "$(jq -r '.sidiora_mint_under_its_handle.data' "$PAIRINGS")"
fails "Sidiora's mint in SID's asset account: expected the asset id $SIDIORA_ID" \
    "an asset account pairing Sidiora's mint with another id" solana

# Solana: the Sidiora pair only the upgrade handler registers.
file=$(fixture solana)
with_call "$file" paxeer "$PRECOMPILE" "$(calldata 'getCap(uint64,address)' "$SOLANA_CHAIN_ID" "$SIDIORA_ID")" \
    "$(encode 'f(string,uint256,uint256,uint256)' '' 0 0 0)"
fails 'the Sidiora cap proposal is not executable' 'a Sidiora pair the upgrade handler has not registered' solana

# Solana: a paused program and a disagreeing native cap.
file=$(fixture solana)
account=$(jq -r '.endpoints.solana["*"][] | select(.method == "getAccountInfo") | .params[0]' "$file" | sed -n 2p)
data=$(jq -r --arg account "$account" \
    '.endpoints.solana["*"][] | select(.method == "getAccountInfo" and .params[0] == $account) | .result.value.data[0]' "$file")
paused=$(printf '%s' "$data" | base64 -d | od -An -v -tx1 | tr -d ' \n' | sed -e 's/^\(.\{148\}\)00/\101/' \
    | python3 -c 'import base64, sys; print(base64.b64encode(bytes.fromhex(sys.stdin.read().strip())).decode())')
with_account "$file" "$account" "$paused"
fails 'program paused: expected 00, found 01' 'a paused Solana program' solana
file=$(fixture solana)
with_call "$file" paxeer "$PRECOMPILE" "$(calldata 'getCap(uint64,address)' "$SOLANA_CHAIN_ID" "$WRAPPED_SOL_ID")" \
    "$(encode 'f(string,uint256,uint256,uint256)' \
        factory/pax1dzfx9mk4fl9kl2mysjmtvk2xp75ljumk6nynhf/lxb1ac7d111ed1c020ca9fdc680e800c2438d68ae6e \
        100000000000000 5000000000001 1200000000)"
fails 'SOL per-transaction cap on Paxeer: expected 5000000000000, found 5000000000001' \
    'a Paxeer SOL cap that disagrees' solana

printf 'checklist-check: every case behaves\n'
