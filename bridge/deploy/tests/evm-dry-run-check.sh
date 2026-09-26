#!/usr/bin/env bash
# End-to-end dry run of the EVM deployment of the Paxeer X Network bridge
# against a local anvil node, and the replay of its recorded exchange.
#
# Usage:
#   evm-dry-run-check.sh             the dry run, then the replay of its own
#                                    recording and of the committed fixture
#   evm-dry-run-check.sh --record    the dry run, then its recording replaces
#                                    bridge/deploy/tests/fixtures/evm_dry_run.json
#   evm-dry-run-check.sh --replay    the committed fixture only, without a node
#
# The dry run starts anvil on the loopback interface under the ethereum chain id
# and generates every key it uses for that run alone: the deployer, the owner,
# the depositor, the recipient and the attestor set. It writes a run-local copy
# of bridge/evm/chains/ethereum/config.json whose placeholder owner and
# attestors are replaced by that run's own addresses and nothing else, and a
# run-local attestor-set manifest naming the same set, then:
#   deploys the vault with bridge/deploy/deploy-evm-chain.sh against it and
#   asserts the deployment record names the address, the code hash and the block
#   the node reports;
#   accepts the ownership the deploy script proposed, sets the attestor set and
#   every cap the configuration declares as the owner, and caps a test token;
#   makes a token deposit and a native deposit, and releases each against the
#   threshold of attestor signatures over the outbound preimage, built here byte
#   by byte from bridge/evm/ATTESTATION.md and asserted equal to the vault's own
#   digest; a second release of the same Paxeer burn is refused;
#   opens the chain on a Paxeer side made of the real layerxbridge keeper and
#   precompile, which reads every governance body bridge/deploy/proposals
#   generates for the deployment through that package's DecodeBody under its
#   type URL, executes it and answers the precompile's views, after proving the
#   decoder refuses a body under a wrong type URL and a body with an unknown
#   field, each by name; and runs
#   bridge/deploy/checklist.sh against both, asserting every check passes with
#   the native coin checked first.
#
# Every read the checklist and the dry run's own assertions make passes through
# a recording proxy on the loopback interface. The recording is written in the
# format the relayer's fixtures use - endpoints by name, then phases, then
# method, params and result - beside the run-local configuration, manifest and
# deployment record and the values the run expects. It carries no key material,
# no endpoint (the endpoints are names), no date and no hostname; the run checks
# that before it keeps the file. The recording is then replayed with every node
# stopped: the checklist passes against it and the dry run's assertions hold.
#
# Nothing public is dialled. A missing tool stops the run naming it; nothing is
# skipped.
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
DEPLOY_DIR=$(cd "$SCRIPT_DIR/.." && pwd)
REPO_ROOT=$(cd "$DEPLOY_DIR/../.." && pwd)
EVM_ROOT="$REPO_ROOT/bridge/evm"
DEPLOY="$DEPLOY_DIR/deploy-evm-chain.sh"
CHECKLIST="$DEPLOY_DIR/checklist.sh"
FIXTURE="$SCRIPT_DIR/fixtures/evm_dry_run.json"
CHAIN=ethereum
COMMITTED_CONFIG="$EVM_ROOT/chains/$CHAIN/config.json"
GOVERNANCE_AUTHORITY=pax10d07y265gmmuvt4z0w9aw880jnsr700jxdwa9m
ZERO_ADDRESS=0x0000000000000000000000000000000000000000
OUTBOUND_DOMAIN=PAXEERX_BRIDGE_OUT_V1
RUN_LOCAL_CONFIGURATION="run-local/$CHAIN/config.json"
CHECKLIST_METHODS='eth_chainId eth_getCode eth_call'
READBACK_METHODS='eth_getCode eth_call eth_getBalance'
FUNDING=0x3635c9adc5dea00000
TOKEN_PER_TX=1000000
TOKEN_TOTAL=5000000
TOKEN_MINTED=1000000
TOKEN_DEPOSIT=400000
TOKEN_RELEASE=150000
NATIVE_DEPOSIT=1000000000000000000
NATIVE_RELEASE=400000000000000000

fail() {
    printf 'evm-dry-run-check: error: %s\n' "$*" >&2
    exit 1
}

usage() {
    printf 'usage: evm-dry-run-check.sh [--record | --replay]\n' >&2
    exit 2
}

mode=live
record=0
[ $# -le 1 ] || usage
case ${1:-} in
"") ;;
--record) record=1 ;;
--replay) mode=replay ;;
*) usage ;;
esac

tools=(jq curl cast go python3 base64 od)
[ "$mode" = replay ] || tools=(anvil forge "${tools[@]}")
for tool in "${tools[@]}"; do
    command -v "$tool" > /dev/null 2>&1 || fail "$tool is required for the dry run and is not on the PATH"
done
for file in "$DEPLOY" "$CHECKLIST" "$COMMITTED_CONFIG"; do
    [ -r "$file" ] || fail "$file is missing"
done

# Nothing an operator happens to have exported may reach the scripts: every
# endpoint, key and path below belongs to this run.
unset "${!PAXEER_BRIDGE_@}" ETH_RPC_URL

WORK=$(mktemp -d)
PIDS=()
GO_PACKAGE=""
cleanup() {
    local pid
    for pid in "${PIDS[@]}"; do
        kill "$pid" 2> /dev/null || true
    done
    for pid in "${PIDS[@]}"; do
        wait "$pid" 2> /dev/null || true
    done
    [ -z "$GO_PACKAGE" ] || rm -rf "$GO_PACKAGE"
    rm -rf "$WORK"
}
trap cleanup EXIT
chmod 0700 "$WORK"

lower() { printf '%s' "$1" | tr '[:upper:]' '[:lower:]'; }

# stop kills one background process this run started and forgets it.
stop() {
    local target=$1 pid kept=()
    kill "$target" 2> /dev/null || true
    wait "$target" 2> /dev/null || true
    for pid in "${PIDS[@]}"; do
        [ "$pid" = "$target" ] || kept+=("$pid")
    done
    PIDS=("${kept[@]}")
}

wait_for_file() {
    local file=$1 pid=$2 label=$3 log=$4
    for _ in $(seq 1 1200); do
        [ -s "$file" ] && return 0
        kill -0 "$pid" 2> /dev/null || {
            sed -e 's/^/    /' "$log" >&2
            fail "$label stopped before it reported its port"
        }
        sleep 0.1
    done
    fail "$label did not report its port"
}

# The loopback server records through to the routes it is given, or replays a
# fixture, answering JSON-RPC by endpoint name, method and params.
cat > "$WORK/server.py" << 'PY'
import http.server
import json
import os
import sys
import urllib.request

mode = sys.argv[1]
if mode == "record":
    routes_file, log_file, phase_file, port_file = sys.argv[2:6]
else:
    fixture_file, log_file, port_file = sys.argv[2:5]
opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))


def replay(endpoint, request):
    with open(fixture_file) as handle:
        phases = json.load(handle)["endpoints"].get(endpoint, {})
    answer = {"jsonrpc": "2.0", "id": request.get("id")}
    for entries in phases.values():
        for entry in entries:
            if entry["method"] == request.get("method") and entry["params"] == request.get("params", []):
                answer["result"] = entry["result"]
                return answer
    answer["error"] = {"code": -32601, "message": "no recording of %s %s" % (
        request.get("method"), json.dumps(request.get("params")))}
    return answer


class Server(http.server.BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_POST(self):
        raw = self.rfile.read(int(self.headers.get("Content-Length", "0")))
        endpoint = self.path.strip("/")
        body = json.loads(raw)
        requests = body if isinstance(body, list) else [body]
        if mode == "record":
            with open(routes_file) as handle:
                routes = json.load(handle)
            if endpoint in routes:
                upstream = urllib.request.Request(routes[endpoint], data=raw,
                                                  headers={"Content-Type": "application/json"})
                with opener.open(upstream, timeout=120) as response:
                    answer = json.loads(response.read())
            else:
                answer = {"jsonrpc": "2.0", "id": None,
                          "error": {"code": -32601, "message": "no route to %s" % endpoint}}
            replies = answer if isinstance(answer, list) else [answer]
            by_id = {json.dumps(reply.get("id")): reply for reply in replies}
            with open(phase_file) as handle:
                phase = handle.read().strip()
            with open(log_file, "a") as log:
                for request in requests:
                    reply = by_id.get(json.dumps(request.get("id")), replies[0])
                    entry = {"endpoint": endpoint, "phase": phase, "method": request.get("method"),
                             "params": request.get("params", [])}
                    if "result" in reply:
                        entry["result"] = reply["result"]
                    else:
                        entry["error"] = reply.get("error")
                    log.write(json.dumps(entry) + "\n")
        else:
            replies = [replay(endpoint, request) for request in requests]
            answer = replies if isinstance(body, list) else replies[0]
            with open(log_file, "a") as log:
                for request in requests:
                    log.write(json.dumps({"endpoint": endpoint, "method": request.get("method")}) + "\n")
        out = json.dumps(answer).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(out)))
        self.end_headers()
        self.wfile.write(out)


server = http.server.HTTPServer(("127.0.0.1", 0), Server)
with open(port_file + ".tmp", "w") as handle:
    handle.write(str(server.server_address[1]))
os.rename(port_file + ".tmp", port_file)
server.serve_forever()
PY

# rpc sends one JSON-RPC request to an endpoint URL and prints its result.
rpc() {
    local url=$1 method=$2 params=$3 response
    jq -cn --arg method "$method" --argjson params "$params" \
        '{jsonrpc: "2.0", id: 1, method: $method, params: $params}' > "$WORK/request.json"
    response=$(curl --silent --show-error --fail --max-time 60 --noproxy '*' \
        --header 'Content-Type: application/json' --data-binary @"$WORK/request.json" "$url") \
        || fail "$url did not answer $method"
    if jq -e 'has("error")' <<< "$response" > /dev/null; then
        fail "$method answered with an error: $(jq -c '.error' <<< "$response")"
    fi
    jq -e 'has("result")' <<< "$response" > /dev/null || fail "$method answered without a result"
    jq -c '.result' <<< "$response"
}

# view calls a view of a contract through an endpoint URL and prints the
# decoded values, one per line.
view() {
    local url=$1 to=$2 signature=$3 returns=$4 data params out
    shift 4
    data=$(cast calldata "$signature" "$@") || fail "$signature cannot be encoded"
    params=$(jq -cn --arg to "$(lower "$to")" --arg data "$data" '[{to: $to, data: $data}, "latest"]')
    out=$(rpc "$url" eth_call "$params" | jq -r .)
    cast abi-decode "f()($returns)" "$out" | sed -e 's/ \[[^]]*\]//g' -e 's/^"\(.*\)"$/\1/' \
        || fail "$signature returned $out, which is not ($returns)"
}

code_at() {
    local url=$1 address=$2 block=$3
    rpc "$url" eth_getCode "$(jq -cn --arg address "$(lower "$address")" --arg block "$block" '[$address, $block]')" \
        | jq -r .
}

expect() {
    local label=$1 expected=$2 found=$3
    [ "$expected" = "$found" ] || fail "$label: expected $expected, found $found"
    printf 'evm-dry-run-check: %s: %s\n' "$label" "$found"
}

# assert_readback proves, through one endpoint, that the deployment record
# names the address, the code hash and the block the node reports, and that the
# deposits and releases of the run left the vault in the state they must.
assert_readback() {
    local url=$1 record=$2 expectations=$3 vault block code token recipient nullifier
    local -a nullifiers
    vault=$(jq -r '.vault' "$record")
    block=$(jq -r '.block' "$record")
    expect "vault in the record" "$(lower "$(jq -r '.vault' "$expectations")")" "$(lower "$vault")"
    code=$(code_at "$url" "$vault" latest)
    if [ -z "$code" ] || [ "$code" = 0x ]; then
        fail "the node reports no code at $vault"
    fi
    expect "code hash the node reports for $vault" "$(jq -r '.code_hash' "$record")" "$(cast keccak "$code")"
    expect "code at $vault before block $block" 0x "$(code_at "$url" "$vault" "$(printf '0x%x' "$((block - 1))")")"
    expect "code at $vault in block $block" "$code" "$(code_at "$url" "$vault" "$(printf '0x%x' "$block")")"

    token=$(jq -r '.token' "$expectations")
    recipient=$(jq -r '.recipient' "$expectations")
    expect "deposits the vault counted" "$(jq -r '.deposit_nonce' "$expectations")" \
        "$(view "$url" "$vault" 'depositNonce()' uint64)"
    expect "native coin outstanding" "$(jq -r '.native_outstanding' "$expectations")" \
        "$(view "$url" "$vault" 'outstanding(address)' uint256 "$ZERO_ADDRESS")"
    expect "token outstanding" "$(jq -r '.token_outstanding' "$expectations")" \
        "$(view "$url" "$vault" 'outstanding(address)' uint256 "$token")"
    expect "token the vault holds" "$(jq -r '.token_outstanding' "$expectations")" \
        "$(view "$url" "$token" 'balanceOf(address)' uint256 "$vault")"
    expect "token released to the recipient" "$(jq -r '.recipient_token' "$expectations")" \
        "$(view "$url" "$token" 'balanceOf(address)' uint256 "$recipient")"
    expect "native coin released to the recipient" "$(jq -r '.recipient_native' "$expectations")" \
        "$(cast to-dec "$(rpc "$url" eth_getBalance "$(jq -cn --arg a "$(lower "$recipient")" '[$a, "latest"]')" | jq -r .)")"
    mapfile -t nullifiers < <(jq -r '.nullifiers[]' "$expectations")
    [ "${#nullifiers[@]}" -gt 0 ] || fail "$expectations names no spent nullifier"
    for nullifier in "${nullifiers[@]}"; do
        expect "nullifier $nullifier spent" true "$(view "$url" "$vault" 'nullified(bytes32)' bool "$nullifier")"
    done
}

# run_checklist runs the real checklist against two endpoint URLs and asserts it
# passes every check with the native coin checked first.
run_checklist() {
    local chains_root=$1 chain_url=$2 paxeer_url=$3 record=$4 manifest=$5 log=$6 rpc_variable first status=0
    rpc_variable=$(jq -r '.environment.rpc_url' "$chains_root/$CHAIN/config.json")
    env "PAXEER_BRIDGE_EVM_CHAINS_ROOT=$chains_root" "$rpc_variable=$chain_url" \
        "PAXEER_BRIDGE_PAXEER_RPC_URL=$paxeer_url" "PAXEER_BRIDGE_DEPLOYMENT_RECORD=$record" \
        "PAXEER_BRIDGE_GOVERNANCE_AUTHORITY=$GOVERNANCE_AUTHORITY" "PAXEER_BRIDGE_ATTESTOR_MANIFEST=$manifest" \
        bash "$CHECKLIST" "$CHAIN" > "$log" 2>&1 || status=$?
    if [ "$status" -ne 0 ] || grep -q ' FAIL ' "$log" || ! grep -q 'passes all [0-9]* checks' "$log"; then
        sed -e 's/^/    /' "$log" >&2
        fail "the checklist did not pass every check (exit $status)"
    fi
    first=$(grep -E "^checklist: $CHAIN (ok|FAIL) " "$log" \
        | grep -vE ' (ok|FAIL) (placeholders in |deployment record |governance bodies:|chain id:)' | head -n 1)
    case $first in
    "checklist: $CHAIN ok ETH registered on the vault:"*) ;;
    *)
        sed -e 's/^/    /' "$log" >&2
        fail "the checklist did not check the native coin first: $first"
        ;;
    esac
    printf 'evm-dry-run-check: %s\n' "$(tail -n 1 "$log")"
}

# The fixture's configuration is the committed one with only the placeholders
# an operator fills in replaced.
assert_no_drift() {
    local config=$1
    [ "$(jq -S 'del(.owner, .attestors)' "$COMMITTED_CONFIG")" = "$(jq -S 'del(.owner, .attestors)' "$config")" ] \
        || fail "$config differs from $COMMITTED_CONFIG in more than the owner and the attestors"
    ! jq -r '.owner, .attestors[]' "$config" | grep -q '^PLACEHOLDER:' \
        || fail "$config still carries a placeholder"
}

start_server() {
    local name=$1
    shift
    python3 "$WORK/server.py" "$@" "$WORK/$name.port" > "$WORK/$name.log" 2>&1 &
    PIDS+=("$!")
    wait_for_file "$WORK/$name.port" "$!" "the $name server" "$WORK/$name.log"
    SERVER_PID=$!
    SERVER_URL="http://127.0.0.1:$(cat "$WORK/$name.port")"
}

# replay_fixture asserts the whole recorded exchange of a fixture without a node:
# the checklist passes against it and the dry run's assertions hold, and neither
# asks for anything the recording does not answer.
replay_fixture() {
    local fixture=$1 label=$2 dir method
    [ -r "$fixture" ] || fail "$fixture is missing; run evm-dry-run-check.sh --record to write it"
    jq -e '.endpoints | type == "object"' "$fixture" > /dev/null 2>&1 || fail "$fixture is not a recorded exchange"
    [ "$(jq -r '.chain' "$fixture")" = "$CHAIN" ] || fail "$fixture does not record $CHAIN"
    dir=$(mktemp -d "$WORK/replay.XXXXXX")
    mkdir -p "$dir/chains/$CHAIN"
    jq '.configuration' "$fixture" > "$dir/chains/$CHAIN/config.json"
    jq '.manifest' "$fixture" > "$dir/attestors.json"
    jq '.record' "$fixture" > "$dir/record.json"
    jq '.expectations' "$fixture" > "$dir/expectations.json"
    assert_no_drift "$dir/chains/$CHAIN/config.json"
    : > "$dir/calls.log"
    start_server "replay-${dir##*.}" replay "$fixture" "$dir/calls.log"
    run_checklist "$dir/chains" "$SERVER_URL/$CHAIN" "$SERVER_URL/paxeer" "$dir/record.json" \
        "$dir/attestors.json" "$dir/checklist.log"
    assert_readback "$SERVER_URL/$CHAIN" "$dir/record.json" "$dir/expectations.json"
    stop "$SERVER_PID"
    while read -r method; do
        case " $CHECKLIST_METHODS $READBACK_METHODS " in
        *" $method "*) ;;
        *) fail "the replay of $label asked for $method, which is not a read" ;;
        esac
    done < <(jq -r '.method' "$dir/calls.log")
    printf 'evm-dry-run-check: the %s replays without a node\n' "$label"
}

if [ "$mode" = replay ]; then
    replay_fixture "$FIXTURE" "committed fixture"
    printf 'evm-dry-run-check: every step behaves\n'
    exit 0
fi

# Keys and addresses of this run only.
cast wallet new --json --number 9 > "$WORK/keys.json" 2> /dev/null || fail "cast did not generate the run's keys"
chmod 0600 "$WORK/keys.json"
key_of() { jq -r --argjson index "$1" '.[$index].private_key' "$WORK/keys.json"; }
address_of() { lower "$(jq -r --argjson index "$1" '.[$index].address' "$WORK/keys.json")"; }
DEPLOYER=$(address_of 0)
DEPLOYER_KEY=$(key_of 0)
OWNER=$(address_of 1)
OWNER_KEY=$(key_of 1)
DEPOSITOR=$(address_of 2)
DEPOSITOR_KEY=$(key_of 2)
RECIPIENT=$(address_of 3)
jq '[.[4:9][] | {address: (.address | ascii_downcase), private_key}] | sort_by(.address)' "$WORK/keys.json" \
    > "$WORK/attestors.keys.json"
mapfile -t ATTESTORS < <(jq -r '.[].address' "$WORK/attestors.keys.json")
mapfile -t ATTESTOR_KEYS < <(jq -r '.[].private_key' "$WORK/attestors.keys.json")
THRESHOLD=$(jq -r '.threshold' "$COMMITTED_CONFIG")
CHAIN_ID=$(jq -r '.chain_id' "$COMMITTED_CONFIG")
[ "$THRESHOLD" -le "${#ATTESTORS[@]}" ] || fail "the run generated fewer attestors than the threshold $THRESHOLD"

# The run-local configuration and manifest.
mkdir -p "$WORK/chains/$CHAIN"
CONFIG="$WORK/chains/$CHAIN/config.json"
jq --arg owner "$OWNER" --argjson attestors "$(printf '%s\n' "${ATTESTORS[@]}" | jq -R . | jq -s .)" \
    '.owner = $owner | .attestors = $attestors' "$COMMITTED_CONFIG" > "$CONFIG"
assert_no_drift "$CONFIG"
MANIFEST="$WORK/attestors.json"
jq '{attestors: .attestors, threshold: .threshold}' "$CONFIG" > "$MANIFEST"
RPC_VARIABLE=$(jq -r '.environment.rpc_url' "$CONFIG")
KEY_VARIABLE=$(jq -r '.environment.deploy_key' "$CONFIG")

# anvil on the loopback interface, under the chain id the configuration names.
ANVIL_PORT=$(python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()')
anvil --host 127.0.0.1 --port "$ANVIL_PORT" --chain-id "$CHAIN_ID" > "$WORK/anvil.log" 2>&1 &
ANVIL_PID=$!
PIDS+=("$ANVIL_PID")
ANVIL_URL="http://127.0.0.1:$ANVIL_PORT"
for _ in $(seq 1 200); do
    curl --silent --fail --noproxy '*' --header 'Content-Type: application/json' \
        --data '{"jsonrpc":"2.0","id":1,"method":"eth_chainId","params":[]}' "$ANVIL_URL" > /dev/null 2>&1 && break
    kill -0 "$ANVIL_PID" 2> /dev/null || {
        sed -e 's/^/    /' "$WORK/anvil.log" >&2
        fail "anvil stopped before it answered"
    }
    sleep 0.1
done
export ETH_RPC_URL="$ANVIL_URL"
expect "chain id anvil answers" "$CHAIN_ID" "$(cast chain-id)"
for account in "$DEPLOYER" "$OWNER" "$DEPOSITOR"; do
    cast rpc anvil_setBalance "$account" "$FUNDING" > /dev/null || fail "anvil did not fund $account"
done

# send broadcasts one transaction and asserts its receipt succeeded.
send() {
    local key=$1 out
    shift
    out=$(cast send --private-key "$key" --json "$@" 2> "$WORK/send.log" < /dev/null) || {
        sed -e 's/^/    /' "$WORK/send.log" >&2
        fail "the transaction $* was not accepted"
    }
    [ "$(jq -r '.status' <<< "$out")" = 0x1 ] || fail "the transaction $* reverted"
    printf '%s' "$out"
}

# The deployment, through the real deploy script. It runs from bridge/evm: the
# pinned forge resolves the deploy script's relative Foundry script path against
# the working directory, not against --root.
RECORD="$WORK/record.json"
if ! (cd "$EVM_ROOT" && env "PAXEER_BRIDGE_EVM_CHAINS_ROOT=$WORK/chains" "$RPC_VARIABLE=$ANVIL_URL" \
    "$KEY_VARIABLE=$DEPLOYER_KEY" "PAXEER_BRIDGE_DEPLOYMENT_RECORD=$RECORD" bash "$DEPLOY" "$CHAIN") \
    > "$WORK/deploy.log" 2>&1; then
    sed -e 's/^/    /' "$WORK/deploy.log" >&2
    fail "deploy-evm-chain.sh did not deploy the vault"
fi
VAULT=$(lower "$(jq -r '.vault' "$RECORD")")
expect "chain in the record" "$CHAIN" "$(jq -r '.chain' "$RECORD")"
expect "chain id in the record" "$CHAIN_ID" "$(jq -r '.chain_id' "$RECORD")"
expect "deployer in the record" "$DEPLOYER" "$(lower "$(jq -r '.deployer' "$RECORD")")"
expect "ownership in the record" proposed "$(jq -r '.ownership' "$RECORD")"
expect "configured owner in the record" "$OWNER" "$(lower "$(jq -r '.configured_owner' "$RECORD")")"

# The owner takes the vault and sets the attestors and every configured cap.
send "$OWNER_KEY" "$VAULT" 'acceptOwnership()' > /dev/null
expect "vault owner" "$OWNER" "$(lower "$(cast call "$VAULT" 'owner()(address)')")"
send "$OWNER_KEY" "$VAULT" 'setAttestors(address[],uint256)' "[$(IFS=,; printf '%s' "${ATTESTORS[*]}")]" \
    "$THRESHOLD" > /dev/null
while IFS=$'\t' read -r asset per_tx total; do
    send "$OWNER_KEY" "$VAULT" 'setCap(address,uint256,uint256)' "$asset" "$per_tx" "$total" > /dev/null
done < <(jq -r '.assets[] | [.address, .per_tx_cap, .total_cap] | @tsv' "$CONFIG")

# A token of the run, capped on the vault beside the configured native coin.
TOKEN_CODE=$(forge inspect --root "$EVM_ROOT" TestToken bytecode 2> "$WORK/inspect.log" \
    | tr -d '"' | grep -oE '0x[0-9a-fA-F]{40,}' | head -n 1) || true
[ -n "$TOKEN_CODE" ] || {
    sed -e 's/^/    /' "$WORK/inspect.log" >&2
    fail "the test token's bytecode is unreadable"
}
TOKEN=$(lower "$(send "$DEPOSITOR_KEY" --create "$TOKEN_CODE" | jq -r '.contractAddress')")
[[ $TOKEN =~ ^0x[0-9a-f]{40}$ ]] || fail "the test token was not created"
send "$OWNER_KEY" "$VAULT" 'setCap(address,uint256,uint256)' "$TOKEN" "$TOKEN_PER_TX" "$TOKEN_TOTAL" > /dev/null
send "$DEPOSITOR_KEY" "$TOKEN" 'mint(address,uint256)' "$DEPOSITOR" "$TOKEN_MINTED" > /dev/null
send "$DEPOSITOR_KEY" "$TOKEN" 'approve(address,uint256)' "$VAULT" "$TOKEN_DEPOSIT" > /dev/null

# A token deposit and a native deposit.
random32() { od -An -N32 -tx1 /dev/urandom | tr -d ' \n'; }
PAXEER_RECIPIENT="0x$(random32)"
send "$DEPOSITOR_KEY" "$VAULT" 'deposit(address,uint256,bytes32)' "$TOKEN" "$TOKEN_DEPOSIT" "$PAXEER_RECIPIENT" > /dev/null
send "$DEPOSITOR_KEY" --value "$NATIVE_DEPOSIT" "$VAULT" 'depositNative(bytes32)' "$PAXEER_RECIPIENT" > /dev/null

# release pays one Paxeer burn out against the threshold of attestor signatures
# over the outbound preimage, and proves the same burn cannot be paid twice.
DOMAIN_HEX=$(printf '%s' "$OUTBOUND_DOMAIN" | od -An -v -tx1 | tr -d ' \n')
NULLIFIERS=()
release() {
    local asset=$1 amount=$2 nonce=$3 burn preimage digest nonce_hex amount_hex signatures="" index signature nullifier
    burn=$(random32)
    nonce_hex=$(printf '%016x' "$nonce")
    amount_hex=$(cast to-uint256 "$amount")
    preimage="0x${DOMAIN_HEX}$(printf '%064x' "$CHAIN_ID")${VAULT#0x}${burn}${nonce_hex}${RECIPIENT#0x}${asset#0x}${amount_hex#0x}"
    [ "${#preimage}" -eq $((2 + 2 * 185)) ] || fail "the outbound preimage is not 185 bytes"
    digest=$(cast keccak "$preimage")
    expect "outbound digest of burn $nonce" "$digest" \
        "$(cast call "$VAULT" 'releaseDigest(bytes32,uint64,address,address,uint256)(bytes32)' \
            "0x$burn" "$nonce" "$RECIPIENT" "$asset" "$amount")"
    for ((index = 0; index < THRESHOLD; index++)); do
        signature=$(cast wallet sign --no-hash --private-key "${ATTESTOR_KEYS[index]}" "$digest") \
            || fail "attestor ${ATTESTORS[index]} did not sign the digest of burn $nonce"
        [[ $signature =~ ^0x[0-9a-fA-F]{130}$ ]] || fail "attestor ${ATTESTORS[index]} signed with $signature"
        signatures+="${signatures:+,}$signature"
    done
    send "$DEPOSITOR_KEY" "$VAULT" 'release(address,uint256,address,bytes32,uint64,bytes[])' \
        "$asset" "$amount" "$RECIPIENT" "0x$burn" "$nonce" "[$signatures]" > /dev/null
    if cast call --from "$DEPOSITOR" "$VAULT" 'release(address,uint256,address,bytes32,uint64,bytes[])' \
        "$asset" "$amount" "$RECIPIENT" "0x$burn" "$nonce" "[$signatures]" > "$WORK/again.log" 2>&1; then
        fail "the vault would pay burn $nonce a second time"
    fi
    nullifier=$(cast keccak "0x${burn}${nonce_hex}")
    grep -qiE "NullifierUsed|$(cast sig 'NullifierUsed(bytes32)' | sed 's/^0x//')" "$WORK/again.log" || {
        sed -e 's/^/    /' "$WORK/again.log" >&2
        fail "a second payment of burn $nonce was refused for another reason than its spent nullifier"
    }
    printf 'evm-dry-run-check: burn %s released %s of %s against %s signatures; a second release is refused\n' \
        "$nonce" "$amount" "$asset" "$THRESHOLD"
    NULLIFIERS+=("$nullifier")
}
release "$TOKEN" "$TOKEN_RELEASE" 1
release "$ZERO_ADDRESS" "$NATIVE_RELEASE" 2

EXPECTATIONS="$WORK/expectations.json"
jq -n --arg vault "$VAULT" --arg token "$TOKEN" --arg recipient "$RECIPIENT" \
    --arg native_outstanding "$((NATIVE_DEPOSIT - NATIVE_RELEASE))" \
    --arg token_outstanding "$((TOKEN_DEPOSIT - TOKEN_RELEASE))" \
    --arg recipient_token "$TOKEN_RELEASE" --arg recipient_native "$NATIVE_RELEASE" \
    --argjson nullifiers "$(printf '%s\n' "${NULLIFIERS[@]}" | jq -R . | jq -s .)" \
    '{vault: $vault, token: $token, recipient: $recipient, deposit_nonce: "2",
      native_outstanding: $native_outstanding, token_outstanding: $token_outstanding,
      recipient_token: $recipient_token, recipient_native: $recipient_native, nullifiers: $nullifiers}' \
    > "$EXPECTATIONS"

# The Paxeer side: the real layerxbridge keeper executes the governance bodies
# generated for this deployment, and its precompile answers the views.
BODIES="$WORK/bodies"
(cd "$REPO_ROOT" && go run ./bridge/deploy/proposals/cmd/paxeer-bridge-proposals -manifest "$MANIFEST" \
    -authority "$GOVERNANCE_AUTHORITY" -vault "$VAULT" "$CONFIG" "$BODIES") > "$WORK/proposals.log" 2>&1 || {
    sed -e 's/^/    /' "$WORK/proposals.log" >&2
    fail "the governance bodies were not generated"
}
GO_PACKAGE=$(mktemp -d "$SCRIPT_DIR/paxeerside_XXXXXX")
cat > "$GO_PACKAGE/paxeerside_test.go" << 'GO'
package paxeerside

import (
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"math/big"
	"net"
	"net/http"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"sync"
	"testing"

	"github.com/ethereum/go-ethereum/common"
	"github.com/sidiora-labs/paxeer-network/bridge/deploy/proposals"
	bridgetestutil "github.com/sidiora-labs/paxeer-network/modules/layerxbridge/testutil"
	"github.com/sidiora-labs/paxeer-network/modules/layerxbridge/types"
	app "github.com/sidiora-labs/paxeer-network/node"
	"github.com/sidiora-labs/paxeer-network/precompiles/layerxbridge"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

// decode reads one generated body through the generator's own decoder, which
// resolves its type URL and refuses a field the message does not define.
func decode[M any](t *testing.T, path string) M {
	t.Helper()
	raw, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("%s: %v", path, err)
	}
	msg, err := proposals.DecodeBody(raw)
	if err != nil {
		t.Fatalf("%s: %v", path, err)
	}
	typed, ok := any(msg).(*M)
	if !ok {
		t.Fatalf("%s decoded into %T, want %T", path, msg, (*M)(nil))
	}
	return *typed
}

// refused asserts the generator's decoder refuses a body and that its refusal
// names what is wrong with it.
func refused(t *testing.T, label string, body []byte, name string) {
	t.Helper()
	_, err := proposals.DecodeBody(body)
	if err == nil {
		t.Fatalf("%s was decoded\n%s", label, body)
	}
	if !strings.Contains(err.Error(), name) {
		t.Fatalf("%s was refused without naming %s: %v", label, name, err)
	}
	fmt.Printf("paxeer side: refused %s by name: %v\n", label, err)
}

// assertRefusals takes the generated registration body and proves the decoder
// refuses it under a type URL the module does not define and with a field the
// message does not define, each refusal naming the offending value.
func assertRefusals(t *testing.T, path string, register *types.MsgRegisterChain) {
	t.Helper()
	raw, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("%s: %v", path, err)
	}
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(raw, &fields); err != nil {
		t.Fatalf("%s: %v", path, err)
	}
	var typeURL string
	if err := json.Unmarshal(fields["@type"], &typeURL); err != nil {
		t.Fatalf("%s carries no type URL: %v", path, err)
	}
	if want := sdk.MsgTypeURL(register); typeURL != want {
		t.Fatalf("%s carries the type URL %q, want %q", path, typeURL, want)
	}
	reencode := func(fields map[string]json.RawMessage) []byte {
		body, err := json.Marshal(fields)
		if err != nil {
			t.Fatal(err)
		}
		return body
	}

	wrongURL := typeURL + "Unregistered"
	wrong := make(map[string]json.RawMessage, len(fields))
	for key, value := range fields {
		wrong[key] = value
	}
	wrong["@type"], _ = json.Marshal(wrongURL)
	refused(t, "a body under a wrong type URL", reencode(wrong), wrongURL)

	const unknownField = "paxeer_side_unknown_field"
	unknown := make(map[string]json.RawMessage, len(fields)+1)
	for key, value := range fields {
		unknown[key] = value
	}
	unknown[unknownField] = json.RawMessage(`"1"`)
	refused(t, "a body with an unknown field", reencode(unknown), unknownField)
}

func TestPaxeerSide(t *testing.T) {
	bodies, portFile := os.Getenv("PAXEER_SIDE_BODIES"), os.Getenv("PAXEER_SIDE_PORT_FILE")
	if bodies == "" || portFile == "" {
		t.Fatal("PAXEER_SIDE_BODIES and PAXEER_SIDE_PORT_FILE are required")
	}
	testApp := app.Setup(t, false, false, false)
	k, ctx := bridgetestutil.NewKeeper(testApp, testApp.GetContextForDeliverTx([]byte{}))
	k.InitGenesis(ctx, *types.DefaultGenesis())

	registerPath := filepath.Join(bodies, "01-register-chain.json")
	register := decode[types.MsgRegisterChain](t, registerPath)
	assertRefusals(t, registerPath, &register)
	if err := k.RegisterChain(ctx, register); err != nil {
		t.Fatalf("01-register-chain.json: %v", err)
	}
	attestors := decode[types.MsgSetAttestors](t, filepath.Join(bodies, "02-set-attestors.json"))
	if err := k.SetAttestors(ctx, attestors); err != nil {
		t.Fatalf("02-set-attestors.json: %v", err)
	}
	caps, err := filepath.Glob(filepath.Join(bodies, "03-set-cap-*.json"))
	if err != nil || len(caps) == 0 {
		t.Fatalf("no cap bodies under %s", bodies)
	}
	sort.Strings(caps)
	for _, path := range caps {
		capBody := decode[types.MsgSetCap](t, path)
		if err := k.SetCap(ctx, capBody); err != nil {
			t.Fatalf("%s: %v", path, err)
		}
	}

	precompile := layerxbridge.NewPrecompileWithKeeper(k)
	executor := precompile.GetExecutor()
	contract := precompile.GetABI()
	var lock sync.Mutex
	call := func(method string, params []json.RawMessage) (string, error) {
		if method != "eth_call" {
			return "", fmt.Errorf("%s is not served", method)
		}
		if len(params) == 0 {
			return "", errors.New("eth_call without a call")
		}
		var target struct {
			To   string `json:"to"`
			Data string `json:"data"`
		}
		if err := json.Unmarshal(params[0], &target); err != nil {
			return "", err
		}
		if !strings.EqualFold(target.To, precompile.Address().Hex()) {
			return "", fmt.Errorf("%s is not the layerxBridge precompile", target.To)
		}
		input, err := hex.DecodeString(strings.TrimPrefix(target.Data, "0x"))
		if err != nil || len(input) < 4 {
			return "", fmt.Errorf("%s is not call data", target.Data)
		}
		abiMethod, err := contract.MethodById(input[:4])
		if err != nil {
			return "", err
		}
		args, err := abiMethod.Inputs.Unpack(input[4:])
		if err != nil {
			return "", err
		}
		lock.Lock()
		defer lock.Unlock()
		out, err := executor.Execute(ctx, abiMethod, common.Address{}, common.Address{}, args, new(big.Int), true, nil, nil)
		if err != nil {
			return "", err
		}
		return "0x" + hex.EncodeToString(out), nil
	}
	handler := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var request struct {
			ID     json.RawMessage   `json:"id"`
			Method string            `json:"method"`
			Params []json.RawMessage `json:"params"`
		}
		answer := map[string]interface{}{"jsonrpc": "2.0"}
		if err := json.NewDecoder(r.Body).Decode(&request); err != nil {
			answer["error"] = map[string]interface{}{"code": -32700, "message": err.Error()}
		} else {
			answer["id"] = request.ID
			if result, err := call(request.Method, request.Params); err != nil {
				answer["error"] = map[string]interface{}{"code": -32000, "message": err.Error()}
			} else {
				answer["result"] = result
			}
		}
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(answer)
	})
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	port := fmt.Sprint(listener.Addr().(*net.TCPAddr).Port)
	if err := os.WriteFile(portFile+".tmp", []byte(port), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.Rename(portFile+".tmp", portFile); err != nil {
		t.Fatal(err)
	}
	t.Fatal(http.Serve(listener, handler))
}
GO
(cd "$REPO_ROOT" && go test -c -o "$WORK/paxeer-side.test" "./${GO_PACKAGE#"$REPO_ROOT"/}") > "$WORK/paxeer-build.log" 2>&1 || {
    sed -e 's/^/    /' "$WORK/paxeer-build.log" >&2
    fail "the Paxeer side did not build"
}
rm -rf "$GO_PACKAGE"
GO_PACKAGE=""
PAXEER_SIDE_BODIES="$BODIES" PAXEER_SIDE_PORT_FILE="$WORK/paxeer.port" \
    "$WORK/paxeer-side.test" -test.run '^TestPaxeerSide$' -test.timeout 0 > "$WORK/paxeer.log" 2>&1 &
PAXEER_PID=$!
PIDS+=("$PAXEER_PID")
wait_for_file "$WORK/paxeer.port" "$PAXEER_PID" "the Paxeer side" "$WORK/paxeer.log"
PAXEER_URL="http://127.0.0.1:$(cat "$WORK/paxeer.port")"
for refusal in 'a body under a wrong type URL' 'a body with an unknown field'; do
    grep -qF "paxeer side: refused $refusal by name: " "$WORK/paxeer.log" || {
        sed -e 's/^/    /' "$WORK/paxeer.log" >&2
        fail "the Paxeer side did not refuse $refusal by name"
    }
    printf 'evm-dry-run-check: the proposal decoder refuses %s by name\n' "$refusal"
done

# Every read from here on passes through the recording proxy.
jq -n --arg chain "$CHAIN" --arg anvil "$ANVIL_URL" --arg paxeer "$PAXEER_URL" \
    '{($chain): $anvil, paxeer: $paxeer}' > "$WORK/routes.json"
: > "$WORK/exchange.jsonl"
printf '*' > "$WORK/phase"
start_server proxy record "$WORK/routes.json" "$WORK/exchange.jsonl" "$WORK/phase"
PROXY_PID=$SERVER_PID
PROXY_URL=$SERVER_URL
run_checklist "$WORK/chains" "$PROXY_URL/$CHAIN" "$PROXY_URL/paxeer" "$RECORD" "$MANIFEST" "$WORK/checklist.log"
printf 'readback' > "$WORK/phase"
assert_readback "$PROXY_URL/$CHAIN" "$RECORD" "$EXPECTATIONS"

# The nodes stop here: everything after this reads the recording only.
stop "$PROXY_PID"
stop "$PAXEER_PID"
stop "$ANVIL_PID"
unset ETH_RPC_URL

jq -e 'select(has("error"))' "$WORK/exchange.jsonl" > /dev/null \
    && fail "the recorded exchange carries an error answer: $(jq -c 'select(has("error"))' "$WORK/exchange.jsonl" | head -n 1)"
while IFS=$'\t' read -r phase method; do
    case $phase in
    '*') allowed=$CHECKLIST_METHODS ;;
    *) allowed=$READBACK_METHODS ;;
    esac
    case " $allowed " in
    *" $method "*) ;;
    *) fail "the $phase phase called $method, which is not a read" ;;
    esac
done < <(jq -r '[.phase, .method] | @tsv' "$WORK/exchange.jsonl")

RECORDING="$WORK/evm_dry_run.json"
jq -s 'reduce .[] as $entry ({};
        if any(.[$entry.endpoint][$entry.phase][]?; .method == $entry.method and .params == $entry.params)
        then .
        else .[$entry.endpoint][$entry.phase] += [{method: $entry.method, params: $entry.params, result: $entry.result}]
        end)' "$WORK/exchange.jsonl" > "$WORK/endpoints.json"
jq -n --slurpfile endpoints "$WORK/endpoints.json" --arg chain "$CHAIN" \
    --arg authority "$GOVERNANCE_AUTHORITY" --arg configuration "$RUN_LOCAL_CONFIGURATION" \
    --slurpfile config "$CONFIG" --slurpfile manifest "$MANIFEST" --slurpfile record "$RECORD" \
    --slurpfile expectations "$EXPECTATIONS" \
    '{endpoints: $endpoints[0], chain: $chain, governance_authority: $authority,
      configuration: $config[0], manifest: $manifest[0],
      record: ($record[0] | .configuration = $configuration), expectations: $expectations[0]}' > "$RECORDING"

# The recording carries no key, no endpoint, no date and no hostname.
while read -r secret; do
    ! grep -qi -- "${secret#0x}" "$RECORDING" || fail "the recording carries a private key of the run"
done < <(jq -r '.[].private_key' "$WORK/keys.json")
! grep -qE 'https?://|127\.0\.0\.1|localhost|:'"$ANVIL_PORT"'\b' "$RECORDING" || fail "the recording carries an endpoint"
! grep -qE '[0-9]{4}-[0-9]{2}-[0-9]{2}' "$RECORDING" || fail "the recording carries a date"
host=$(hostname 2> /dev/null || true)
[ -z "$host" ] || ! grep -qiF -- "$host" "$RECORDING" || fail "the recording carries the hostname"
! grep -qF -- "$WORK" "$RECORDING" || fail "the recording carries a path of the run"

replay_fixture "$RECORDING" "recording of this run"
if [ "$record" -eq 1 ]; then
    jq . "$RECORDING" > "$FIXTURE"
    printf 'evm-dry-run-check: the recording is written to %s\n' "${FIXTURE#"$REPO_ROOT"/}"
else
    replay_fixture "$FIXTURE" "committed fixture"
fi
printf 'evm-dry-run-check: every step behaves\n'
