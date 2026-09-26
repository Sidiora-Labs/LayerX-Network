#!/usr/bin/env bash
# End-to-end dry run of the Solana deployment of the Paxeer X Network bridge
# against a local solana-test-validator, and the replay of its recorded exchange.
#
# Usage:
#   solana-dry-run-check.sh             the dry run, then the replay of its own
#                                       recording and of the committed fixture
#   solana-dry-run-check.sh --record    the dry run, then its recording replaces
#                                       bridge/deploy/tests/fixtures/solana_dry_run.json
#   solana-dry-run-check.sh --replay    the committed fixture only, without a node
#
# The dry run starts solana-test-validator on the loopback interface and
# generates every key it uses for that run alone: the publisher, the owner, the
# program, the depositor, the recipient, the authority of the run's Sidiora mint
# and the secp256k1 attestor set. It writes a run-local copy of
# bridge/solana/chains/solana/config.json whose placeholder owner, attestors and
# program id are replaced by that run's own values and nothing else, and a
# run-local attestor-set manifest naming the same set. The cluster carries the
# wrapped SOL mint of its genesis and, at Sidiora's mint address, a six-decimal
# SPL mint of this run standing for Sidiora, so the configured pair of that mint
# and the asset id 0x21f7b20a555199fa73A238B1a91FD0f549068fEe is registered as it
# will be on mainnet. Then it:
#   deploys, initialises and registers both assets with
#   bridge/deploy/deploy-solana-program.sh, through the real admin client, and
#   asserts the deployment record names the program id, the program data
#   account, the ELF hash of the deployed program and the vault-authority PDA
#   with the handle bridge/vectors derives;
#   deposits wrapped SOL and Sidiora, and asserts each receipt PDA;
#   releases part of each against the threshold of attestor signatures over the
#   outbound preimage, verified by the native secp256k1 program in the same
#   transaction, asserts the recipient's balances and each nullifier PDA, and
#   proves a second release of the same Paxeer burn is refused as replayed;
#   opens Solana on a Paxeer side made of the real layerxbridge keeper and
#   precompile, which executes the governance bodies bridge/deploy/proposals
#   generates for the deployment and registers Sidiora the way the chain's
#   upgrade handler does, and runs bridge/deploy/checklist.sh against both,
#   asserting every check passes with the wrapped SOL asset checked first.
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
# Nothing public is dialled. A missing tool stops the run naming it, and so does
# a toolchain that cannot build the program; nothing is skipped.
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
DEPLOY_DIR=$(cd "$SCRIPT_DIR/.." && pwd)
REPO_ROOT=$(cd "$DEPLOY_DIR/../.." && pwd)
PROGRAM_DIR="$REPO_ROOT/bridge/solana"
DEPLOY="$DEPLOY_DIR/deploy-solana-program.sh"
CHECKLIST="$DEPLOY_DIR/checklist.sh"
FIXTURE="$SCRIPT_DIR/fixtures/solana_dry_run.json"
CHAIN=solana
COMMITTED_CONFIG="$PROGRAM_DIR/chains/$CHAIN/config.json"
GOVERNANCE_AUTHORITY=pax10d07y265gmmuvt4z0w9aw880jnsr700jxdwa9m
RUN_LOCAL_CONFIGURATION="run-local/$CHAIN/config.json"
OUTBOUND_DOMAIN=PAXEERX_BRIDGE_OUT_V1
WRAPPED_SOL_MINT=So11111111111111111111111111111111111111112
SIDIORA_MINT=5w3wVdJaESaJKyLmStM6Hv9UyUkmZ1b9DLQquAqqpump
SIDIORA_ASSET_ID=0x21f7b20a555199fa73a238b1a91fd0f549068fee
TOKEN_PROGRAM=TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA
ASSOCIATED_TOKEN_PROGRAM=ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL
SYSTEM_PROGRAM=11111111111111111111111111111111
SECP256K1_PROGRAM=KeccakSecp256k11111111111111111111111111111
INSTRUCTIONS_SYSVAR=Sysvar1nstructions1111111111111111111111111
UPGRADEABLE_LOADER=BPFLoaderUpgradeab1e11111111111111111111111
INSTRUCTION_PREFIX=505842520001
OP_DEPOSIT=08
OP_RELEASE=0a
REPLAYED_ERROR=14
CHECKLIST_METHODS='getGenesisHash getAccountInfo eth_call'
READBACK_METHODS='getAccountInfo'
FUNDING_SOL=1000
WRAPPED_SOL=3
SIDIORA_MINTED=1000000000
SOL_DEPOSIT=1500000000
SOL_RELEASE=400000000
SIDIORA_DEPOSIT=250000000
SIDIORA_RELEASE=100000000

fail() {
    printf 'solana-dry-run-check: error: %s\n' "$*" >&2
    exit 1
}

usage() {
    printf 'usage: solana-dry-run-check.sh [--record | --replay]\n' >&2
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

tools=(jq curl cast go python3 base64 od sha256sum)
[ "$mode" = replay ] \
    || tools=(solana solana-test-validator solana-keygen cargo-build-sbf spl-token cargo "${tools[@]}")
for tool in "${tools[@]}"; do
    command -v "$tool" > /dev/null 2>&1 || fail "$tool is required for the dry run and is not on the PATH"
done
for file in "$DEPLOY" "$CHECKLIST" "$COMMITTED_CONFIG" "$PROGRAM_DIR/Cargo.toml"; do
    [ -r "$file" ] || fail "$file is missing"
done

# Nothing an operator happens to have exported may reach the scripts: every
# endpoint, key and path below belongs to this run, and loopback traffic never
# goes through a proxy.
unset "${!PAXEER_BRIDGE_@}"
export NO_PROXY=127.0.0.1 no_proxy=127.0.0.1

WORK=$(mktemp -d)
PIDS=()
GO_PACKAGES=()
cleanup() {
    local pid package
    for pid in "${PIDS[@]}"; do
        kill "$pid" 2> /dev/null || true
    done
    for pid in "${PIDS[@]}"; do
        wait "$pid" 2> /dev/null || true
    done
    for package in "${GO_PACKAGES[@]}"; do
        rm -rf "$package"
    done
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
    for _ in $(seq 1 3000); do
        [ -s "$file" ] && return 0
        kill -0 "$pid" 2> /dev/null || {
            sed -e 's/^/    /' "$log" >&2
            fail "$label stopped before it reported its port"
        }
        sleep 0.1
    done
    fail "$label did not report its port"
}

expect() {
    local label=$1 expected=$2 found=$3
    [ "$expected" = "$found" ] || fail "$label: expected $expected, found $found"
    printf 'solana-dry-run-check: %s: %s\n' "$label" "$found"
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

start_server() {
    local name=$1
    shift
    python3 "$WORK/server.py" "$@" "$WORK/$name.port" > "$WORK/$name.log" 2>&1 &
    PIDS+=("$!")
    wait_for_file "$WORK/$name.port" "$!" "the $name server" "$WORK/$name.log"
    SERVER_PID=$!
    SERVER_URL="http://127.0.0.1:$(cat "$WORK/$name.port")"
}

# Every program-derived address and every handle of the run comes from
# bridge/vectors, the package that pins the Solana identity mapping, so the dry
# run derives nothing of its own.
GO_PACKAGE=$(mktemp -d "$SCRIPT_DIR/solanakeys_XXXXXX")
GO_PACKAGES+=("$GO_PACKAGE")
cat > "$GO_PACKAGE/main.go" << 'GO'
package main

import (
	"encoding/hex"
	"fmt"
	"os"

	"github.com/sidiora-labs/paxeer-network/bridge/vectors"
)

func key(text string) vectors.Key32 {
	parsed, err := vectors.Key(text)
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	return parsed
}

func main() {
	if len(os.Args) < 3 {
		fmt.Fprintln(os.Stderr, "usage: handle <key> | vault <program id> | pda <program id> <hex seed>...")
		os.Exit(2)
	}
	switch os.Args[1] {
	case "handle":
		fmt.Println(vectors.Handle(key(os.Args[2])).Hex())
	case "vault":
		program := key(os.Args[2])
		authority, _, err := vectors.VaultAuthority(program)
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
		handle, err := vectors.VaultHandle(program)
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
		fmt.Println(authority.Base58(), handle.Hex())
	case "pda":
		seeds := make([][]byte, 0, len(os.Args)-3)
		for _, text := range os.Args[3:] {
			seed, err := hex.DecodeString(text)
			if err != nil {
				fmt.Fprintf(os.Stderr, "%s is not a hex seed\n", text)
				os.Exit(1)
			}
			seeds = append(seeds, seed)
		}
		address, _, err := vectors.FindProgramAddress(key(os.Args[2]), seeds)
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
		fmt.Println(address.Base58())
	default:
		fmt.Fprintf(os.Stderr, "%s is not a command\n", os.Args[1])
		os.Exit(2)
	}
}
GO
(cd "$REPO_ROOT" && go build -o "$WORK/solana-keys" "./${GO_PACKAGE#"$REPO_ROOT"/}") > "$WORK/keys-build.log" 2>&1 || {
    sed -e 's/^/    /' "$WORK/keys-build.log" >&2
    fail "the bridge/vectors key helper did not build"
}
rm -rf "$GO_PACKAGE"
KEYS="$WORK/solana-keys"

key_hex() {
    python3 -c '
import sys
ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
text = sys.argv[1]
number = 0
for character in text:
    number = number * 58 + ALPHABET.index(character)
raw = b"\x00" * (len(text) - len(text.lstrip("1"))) + number.to_bytes((number.bit_length() + 7) // 8, "big")
assert len(raw) == 32, text
print(raw.hex())' "$1" || fail "$1 is not a 32-byte base58 key"
}
hex_of() { printf '%s' "$1" | od -An -v -tx1 | tr -d ' \n'; }
pda() {
    local program=$1
    shift
    "$KEYS" pda "$program" "$@" || fail "no program-derived address of $program for the seeds $*"
}
receipt_address() { pda "$1" "$(hex_of receipt)" "$(printf '%016x' "$2")"; }
nullifier_address() { pda "$1" "$(hex_of nullifier)" "$2"; }
program_data_address() { pda "$UPGRADEABLE_LOADER" "$(key_hex "$1")"; }

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

# account prints the owner and the hex data of an account at the finalized
# commitment, or "none" when it does not exist.
account() {
    local url=$1 address=$2 info
    info=$(rpc "$url" getAccountInfo "$(jq -cn --arg a "$address" '[$a, {encoding: "base64", commitment: "finalized"}]')")
    if [ "$(jq -r '.value == null' <<< "$info")" = true ]; then
        printf 'none\n'
        return 0
    fi
    printf '%s %s\n' "$(jq -r '.value.owner' <<< "$info")" \
        "$(jq -r '.value.data[0]' <<< "$info" | base64 -d | od -An -v -tx1 | tr -d ' \n')"
}

u64be() { printf '%u' "$((16#$1))"; }
u64le() {
    local bytes=$1 out="" index
    for ((index = 14; index >= 0; index -= 2)); do
        out+=${bytes:index:2}
    done
    u64be "$out"
}

# token_amount prints the amount an SPL token account holds and requires it to
# be an account of the mint and owner given.
token_amount() {
    local url=$1 address=$2 mint=$3 owner=$4 found data
    found=$(account "$url" "$address")
    [ "$found" != none ] || fail "the token account $address does not exist"
    [ "${found%% *}" = "$TOKEN_PROGRAM" ] || fail "$address is not an SPL token account"
    data=${found#* }
    [ "${data:0:64}" = "$(key_hex "$mint")" ] || fail "$address is not an account of the mint $mint"
    [ "${data:64:64}" = "$(key_hex "$owner")" ] || fail "$address is not owned by $owner"
    u64le "${data:128:16}"
}

# assert_readback proves, through one endpoint, that the deployment record names
# the program data account and the vault authority the program id derives, and
# that the deposits and releases of the run left the program in the state they
# must: every receipt, every nullifier, the recipient's balances and the vault's
# custody.
assert_readback() {
    local url=$1 record=$2 expectations=$3 program vault_line index count found data address mint owner
    program=$(jq -r '.program_id' "$record")
    expect "program id in the record" "$(jq -r '.program_id' "$expectations")" "$program"
    expect "program data account in the record" "$(program_data_address "$program")" \
        "$(jq -r '.program_data_account' "$record")"
    vault_line=$("$KEYS" vault "$program") || fail "bridge/vectors derives no vault authority for $program"
    expect "vault authority in the record" "${vault_line%% *}" "$(jq -r '.vault_authority' "$record")"
    expect "vault handle in the record, as bridge/vectors derives it" "${vault_line#* }" \
        "$(jq -r '.vault_handle' "$record")"
    [[ $(jq -r '.program_elf_sha256' "$record") =~ ^[0-9a-f]{64}$ ]] || fail "the record names no ELF hash"

    count=$(jq '.receipts | length' "$expectations")
    [ "$count" -gt 0 ] || fail "$expectations names no receipt"
    for ((index = 0; index < count; index++)); do
        address=$(jq -r --argjson i "$index" '.receipts[$i].account' "$expectations")
        expect "receipt PDA of deposit $((index + 1))" \
            "$(receipt_address "$program" "$(jq -r --argjson i "$index" '.receipts[$i].nonce' "$expectations")")" "$address"
        found=$(account "$url" "$address")
        [ "$found" != none ] || fail "the receipt $address does not exist"
        expect "owner of receipt $address" "$program" "${found%% *}"
        data=${found#* }
        [ "${data:0:16}" = "$(hex_of PXBRRCP0)" ] || fail "the receipt $address does not open with PXBRRCP0"
        expect "receipt $address" "$(jq -r --argjson i "$index" '.receipts[$i] |
            "v1 nonce \(.nonce) mint \(.mint_hex) amount \(.amount) recipient \(.paxeer_recipient) depositor \(.depositor_hex)"' "$expectations")" \
            "v$((16#${data:16:4})) nonce $(u64be "${data:20:16}") mint ${data:36:64} amount $(u64be "${data:100:16}") recipient 0x${data:116:64} depositor ${data:180:64}"
    done

    count=$(jq '.nullifiers | length' "$expectations")
    [ "$count" -gt 0 ] || fail "$expectations names no nullifier"
    for ((index = 0; index < count; index++)); do
        address=$(jq -r --argjson i "$index" '.nullifiers[$i].account' "$expectations")
        expect "nullifier PDA of burn $(jq -r --argjson i "$index" '.nullifiers[$i].paxeer_nonce' "$expectations")" \
            "$(nullifier_address "$program" "$(jq -r --argjson i "$index" '.nullifiers[$i].nullifier' "$expectations")")" "$address"
        found=$(account "$url" "$address")
        [ "$found" != none ] || fail "the nullifier $address does not exist"
        expect "owner of nullifier $address" "$program" "${found%% *}"
        data=${found#* }
        [ "${data:0:16}" = "$(hex_of PXBRNUL0)" ] || fail "the nullifier $address does not open with PXBRNUL0"
        expect "nullifier $address" "$(jq -r --argjson i "$index" '.nullifiers[$i] |
            "v1 burn \(.paxeer_tx_hash) nonce \(.paxeer_nonce) mint \(.mint_hex) recipient \(.recipient_hex) amount \(.amount)"' "$expectations")" \
            "v$((16#${data:16:4})) burn 0x${data:20:64} nonce $(u64be "${data:84:16}") mint ${data:100:64} recipient ${data:164:64} amount $(u64be "${data:228:16}")"
    done

    count=$(jq '.balances | length' "$expectations")
    [ "$count" -gt 0 ] || fail "$expectations names no balance"
    for ((index = 0; index < count; index++)); do
        address=$(jq -r --argjson i "$index" '.balances[$i].account' "$expectations")
        mint=$(jq -r --argjson i "$index" '.balances[$i].mint' "$expectations")
        owner=$(jq -r --argjson i "$index" '.balances[$i].owner' "$expectations")
        expect "$(jq -r --argjson i "$index" '.balances[$i].label' "$expectations")" \
            "$(jq -r --argjson i "$index" '.balances[$i].amount' "$expectations")" \
            "$(token_amount "$url" "$address" "$mint" "$owner")"
    done
}

# run_checklist runs the real checklist against two endpoint URLs and asserts it
# passes every check with the wrapped SOL asset checked first.
run_checklist() {
    local chains_root=$1 chain_url=$2 paxeer_url=$3 record=$4 manifest=$5 log=$6 rpc_variable first status=0
    rpc_variable=$(jq -r '.environment.rpc_url' "$chains_root/$CHAIN/config.json")
    env "PAXEER_BRIDGE_SOLANA_CHAINS_ROOT=$chains_root" "$rpc_variable=$chain_url" \
        "PAXEER_BRIDGE_PAXEER_RPC_URL=$paxeer_url" "PAXEER_BRIDGE_DEPLOYMENT_RECORD=$record" \
        "PAXEER_BRIDGE_GOVERNANCE_AUTHORITY=$GOVERNANCE_AUTHORITY" "PAXEER_BRIDGE_ATTESTOR_MANIFEST=$manifest" \
        bash "$CHECKLIST" "$CHAIN" > "$log" 2>&1 || status=$?
    if [ "$status" -ne 0 ] || grep -q ' FAIL ' "$log" || ! grep -q 'passes all [0-9]* checks' "$log"; then
        sed -e 's/^/    /' "$log" >&2
        fail "the checklist did not pass every check (exit $status)"
    fi
    first=$(grep -E "^checklist: $CHAIN (ok|FAIL) " "$log" \
        | grep -vE ' (ok|FAIL) (placeholders in |deployment record |governance bodies:|genesis hash:|program account )' \
        | head -n 1)
    case $first in
    "checklist: $CHAIN ok SOL asset account owner:"*) ;;
    *)
        sed -e 's/^/    /' "$log" >&2
        fail "the checklist did not check the wrapped SOL asset first: $first"
        ;;
    esac
    printf 'solana-dry-run-check: %s\n' "$(tail -n 1 "$log")"
}

# The fixture's configuration is the committed one with only the placeholders
# an operator fills in replaced.
assert_no_drift() {
    local config=$1
    [ "$(jq -S 'del(.owner, .attestors, .solana.program_id)' "$COMMITTED_CONFIG")" \
        = "$(jq -S 'del(.owner, .attestors, .solana.program_id)' "$config")" ] \
        || fail "$config differs from $COMMITTED_CONFIG in more than the owner, the attestors and the program id"
    ! jq -r '.owner, .attestors[], .solana.program_id' "$config" | grep -q '^PLACEHOLDER:' \
        || fail "$config still carries a placeholder"
}

# replay_fixture asserts the whole recorded exchange of a fixture without a node:
# the checklist passes against it and the dry run's assertions hold, and neither
# asks for anything the recording does not answer.
replay_fixture() {
    local fixture=$1 label=$2 dir method
    [ -r "$fixture" ] || fail "$fixture is missing; run solana-dry-run-check.sh --record to write it"
    jq -e '.endpoints | type == "object"' "$fixture" > /dev/null 2>&1 || fail "$fixture is not a recorded exchange"
    [ "$(jq -r '.chain' "$fixture")" = "$CHAIN" ] || fail "$fixture does not record $CHAIN"
    dir=$(mktemp -d "$WORK/replay.XXXXXX")
    mkdir -p "$dir/chains/$CHAIN"
    jq '.configuration' "$fixture" > "$dir/chains/$CHAIN/config.json"
    jq '.manifest' "$fixture" > "$dir/attestors.json"
    jq '.record' "$fixture" > "$dir/record.json"
    jq '.expectations' "$fixture" > "$dir/expectations.json"
    assert_no_drift "$dir/chains/$CHAIN/config.json"
    expect "program id the replayed configuration names" "$(jq -r '.solana.program_id' "$dir/chains/$CHAIN/config.json")" \
        "$(jq -r '.program_id' "$dir/record.json")"
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
    printf 'solana-dry-run-check: the %s replays without a node\n' "$label"
}

if [ "$mode" = replay ]; then
    replay_fixture "$FIXTURE" "committed fixture"
    printf 'solana-dry-run-check: every step behaves\n'
    exit 0
fi

# The run's Solana transactions: a legacy message compiled from instructions,
# signed with the run's ed25519 keypair files, sent with preflight and confirmed
# at the finalized commitment.
cat > "$WORK/transaction.py" << 'PY'
import base64
import json
import sys
import time
import urllib.request

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))


def b58decode(text):
    number = 0
    for character in text:
        number = number * 58 + ALPHABET.index(character)
    raw = number.to_bytes((number.bit_length() + 7) // 8, "big")
    return b"\x00" * (len(text) - len(text.lstrip("1"))) + raw


def b58encode(raw):
    number = int.from_bytes(raw, "big")
    text = ""
    while number:
        number, remainder = divmod(number, 58)
        text = ALPHABET[remainder] + text
    return "1" * (len(raw) - len(raw.lstrip(b"\x00"))) + text


def compact(length):
    out = bytearray()
    while True:
        byte = length & 0x7F
        length >>= 7
        if length:
            out.append(byte | 0x80)
        else:
            out.append(byte)
            return bytes(out)


def rpc(url, method, params):
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode()
    request = urllib.request.Request(url, data=body, headers={"Content-Type": "application/json"})
    with opener.open(request, timeout=60) as response:
        return json.loads(response.read())


def keypair(path):
    with open(path) as handle:
        secret = bytes(json.load(handle))
    private = Ed25519PrivateKey.from_private_bytes(secret[:32])
    return b58encode(secret[32:64]), private


def compile_message(payer, instructions, blockhash):
    metas = {payer: [True, True]}
    order = [payer]
    for instruction in instructions:
        for key, signer, writable in instruction["accounts"]:
            if key not in metas:
                metas[key] = [False, False]
                order.append(key)
            metas[key][0] |= signer
            metas[key][1] |= writable
        if instruction["program"] not in metas:
            metas[instruction["program"]] = [False, False]
            order.append(instruction["program"])
    groups = ([k for k in order if metas[k] == [True, True]],
              [k for k in order if metas[k] == [True, False]],
              [k for k in order if metas[k] == [False, True]],
              [k for k in order if metas[k] == [False, False]])
    keys = [key for group in groups for key in group]
    index = {key: position for position, key in enumerate(keys)}
    message = bytes([len(groups[0]) + len(groups[1]), len(groups[1]), len(groups[3])])
    message += compact(len(keys)) + b"".join(b58decode(key) for key in keys)
    message += b58decode(blockhash) + compact(len(instructions))
    for instruction in instructions:
        data = bytes.fromhex(instruction["data"])
        message += bytes([index[instruction["program"]]])
        message += compact(len(instruction["accounts"]))
        message += bytes(index[key] for key, _, _ in instruction["accounts"])
        message += compact(len(data)) + data
    return message, keys[:len(groups[0]) + len(groups[1])]


def send(url, instructions_file, signer_files):
    with open(instructions_file) as handle:
        instructions = json.load(handle)
    signers = dict(keypair(path) for path in signer_files)
    payer = keypair(signer_files[0])[0]
    answer = rpc(url, "getLatestBlockhash", [{"commitment": "finalized"}])
    blockhash = answer["result"]["value"]["blockhash"]
    message, required = compile_message(payer, instructions, blockhash)
    missing = [key for key in required if key not in signers]
    if missing:
        raise SystemExit("no keypair file signs for %s" % ", ".join(missing))
    signatures = [signers[key].sign(message) for key in required]
    transaction = compact(len(signatures)) + b"".join(signatures) + message
    answer = rpc(url, "sendTransaction", [base64.b64encode(transaction).decode(),
                                          {"encoding": "base64", "preflightCommitment": "finalized"}])
    if "error" in answer:
        print(json.dumps(answer["error"]), file=sys.stderr)
        raise SystemExit(3)
    signature = answer["result"]
    for _ in range(600):
        status = rpc(url, "getSignatureStatuses", [[signature], {"searchTransactionHistory": True}])
        value = status["result"]["value"][0]
        if value is not None:
            if value.get("err") is not None:
                print(json.dumps(value["err"]), file=sys.stderr)
                raise SystemExit(4)
            if value.get("confirmationStatus") == "finalized":
                print(signature)
                return
        time.sleep(0.2)
    raise SystemExit("%s was not finalized" % signature)


def secp_data(preimage, entries):
    message = bytes.fromhex(preimage)
    count = len(entries)
    table = 1 + 11 * count
    body = b""
    rows = b""
    for position, (signature, address) in enumerate(entries):
        raw = bytes.fromhex(signature[2:])
        if len(raw) != 65 or raw[64] not in (27, 28):
            raise SystemExit("%s is not a 65-byte signature with v 27 or 28" % signature)
        offset = table + 85 * position
        rows += offset.to_bytes(2, "little") + b"\x00"
        rows += (offset + 65).to_bytes(2, "little") + b"\x00"
        rows += (table + 85 * count).to_bytes(2, "little") + len(message).to_bytes(2, "little") + b"\x00"
        body += raw[:64] + bytes([raw[64] - 27]) + bytes.fromhex(address[2:])
    return (bytes([count]) + rows + body + message).hex()


def settle(url):
    target = rpc(url, "getSlot", [{"commitment": "confirmed"}])["result"]
    for _ in range(600):
        if rpc(url, "getSlot", [{"commitment": "finalized"}])["result"] >= target:
            return
        time.sleep(0.2)
    raise SystemExit("the finalized slot did not reach %d" % target)


def mint_account(address, authority, decimals, supply, lamports, token_program):
    data = (b"\x01\x00\x00\x00" + b58decode(authority) + supply.to_bytes(8, "little")
            + bytes([decimals, 1]) + b"\x00" * 36)
    print(json.dumps({"pubkey": address, "account": {
        "lamports": lamports, "data": [base64.b64encode(data).decode(), "base64"],
        "owner": token_program, "executable": False, "rentEpoch": 0, "space": len(data)}}))


command = sys.argv[1]
if command == "pubkey":
    print(keypair(sys.argv[2])[0])
elif command == "secret":
    with open(sys.argv[2]) as handle:
        print(b58encode(bytes(json.load(handle))))
elif command == "send":
    send(sys.argv[2], sys.argv[3], sys.argv[4:])
elif command == "secp-data":
    print(secp_data(sys.argv[2], [entry.split(":") for entry in sys.argv[3:]]))
elif command == "settle":
    settle(sys.argv[2])
elif command == "mint-account":
    mint_account(sys.argv[2], sys.argv[3], int(sys.argv[4]), int(sys.argv[5]), int(sys.argv[6]), sys.argv[7])
else:
    raise SystemExit("%s is not a command" % command)
PY
TX=(python3 "$WORK/transaction.py")
python3 -c 'import cryptography.hazmat.primitives.asymmetric.ed25519' 2> /dev/null \
    || fail "the Python cryptography package is required to sign the run's transactions and is not installed"

TOOLCHAIN=$(dirname "$(command -v solana)")
PLATFORM_TOOLS=$(sed -n 's/^PLATFORM_TOOLS_VERSION=//p' "$DEPLOY")
[ -n "$PLATFORM_TOOLS" ] || fail "$DEPLOY declares no PLATFORM_TOOLS_VERSION, so the platform tools it builds with are unknown"
for tool in solana-keygen cargo-build-sbf; do
    [ "$(dirname "$(command -v "$tool")")" = "$TOOLCHAIN" ] \
        || fail "$tool is not in $TOOLCHAIN beside solana; the dry run deploys with one pinned toolchain"
done

# The admin client the deploy script calls, built from the workspace it lives in.
cargo build --locked --quiet --manifest-path "$PROGRAM_DIR/Cargo.toml" -p paxeer-x-bridge-solana-admin \
    > "$WORK/admin-build.log" 2>&1 || {
    sed -e 's/^/    /' "$WORK/admin-build.log" >&2
    fail "the admin client paxeer-x-bridge-solana-admin did not build"
}
ADMIN=$(cargo metadata --format-version 1 --no-deps --manifest-path "$PROGRAM_DIR/Cargo.toml" \
    | jq -r '.target_directory')/debug/paxeer-x-bridge-solana-admin
[ -x "$ADMIN" ] || fail "$ADMIN was not built"

# Keys of this run only.
mkdir -p "$WORK/keys"
chmod 0700 "$WORK/keys"
for name in publisher owner program depositor recipient sidiora-authority vault-sol vault-sid; do
    solana-keygen new --no-bip39-passphrase --silent --force --outfile "$WORK/keys/$name.json" > /dev/null \
        || fail "solana-keygen did not generate the run's $name key"
done
pubkey_of() { "${TX[@]}" pubkey "$WORK/keys/$1.json"; }
PUBLISHER=$(pubkey_of publisher)
OWNER=$(pubkey_of owner)
PROGRAM_ID=$(pubkey_of program)
DEPOSITOR=$(pubkey_of depositor)
RECIPIENT=$(pubkey_of recipient)
SIDIORA_AUTHORITY=$(pubkey_of sidiora-authority)
VAULT_SOL=$(pubkey_of vault-sol)
VAULT_SID=$(pubkey_of vault-sid)
expect "program id the run's program keypair holds" "$PROGRAM_ID" "$(solana-keygen pubkey "$WORK/keys/program.json")"
cast wallet new --json --number 5 > "$WORK/attestors.raw.json" 2> /dev/null || fail "cast did not generate the run's attestors"
chmod 0600 "$WORK/attestors.raw.json"
jq '[.[] | {address: (.address | ascii_downcase), private_key}] | sort_by(.address)' "$WORK/attestors.raw.json" \
    > "$WORK/attestors.keys.json"
mapfile -t ATTESTORS < <(jq -r '.[].address' "$WORK/attestors.keys.json")
mapfile -t ATTESTOR_KEYS < <(jq -r '.[].private_key' "$WORK/attestors.keys.json")
THRESHOLD=$(jq -r '.threshold' "$COMMITTED_CONFIG")
CHAIN_ID=$(jq -r '.chain_id' "$COMMITTED_CONFIG")
[ "$THRESHOLD" -le "${#ATTESTORS[@]}" ] || fail "the run generated fewer attestors than the threshold $THRESHOLD"

# The run-local configuration and manifest.
mkdir -p "$WORK/chains/$CHAIN"
CONFIG="$WORK/chains/$CHAIN/config.json"
jq --arg owner "$OWNER" --arg program "$PROGRAM_ID" \
    --argjson attestors "$(printf '%s\n' "${ATTESTORS[@]}" | jq -R . | jq -s .)" \
    '.owner = $owner | .attestors = $attestors | .solana.program_id = $program' "$COMMITTED_CONFIG" > "$CONFIG"
assert_no_drift "$CONFIG"
MANIFEST="$WORK/attestors.json"
jq '{attestors: .attestors, threshold: .threshold}' "$CONFIG" > "$MANIFEST"
[ "$(jq -r '.assets[0].address' "$CONFIG")" = "$WRAPPED_SOL_MINT" ] || fail "the first configured asset is not wrapped SOL"
[ "$(jq -r '.assets[1].address' "$CONFIG")" = "$SIDIORA_MINT" ] || fail "the second configured asset is not Sidiora's mint"
[ "$(lower "$(jq -r '.assets[1].asset_id' "$CONFIG")")" = "$SIDIORA_ASSET_ID" ] \
    || fail "Sidiora's mint is not configured with the asset id $SIDIORA_ASSET_ID"
SIDIORA_DECIMALS=$(jq -r '.assets[1].decimals' "$CONFIG")
expect "decimals of the Sidiora mint" 6 "$SIDIORA_DECIMALS"
RPC_VARIABLE=$(jq -r '.environment.rpc_url' "$CONFIG")
KEY_VARIABLE=$(jq -r '.environment.deploy_key' "$CONFIG")
TOOLCHAIN_VARIABLE=$(jq -r '.environment.toolchain_bin' "$CONFIG")

# The six-decimal mint standing for Sidiora, at Sidiora's mint address, whose
# authority is the run's own key.
"${TX[@]}" mint-account "$SIDIORA_MINT" "$SIDIORA_AUTHORITY" "$SIDIORA_DECIMALS" 0 1461600 "$TOKEN_PROGRAM" \
    > "$WORK/sidiora-mint.json"

# The Solana CLI reads its commitment from its own configuration; the run's
# configuration waits for the finalized commitment the chain configuration
# names, so the deployment is rooted before the deploy script reads it back.
# The caches the build reuses stay where they are.
CLI_HOME="$WORK/home"
mkdir -p "$CLI_HOME/.config/solana/cli"
for cache in .cache .cargo .rustup; do
    [ ! -e "$HOME/$cache" ] || ln -s "$HOME/$cache" "$CLI_HOME/$cache"
done
export CARGO_HOME=${CARGO_HOME:-$HOME/.cargo} RUSTUP_HOME=${RUSTUP_HOME:-$HOME/.rustup}

free_port() {
    python3 -c '
import socket
while True:
    first = socket.socket(); first.bind(("127.0.0.1", 0)); port = first.getsockname()[1]
    second = socket.socket()
    try:
        second.bind(("127.0.0.1", port + 1))
    except OSError:
        first.close(); second.close(); continue
    first.close(); second.close(); print(port); break'
}
RPC_PORT=$(free_port)
FAUCET_PORT=$(free_port)
VALIDATOR_URL="http://127.0.0.1:$RPC_PORT"
cat > "$CLI_HOME/.config/solana/cli/config.yml" << YAML
json_rpc_url: "$VALIDATOR_URL"
websocket_url: ""
keypair_path: "$WORK/keys/publisher.json"
address_labels: {}
commitment: finalized
YAML

# solana-test-validator on the loopback interface, its genesis funding the
# run's publisher, its ledger inside the run.
HOME="$CLI_HOME" solana-test-validator --ledger "$WORK/ledger" --reset --quiet \
    --bind-address 127.0.0.1 --rpc-port "$RPC_PORT" --faucet-port "$FAUCET_PORT" \
    --mint "$PUBLISHER" --account "$SIDIORA_MINT" "$WORK/sidiora-mint.json" \
    > "$WORK/validator.log" 2>&1 &
VALIDATOR_PID=$!
PIDS+=("$VALIDATOR_PID")
for _ in $(seq 1 600); do
    health=$(curl --silent --noproxy '*' --header 'Content-Type: application/json' \
        --data '{"jsonrpc":"2.0","id":1,"method":"getHealth"}' "$VALIDATOR_URL" 2> /dev/null || true)
    [ "$(jq -r '.result // empty' <<< "$health" 2> /dev/null)" = ok ] && break
    kill -0 "$VALIDATOR_PID" 2> /dev/null || {
        sed -e 's/^/    /' "$WORK/validator.log" >&2
        tail -n 40 "$WORK/ledger/validator.log" 2> /dev/null | sed -e 's/^/    /' >&2 || true
        fail "solana-test-validator stopped before it answered"
    }
    sleep 0.2
done
[ "$(jq -r '.result // empty' <<< "${health:-}" 2> /dev/null)" = ok ] || fail "solana-test-validator did not become healthy"
"${TX[@]}" settle "$VALIDATOR_URL" || fail "the local cluster finalized no slot"
found=$(account "$VALIDATOR_URL" "$WRAPPED_SOL_MINT")
expect "owner of the wrapped SOL mint" "$TOKEN_PROGRAM" "${found%% *}"
found=$(account "$VALIDATOR_URL" "$SIDIORA_MINT")
expect "owner of the run's Sidiora mint" "$TOKEN_PROGRAM" "${found%% *}"

cli() { HOME="$CLI_HOME" "$@" --url "$VALIDATOR_URL"; }
for name in owner depositor; do
    cli solana transfer --keypair "$WORK/keys/publisher.json" --allow-unfunded-recipient \
        "$(pubkey_of "$name")" "$FUNDING_SOL" > "$WORK/fund-$name.log" 2>&1 || {
        sed -e 's/^/    /' "$WORK/fund-$name.log" >&2
        fail "the run's $name was not funded"
    }
done

# The deployment, initialisation and asset registration, through the real
# deploy script and the real admin client.
RECORD="$WORK/record.json"
if ! env HOME="$CLI_HOME" "PAXEER_BRIDGE_SOLANA_CHAINS_ROOT=$WORK/chains" "$RPC_VARIABLE=$VALIDATOR_URL" \
    "$KEY_VARIABLE=$WORK/keys/publisher.json" "$TOOLCHAIN_VARIABLE=$TOOLCHAIN" \
    "PAXEER_BRIDGE_DEPLOYMENT_RECORD=$RECORD" "PAXEER_BRIDGE_SOLANA_ADMIN_CLI=$ADMIN" \
    "PAXEER_BRIDGE_SOLANA_PROGRAM_KEYPAIR_FILE=$WORK/keys/program.json" \
    "PAXEER_BRIDGE_SOLANA_OWNER_KEYPAIR_FILE=$WORK/keys/owner.json" \
    bash "$DEPLOY" > "$WORK/deploy.log" 2>&1; then
    sed -e 's/^/    /' "$WORK/deploy.log" >&2
    if grep -q 'cargo-build-sbf could not build' "$WORK/deploy.log"; then
        missing=$(grep -m 1 -oE 'feature .[a-z0-9_-]+. is required' "$WORK/deploy.log" \
            || grep -m 1 -E '^error' "$WORK/deploy.log" || printf 'see the build output above')
        fail "cargo-build-sbf in $TOOLCHAIN cannot build bridge/solana with platform tools $PLATFORM_TOOLS, so there is no program to deploy: the cargo of those platform tools reports $missing"
    fi
    fail "deploy-solana-program.sh did not deploy, initialise and register the program"
fi
expect "chain in the record" "$CHAIN" "$(jq -r '.chain' "$RECORD")"
expect "chain id in the record" "$CHAIN_ID" "$(jq -r '.chain_id' "$RECORD")"
expect "program id in the record" "$PROGRAM_ID" "$(jq -r '.program_id' "$RECORD")"
expect "publisher in the record" "$PUBLISHER" "$(jq -r '.publisher' "$RECORD")"
expect "upgrade authority in the record" "$PUBLISHER" "$(jq -r '.upgrade_authority' "$RECORD")"
expect "owner in the record" "$OWNER" "$(jq -r '.owner' "$RECORD")"
expect "threshold in the record" "$THRESHOLD" "$(jq -r '.threshold' "$RECORD")"
expect "assets in the record" "SOL,SID" "$(jq -r '[.assets[].symbol] | join(",")' "$RECORD")"
cli solana program dump "$PROGRAM_ID" "$WORK/deployed.so" > "$WORK/dump.log" 2>&1 || {
    sed -e 's/^/    /' "$WORK/dump.log" >&2
    fail "the deployed program could not be read back"
}
expect "ELF hash of the deployed program" "$(jq -r '.program_elf_sha256' "$RECORD")" \
    "$(head -c "$(jq -r '.program_elf_bytes' "$RECORD")" "$WORK/deployed.so" | sha256sum | cut -d ' ' -f 1)"
VAULT_LINE=$("$KEYS" vault "$PROGRAM_ID") || fail "bridge/vectors derives no vault authority for $PROGRAM_ID"
VAULT=${VAULT_LINE%% *}
VAULT_HANDLE=${VAULT_LINE#* }
expect "vault authority the Solana CLI derives" "$VAULT" \
    "$(cli solana find-program-derived-address "$PROGRAM_ID" string:vault-authority | head -n 1)"

# Custody accounts of the vault, the depositor's and the recipient's token
# accounts, wrapped SOL and the run's Sidiora for the depositor.
spl() {
    local label=$1
    shift
    cli spl-token --fee-payer "$WORK/keys/publisher.json" "$@" > "$WORK/spl.log" 2>&1 || {
        sed -e 's/^/    /' "$WORK/spl.log" >&2
        fail "spl-token could not $label"
    }
}
ata() { pda "$ASSOCIATED_TOKEN_PROGRAM" "$(key_hex "$1")" "$(key_hex "$TOKEN_PROGRAM")" "$(key_hex "$2")"; }
spl "open the vault's wrapped SOL account" create-account "$WRAPPED_SOL_MINT" "$WORK/keys/vault-sol.json" --owner "$VAULT"
spl "open the vault's Sidiora account" create-account "$SIDIORA_MINT" "$WORK/keys/vault-sid.json" --owner "$VAULT"
DEPOSITOR_SOL=$(ata "$DEPOSITOR" "$WRAPPED_SOL_MINT")
DEPOSITOR_SID=$(ata "$DEPOSITOR" "$SIDIORA_MINT")
RECIPIENT_SOL=$(ata "$RECIPIENT" "$WRAPPED_SOL_MINT")
RECIPIENT_SID=$(ata "$RECIPIENT" "$SIDIORA_MINT")
# The depositor's wrapped SOL account is opened at the publisher's expense and
# then holds exactly the SOL the depositor moves into it, so the wrapped amount
# is not reduced by the account's rent.
spl "open the depositor's wrapped SOL account" create-account "$WRAPPED_SOL_MINT" --owner "$DEPOSITOR"
cli solana transfer --keypair "$WORK/keys/depositor.json" "$DEPOSITOR_SOL" "$WRAPPED_SOL" \
    > "$WORK/wrap.log" 2>&1 || {
    sed -e 's/^/    /' "$WORK/wrap.log" >&2
    fail "the depositor's SOL was not moved into its wrapped SOL account"
}
spl "wrap SOL for the depositor" sync-native --address "$DEPOSITOR_SOL"
spl "open the depositor's Sidiora account" create-account "$SIDIORA_MINT" --owner "$DEPOSITOR"
spl "mint the run's Sidiora to the depositor" mint "$SIDIORA_MINT" "$((SIDIORA_MINTED / 1000000))" "$DEPOSITOR_SID" \
    --mint-authority "$WORK/keys/sidiora-authority.json"
spl "open the recipient's wrapped SOL account" create-account "$WRAPPED_SOL_MINT" --owner "$RECIPIENT"
spl "open the recipient's Sidiora account" create-account "$SIDIORA_MINT" --owner "$RECIPIENT"
"${TX[@]}" settle "$VALIDATOR_URL" || fail "the setup of the token accounts was not finalized"
expect "depositor's wrapped SOL" "$((WRAPPED_SOL * 1000000000))" \
    "$(token_amount "$VALIDATOR_URL" "$DEPOSITOR_SOL" "$WRAPPED_SOL_MINT" "$DEPOSITOR")"
expect "depositor's Sidiora" "$SIDIORA_MINTED" "$(token_amount "$VALIDATOR_URL" "$DEPOSITOR_SID" "$SIDIORA_MINT" "$DEPOSITOR")"

CONFIG_ACCOUNT=$(pda "$PROGRAM_ID" "$(hex_of config)")
asset_account() { pda "$PROGRAM_ID" "$(hex_of asset)" "$(key_hex "$1")"; }
random_hex() { od -An -N"$1" -tx1 /dev/urandom | tr -d ' \n'; }

# send_instructions sends one transaction built from a JSON list of
# instructions, signed by the keypair files given, the payer first.
send_instructions() {
    local label=$1 instructions=$2
    shift 2
    "${TX[@]}" send "$VALIDATOR_URL" "$instructions" "$@" 2> "$WORK/send.log" || {
        sed -e 's/^/    /' "$WORK/send.log" >&2
        fail "$label was not accepted"
    }
}

# deposit locks an amount of a mint for a Paxeer address and asserts the receipt
# the next nonce seeds.
RECEIPTS='[]'
NONCE=0
deposit() {
    local mint=$1 source=$2 vault_token=$3 amount=$4 paxeer receipt data
    paxeer="000000000000000000000000$(random_hex 20)"
    NONCE=$((NONCE + 1))
    receipt=$(receipt_address "$PROGRAM_ID" "$NONCE")
    data="${INSTRUCTION_PREFIX}${OP_DEPOSIT}$(printf '%016x' "$amount")${paxeer}"
    jq -n --arg program "$PROGRAM_ID" --arg data "$data" --arg depositor "$DEPOSITOR" \
        --arg config "$CONFIG_ACCOUNT" --arg asset "$(asset_account "$mint")" --arg mint "$mint" \
        --arg source "$source" --arg vault "$vault_token" --arg receipt "$receipt" \
        --arg token "$TOKEN_PROGRAM" --arg system "$SYSTEM_PROGRAM" \
        '[{program: $program, data: $data, accounts: [[$depositor, true, true], [$config, false, true],
          [$asset, false, true], [$mint, false, false], [$source, false, true], [$vault, false, true],
          [$receipt, false, true], [$token, false, false], [$system, false, false]]}]' > "$WORK/deposit.json"
    send_instructions "the deposit of $amount of $mint" "$WORK/deposit.json" "$WORK/keys/depositor.json" > /dev/null
    RECEIPTS=$(jq -c --arg account "$receipt" --arg nonce "$NONCE" --arg mint "$mint" \
        --arg mint_hex "$(key_hex "$mint")" --arg amount "$amount" --arg recipient "0x$paxeer" \
        --arg depositor_hex "$(key_hex "$DEPOSITOR")" \
        '. + [{account: $account, nonce: $nonce, mint: $mint, mint_hex: $mint_hex, amount: $amount,
               paxeer_recipient: $recipient, depositor_hex: $depositor_hex}]' <<< "$RECEIPTS")
    printf 'solana-dry-run-check: deposit %s locked %s of %s\n' "$NONCE" "$amount" "$mint"
}
deposit "$WRAPPED_SOL_MINT" "$DEPOSITOR_SOL" "$VAULT_SOL" "$SOL_DEPOSIT"
deposit "$SIDIORA_MINT" "$DEPOSITOR_SID" "$VAULT_SID" "$SIDIORA_DEPOSIT"

# release pays one Paxeer burn out against the threshold of attestor signatures
# over the outbound preimage, verified by the native secp256k1 program in the
# instruction before it, and proves the same burn cannot be paid twice.
DOMAIN_HEX=$(hex_of "$OUTBOUND_DOMAIN")
RECIPIENT_HANDLE=$("$KEYS" handle "$RECIPIENT") || fail "bridge/vectors derives no handle for $RECIPIENT"
NULLIFIERS='[]'
release() {
    local mint=$1 asset_id=$2 vault_token=$3 recipient_token=$4 amount=$5 paxeer_nonce=$6
    local burn preimage digest index signature entries=() secp nullifier nullifier_account data status=0
    burn=$(random_hex 32)
    preimage="${DOMAIN_HEX}$(printf '%064x' "$CHAIN_ID")${VAULT_HANDLE#0x}${burn}$(printf '%016x' "$paxeer_nonce")"
    preimage+="${RECIPIENT_HANDLE#0x}${asset_id#0x}$(printf '%064x' "$amount")"
    [ "${#preimage}" -eq $((2 * 185)) ] || fail "the outbound preimage is not 185 bytes"
    digest=$(cast keccak "0x$preimage")
    for ((index = 0; index < THRESHOLD; index++)); do
        signature=$(cast wallet sign --no-hash --private-key "${ATTESTOR_KEYS[index]}" "$digest") \
            || fail "attestor ${ATTESTORS[index]} did not sign the digest of burn $paxeer_nonce"
        [[ $signature =~ ^0x[0-9a-fA-F]{130}$ ]] || fail "attestor ${ATTESTORS[index]} signed with $signature"
        entries+=("$signature:${ATTESTORS[index]}")
    done
    secp=$("${TX[@]}" secp-data "$preimage" "${entries[@]}") || fail "the secp256k1 instruction of burn $paxeer_nonce was not built"
    nullifier=$(cast keccak "0x${burn}$(printf '%016x' "$paxeer_nonce")")
    nullifier=${nullifier#0x}
    nullifier_account=$(nullifier_address "$PROGRAM_ID" "$nullifier")
    data="${INSTRUCTION_PREFIX}${OP_RELEASE}${burn}$(printf '%016x' "$paxeer_nonce")$(key_hex "$RECIPIENT")$(printf '%016x' "$amount")"
    jq -n --arg secp_program "$SECP256K1_PROGRAM" --arg secp "$secp" --arg program "$PROGRAM_ID" \
        --arg data "$data" --arg payer "$PUBLISHER" --arg config "$CONFIG_ACCOUNT" \
        --arg asset "$(asset_account "$mint")" --arg mint "$mint" --arg vault "$VAULT" \
        --arg vault_token "$vault_token" --arg recipient_token "$recipient_token" \
        --arg nullifier "$nullifier_account" --arg instructions "$INSTRUCTIONS_SYSVAR" \
        --arg token "$TOKEN_PROGRAM" --arg system "$SYSTEM_PROGRAM" \
        '[{program: $secp_program, data: $secp, accounts: []},
          {program: $program, data: $data, accounts: [[$payer, true, true], [$config, false, false],
          [$asset, false, true], [$mint, false, false], [$vault, false, false], [$vault_token, false, true],
          [$recipient_token, false, true], [$nullifier, false, true], [$instructions, false, false],
          [$token, false, false], [$system, false, false]]}]' > "$WORK/release.json"
    send_instructions "the release of burn $paxeer_nonce" "$WORK/release.json" "$WORK/keys/publisher.json" > /dev/null
    jq --arg payer "$DEPOSITOR" '.[1].accounts[0][0] = $payer' "$WORK/release.json" > "$WORK/again.json"
    "${TX[@]}" send "$VALIDATOR_URL" "$WORK/again.json" "$WORK/keys/depositor.json" > /dev/null 2> "$WORK/again.log" \
        || status=$?
    [ "$status" -ne 0 ] || fail "the program paid burn $paxeer_nonce a second time"
    grep -qE "\"Custom\": ?$REPLAYED_ERROR\b|custom program error: 0x$(printf '%x' "$REPLAYED_ERROR")\b" "$WORK/again.log" || {
        sed -e 's/^/    /' "$WORK/again.log" >&2
        fail "a second payment of burn $paxeer_nonce was refused for another reason than its spent nullifier"
    }
    NULLIFIERS=$(jq -c --arg account "$nullifier_account" --arg nullifier "$nullifier" --arg burn "0x$burn" \
        --arg nonce "$paxeer_nonce" --arg mint_hex "$(key_hex "$mint")" --arg recipient_hex "$(key_hex "$RECIPIENT")" \
        --arg amount "$amount" \
        '. + [{account: $account, nullifier: $nullifier, paxeer_tx_hash: $burn, paxeer_nonce: $nonce,
               mint_hex: $mint_hex, recipient_hex: $recipient_hex, amount: $amount}]' <<< "$NULLIFIERS")
    printf 'solana-dry-run-check: burn %s released %s of %s against %s signatures; a second release is refused\n' \
        "$paxeer_nonce" "$amount" "$mint" "$THRESHOLD"
}
release "$WRAPPED_SOL_MINT" "$(lower "$(jq -r '.assets[0].asset_id' "$CONFIG")")" "$VAULT_SOL" "$RECIPIENT_SOL" "$SOL_RELEASE" 1
release "$SIDIORA_MINT" "$SIDIORA_ASSET_ID" "$VAULT_SID" "$RECIPIENT_SID" "$SIDIORA_RELEASE" 2

EXPECTATIONS="$WORK/expectations.json"
jq -n --arg program "$PROGRAM_ID" --argjson receipts "$RECEIPTS" --argjson nullifiers "$NULLIFIERS" \
    --arg sol "$WRAPPED_SOL_MINT" --arg sid "$SIDIORA_MINT" --arg vault "$VAULT" --arg recipient "$RECIPIENT" \
    --arg vault_sol "$VAULT_SOL" --arg vault_sid "$VAULT_SID" \
    --arg recipient_sol "$RECIPIENT_SOL" --arg recipient_sid "$RECIPIENT_SID" \
    --arg sol_custody "$((SOL_DEPOSIT - SOL_RELEASE))" --arg sid_custody "$((SIDIORA_DEPOSIT - SIDIORA_RELEASE))" \
    --arg sol_paid "$SOL_RELEASE" --arg sid_paid "$SIDIORA_RELEASE" \
    '{program_id: $program, receipts: $receipts, nullifiers: $nullifiers,
      balances: [
        {label: "wrapped SOL in the vault", account: $vault_sol, mint: $sol, owner: $vault, amount: $sol_custody},
        {label: "Sidiora in the vault", account: $vault_sid, mint: $sid, owner: $vault, amount: $sid_custody},
        {label: "wrapped SOL released to the recipient", account: $recipient_sol, mint: $sol, owner: $recipient, amount: $sol_paid},
        {label: "Sidiora released to the recipient", account: $recipient_sid, mint: $sid, owner: $recipient, amount: $sid_paid}]}' \
    > "$EXPECTATIONS"

# The Paxeer side: the real layerxbridge keeper executes the governance bodies
# generated for this deployment and registers Sidiora for Solana the way the
# chain's upgrade handler does, and its precompile answers the views.
BODIES="$WORK/bodies"
(cd "$REPO_ROOT" && go run ./bridge/deploy/proposals/cmd/paxeer-bridge-proposals -manifest "$MANIFEST" \
    -authority "$GOVERNANCE_AUTHORITY" -vault "$VAULT_HANDLE" "$CONFIG" "$BODIES") > "$WORK/proposals.log" 2>&1 || {
    sed -e 's/^/    /' "$WORK/proposals.log" >&2
    fail "the governance bodies were not generated"
}
GO_PACKAGE=$(mktemp -d "$SCRIPT_DIR/paxeerside_XXXXXX")
GO_PACKAGES+=("$GO_PACKAGE")
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
	typed, ok := msg.(*M)
	if !ok {
		t.Fatalf("%s decoded into %T, want %T", path, msg, (*M)(nil))
	}
	return *typed
}

func TestPaxeerSide(t *testing.T) {
	bodies, portFile := os.Getenv("PAXEER_SIDE_BODIES"), os.Getenv("PAXEER_SIDE_PORT_FILE")
	if bodies == "" || portFile == "" {
		t.Fatal("PAXEER_SIDE_BODIES and PAXEER_SIDE_PORT_FILE are required")
	}
	testApp := app.Setup(t, false, false, false)
	k, ctx := bridgetestutil.NewKeeper(testApp, testApp.GetContextForDeliverTx([]byte{}))
	k.InitGenesis(ctx, *types.DefaultGenesis())

	register := decode[types.MsgRegisterChain](t, filepath.Join(bodies, "01-register-chain.json"))
	if err := k.RegisterChain(ctx, register); err != nil {
		t.Fatalf("01-register-chain.json: %v", err)
	}
	attestors := decode[types.MsgSetAttestors](t, filepath.Join(bodies, "02-set-attestors.json"))
	if err := k.SetAttestors(ctx, attestors); err != nil {
		t.Fatalf("02-set-attestors.json: %v", err)
	}
	denom, err := k.EnsureSidioraDenom(ctx, register.Chain.ChainID)
	if err != nil {
		t.Fatalf("EnsureSidioraDenom(%d): %v", register.Chain.ChainID, err)
	}
	if denom != types.SidioraDenom() {
		t.Fatalf("EnsureSidioraDenom(%d) registered %s", register.Chain.ChainID, denom)
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
PAXEER_SIDE_BODIES="$BODIES" PAXEER_SIDE_PORT_FILE="$WORK/paxeer.port" \
    "$WORK/paxeer-side.test" -test.run '^TestPaxeerSide$' -test.timeout 0 > "$WORK/paxeer.log" 2>&1 &
PAXEER_PID=$!
PIDS+=("$PAXEER_PID")
wait_for_file "$WORK/paxeer.port" "$PAXEER_PID" "the Paxeer side" "$WORK/paxeer.log"
PAXEER_URL="http://127.0.0.1:$(cat "$WORK/paxeer.port")"

# Every read from here on passes through the recording proxy.
jq -n --arg chain "$CHAIN" --arg validator "$VALIDATOR_URL" --arg paxeer "$PAXEER_URL" \
    '{($chain): $validator, paxeer: $paxeer}' > "$WORK/routes.json"
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
stop "$VALIDATOR_PID"
rm -rf "$WORK/ledger"

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

RECORDING="$WORK/solana_dry_run.json"
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
for keypair in "$WORK"/keys/*.json; do
    ! grep -qiF -- "$(jq -r '.[0:32][]' "$keypair" | awk '{printf "%02x", $1}')" "$RECORDING" \
        || fail "the recording carries the secret of $(basename "$keypair")"
    ! grep -qF -- "$("${TX[@]}" secret "$keypair")" "$RECORDING" \
        || fail "the recording carries the keypair $(basename "$keypair")"
done
while read -r attestor_key; do
    ! grep -qi -- "${attestor_key#0x}" "$RECORDING" || fail "the recording carries a private key of the run"
done < <(jq -r '.[].private_key' "$WORK/attestors.keys.json")
! grep -qE 'https?://|127\.0\.0\.1|localhost' "$RECORDING" || fail "the recording carries an endpoint"
! grep -qE '[0-9]{4}-[0-9]{2}-[0-9]{2}' "$RECORDING" || fail "the recording carries a date"
host=$(hostname 2> /dev/null || true)
[ -z "$host" ] || ! grep -qiF -- "$host" "$RECORDING" || fail "the recording carries the hostname"
! grep -qF -- "$WORK" "$RECORDING" || fail "the recording carries a path of the run"
! grep -qF -- "$HOME" "$RECORDING" || fail "the recording carries a home directory"

replay_fixture "$RECORDING" "recording of this run"
if [ "$record" -eq 1 ]; then
    jq . "$RECORDING" > "$FIXTURE"
    printf 'solana-dry-run-check: the recording is written to %s\n' "${FIXTURE#"$REPO_ROOT"/}"
else
    replay_fixture "$FIXTURE" "committed fixture"
fi
printf 'solana-dry-run-check: every step behaves\n'
