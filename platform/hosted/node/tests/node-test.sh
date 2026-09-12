#!/usr/bin/env bash
# Real-process test for the beta node bring-up: bootstraps a temporary data
# directory, runs the replica and sequencer supervisors against build/bin,
# proves the LNI handshake with layerx-client, reads the treasury balance,
# exercises the supervisor reset socket, and stops everything.
#
# Runs as root (or with CAP_SETUID) so the LNI client can present a uid that
# differs from the daemon uid, as the LNI requires. Override the client
# identity with LAYERX_NODE_TEST_CLIENT_UID / LAYERX_NODE_TEST_CLIENT_GID.
# The supervisors need socat: LAYERX_TEST_SOCAT_BIN names it, otherwise the
# first socat on PATH is used. The genesis metadata the bootstrap requires is
# built from tests/support/lxgb_metadata.py over the beta asset and the
# treasury key generated here.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../../../.." && pwd)
NODE_DIR="$ROOT/platform/hosted/node"
NATIVE_BIN_DIR=${LAYERX_TEST_NATIVE_BIN_DIR:-$ROOT/build/bin}
LAYERXD="$NATIVE_BIN_DIR/layerxd"
GENESIS_BUILD="$NATIVE_BIN_DIR/layerx-genesis-build"
CARGO=${PLATFORM_CARGO:-cargo}
NETWORK_ID=${LAYERX_NODE_TEST_NETWORK_ID:-4242}
ASSET_ID=b5a32b12029f8ddfb905f90f280f664b46390de0fc62770fc197dd87b18cd898
PROGRAM_PORT=${LAYERX_NODE_TEST_PROGRAM_PORT:-19401}
REPLICA_PORT=${LAYERX_NODE_TEST_REPLICA_PORT:-19402}

log() { printf 'node-test: %s\n' "$*" >&2; }
fail() { log "FAIL: $*"; exit 1; }

[ -x "$LAYERXD" ] || fail "$LAYERXD missing; run make layerxd"
[ -x "$GENESIS_BUILD" ] || fail "$GENESIS_BUILD missing; run make layerx-genesis-build"
for tool in openssl setpriv od sha256sum jq python3; do
    command -v "$tool" >/dev/null || fail "$tool is required"
done
if [ -n "${LAYERX_TEST_SOCAT_BIN:-}" ]; then
    SOCAT=$LAYERX_TEST_SOCAT_BIN
    [ -f "$SOCAT" ] && [ -x "$SOCAT" ] || fail "socat_invalid: LAYERX_TEST_SOCAT_BIN=$SOCAT is not an executable file"
else
    SOCAT=$(command -v socat || true)
    [ -n "$SOCAT" ] || fail "socat_missing: no socat executable on PATH; set LAYERX_TEST_SOCAT_BIN"
fi
[ "$(id -u)" -eq 0 ] || fail "must run as root so the LNI client can present a distinct uid"
CLIENT_UID=${LAYERX_NODE_TEST_CLIENT_UID:-$(id -u nobody)}
CLIENT_GID=${LAYERX_NODE_TEST_CLIENT_GID:-$(id -g nobody)}
[ "$CLIENT_UID" != "$(id -u)" ] || fail "client uid must differ from the daemon uid"

log "building the probe"
"$CARGO" build --locked --offline --release --manifest-path "$NODE_DIR/tests/probe/Cargo.toml" >&2
PROBE="$NODE_DIR/tests/probe/target/release/layerx-node-probe"
[ -x "$PROBE" ] || fail "probe binary missing at $PROBE"

WORK=$(mktemp -d /tmp/layerx-node-test.XXXXXX)
chmod 0755 "$WORK"
cp "$PROBE" "$WORK/probe"
cp "$NATIVE_BIN_DIR/layerxctl" "$WORK/layerxctl"
chmod 0755 "$WORK/probe"
chmod 0755 "$WORK/layerxctl"
DATA="$WORK/data"
RUN="$WORK/run"
SEQUENCER_PID=""
REPLICA_PID=""
KEEP=1

cleanup() {
    local pid
    for pid in "$SEQUENCER_PID" "$REPLICA_PID"; do
        if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
            kill -TERM "$pid" 2>/dev/null || true
        fi
    done
    for pid in "$SEQUENCER_PID" "$REPLICA_PID"; do
        [ -n "$pid" ] && wait "$pid" 2>/dev/null || true
    done
    pkill -TERM -f "$LAYERXD --serve $DATA/" 2>/dev/null || true
    pkill -TERM -f "$LAYERXD --authority-replica $DATA/" 2>/dev/null || true
    if [ "$KEEP" -eq 1 ]; then
        log "logs retained under $WORK"
        for logfile in "$WORK/sequencer.log" "$WORK/replica.log"; do
            [ -r "$logfile" ] && { printf -- '--- %s\n' "$logfile" >&2; tail -n 40 "$logfile" >&2; }
        done
    else
        rm -rf "$WORK"
    fi
}
trap cleanup EXIT

umask 077
openssl rand 32 > "$WORK/sequencer.key"
openssl rand 32 > "$WORK/treasury.key"
umask 022
TREASURY_PUBLIC=$({ printf '\x30\x2e\x02\x01\x00\x30\x05\x06\x03\x2b\x65\x70\x04\x22\x04\x20'; cat "$WORK/treasury.key"; } \
    | openssl pkey -inform DER -pubout -outform DER | tail -c 32 | od -An -v -tx1 | tr -d ' \n')
[ ${#TREASURY_PUBLIC} -eq 64 ] || fail "could not derive the treasury public key"
METADATA="$WORK/genesis-metadata.lxgb"
python3 - "$ROOT/tests/support/lxgb_metadata.py" "$ASSET_ID" "$TREASURY_PUBLIC" "$METADATA" <<'LXGB' || fail "could not build the genesis metadata"
import importlib.util, os, sys
spec = importlib.util.spec_from_file_location('lxgb_metadata', sys.argv[1])
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
module.check()
with open(sys.argv[4], 'wb') as output:
    output.write(module.metadata(bytes.fromhex(sys.argv[2]), bytes.fromhex(sys.argv[3]), os.urandom(32)))
LXGB
chmod 0644 "$METADATA"

as_client() {
    setpriv --reuid="$CLIENT_UID" --regid="$CLIENT_GID" --clear-groups "$@"
}

wait_for() {
    # wait_for PATH SECONDS
    local deadline=$(( $(date +%s) + $2 ))
    while [ ! -e "$1" ]; do
        if [ -n "$SEQUENCER_PID" ] && ! kill -0 "$SEQUENCER_PID" 2>/dev/null; then
            fail "sequencer supervisor exited while waiting for $1"
        fi
        if [ -n "$REPLICA_PID" ] && ! kill -0 "$REPLICA_PID" 2>/dev/null; then
            fail "replica supervisor exited while waiting for $1"
        fi
        [ "$(date +%s)" -lt "$deadline" ] || fail "timed out waiting for $1"
        sleep 0.2
    done
}

expect_contains() {
    case "$1" in *"$2"*) ;; *) fail "expected $2 in: $1" ;; esac
}

log "starting the replica supervisor"
bash "$NODE_DIR/supervisor.sh" --role replica --data-dir "$DATA" --run-dir "$RUN" \
    --layerxd "$LAYERXD" --socat "$SOCAT" > "$WORK/replica.log" 2>&1 &
REPLICA_PID=$!

log "starting the sequencer supervisor (bootstraps $DATA)"
bash "$NODE_DIR/supervisor.sh" --role sequencer --data-dir "$DATA" --run-dir "$RUN" \
    --layerxd "$LAYERXD" --socat "$SOCAT" -- \
    --network-id "$NETWORK_ID" --asset "$ASSET_ID" --genesis-metadata "$METADATA" \
    --sequencer-key "$WORK/sequencer.key" --treasury-key "$WORK/treasury.key" \
    --lni-uid "$CLIENT_UID" --lni-gid "$CLIENT_GID" \
    --program-port "$PROGRAM_PORT" --replica-port "$REPLICA_PORT" \
    --migrations "$ROOT/migrations/0007_history_index.sql" \
    --genesis-build "$GENESIS_BUILD" > "$WORK/sequencer.log" 2>&1 &
SEQUENCER_PID=$!

wait_for "$DATA/node.env" 60
wait_for "$RUN/layerxd.lni.sock" 60
wait_for "$RUN/supervisor.sock" 60
set -a
# shellcheck disable=SC1091
. "$DATA/node.env"
set +a
[ "$LAYERX_NODE_NETWORK_ID" = "$NETWORK_ID" ] || fail "node.env network id mismatch"
[ "$LAYERX_NODE_ASSET_ID" = "$ASSET_ID" ] || fail "node.env asset id mismatch"
[ "$LAYERX_NODE_TREASURY_PUBLIC_KEY" = "$TREASURY_PUBLIC" ] || fail "node.env treasury public key mismatch"
[ "$(stat -c %a "$RUN")" = 750 ] || fail "run directory is not mode 0750"
[ "$(stat -c %g "$RUN")" = "$CLIENT_GID" ] || fail "run directory group is not the LNI gid"
[ "$(stat -c %s "$DATA/genesis/genesis.registration")" = 82 ] || fail "bootstrap registration missing"
grep -q "^$(printf '%s' "$LAYERX_NODE_TREASURY_DID" | od -An -v -tx1 | tr -d ' \n'):$LAYERX_NODE_TREASURY_PUBLIC_KEY:0$" "$DATA/identities.txt" \
    || fail "treasury identity not registered"
[ "$LAYERX_NODE_GENESIS_GUARANTOR_COUNT" -eq "$(jq -r '.finality_policy.certificate_threshold' "$ROOT/contracts/config/checkpoint-settlement.json")" ] || fail "genesis guarantor count mismatch"
FIRST_MANIFEST_INODE=$(stat -c %i "$DATA/genesis/genesis.manifest")

log "LNI handshake as uid $CLIENT_UID"
HANDSHAKE=$(as_client "$WORK/probe" handshake --socket "$LAYERX_NODE_LNI_SOCKET" --network-id "$NETWORK_ID")
log "$HANDSHAKE"
expect_contains "$HANDSHAKE" "\"network_id\":$NETWORK_ID"
expect_contains "$HANDSHAKE" '"role":"Sequencer"'
expect_contains "$HANDSHAKE" "\"sequencer_public_key\":\"$LAYERX_NODE_SEQUENCER_PUBLIC_KEY\""
expect_contains "$HANDSHAKE" 'AccountRead'

log "operator state read over the real LNI"
OPERATOR_STATE=$(as_client "$WORK/layerxctl" read-state --socket "$LAYERX_NODE_LNI_SOCKET" \
    --network-id "$NETWORK_ID" --protocol-version 3 --actor "$LAYERX_NODE_TREASURY_DID")
expect_contains "$OPERATOR_STATE" "\"network_id\":$NETWORK_ID"
expect_contains "$OPERATOR_STATE" '"global_sequence":0'
expect_contains "$OPERATOR_STATE" '"evidence":"authenticated_node_snapshot"'

log "treasury balance read"
BALANCE=$(as_client "$WORK/probe" balance --socket "$LAYERX_NODE_LNI_SOCKET" --network-id "$NETWORK_ID" \
    --account "$LAYERX_NODE_TREASURY_ACCOUNT" --asset "$LAYERX_NODE_ASSET_ID")
log "$BALANCE"
expect_contains "$BALANCE" "\"balance\":\"$LAYERX_NODE_TREASURY_BALANCE\""
expect_contains "$BALANCE" "\"asset\":\"$LAYERX_NODE_ASSET_ID\""

log "operator submits a real signed SEND once and preserves its idempotency key"
chown "$CLIENT_UID:$CLIENT_GID" "$WORK/treasury.key"
mkdir "$WORK/operator"
chown "$CLIENT_UID:$CLIENT_GID" "$WORK/operator"
ACTIVITY_ID=$(as_client "$WORK/probe" write-send --socket "$LAYERX_NODE_LNI_SOCKET" \
    --network-id "$NETWORK_ID" --seed-file "$WORK/treasury.key" \
    --destination-did "did:layerx:$LAYERX_NODE_SEQUENCER_PUBLIC_KEY" \
    --asset "$LAYERX_NODE_ASSET_ID" --output "$WORK/operator/send.bin")
ADMISSION=$(as_client "$WORK/layerxctl" submit --socket "$LAYERX_NODE_LNI_SOCKET" \
    --network-id "$NETWORK_ID" --protocol-version 3 --actor "$LAYERX_NODE_TREASURY_DID" \
    --public-key "$LAYERX_NODE_TREASURY_PUBLIC_KEY" --activity "$WORK/operator/send.bin")
expect_contains "$ADMISSION" '"state":"acknowledged"'
expect_contains "$ADMISSION" "\"activity_id\":\"$ACTIVITY_ID\""
REPEATED_ADMISSION=$(as_client "$WORK/layerxctl" submit --socket "$LAYERX_NODE_LNI_SOCKET" \
    --network-id "$NETWORK_ID" --protocol-version 3 --actor "$LAYERX_NODE_TREASURY_DID" \
    --public-key "$LAYERX_NODE_TREASURY_PUBLIC_KEY" --activity "$WORK/operator/send.bin")
[ "$ADMISSION" = "$REPEATED_ADMISSION" ] || fail "repeated canonical submission changed identity"

log "supervisor status"
STATUS=$(as_client "$WORK/probe" supervisor --socket "$LAYERX_NODE_SUPERVISOR_SOCKET" --request status)
log "$STATUS"
expect_contains "$STATUS" '"state":"running","generation":1'

log "supervisor reset"
RESET=$(as_client "$WORK/probe" supervisor --socket "$LAYERX_NODE_SUPERVISOR_SOCKET" --request reset)
log "$RESET"
expect_contains "$RESET" '"state":"reset","reset_id":"'
wait_for "$RUN/layerxd.lni.sock" 60
set -a
# shellcheck disable=SC1091
. "$DATA/node.env"
set +a
[ "$(stat -c %i "$DATA/genesis/genesis.manifest")" != "$FIRST_MANIFEST_INODE" ] || fail "genesis was not rebuilt by the reset"
[ -z "$(ls -A "$DATA/checkpoints")" ] || fail "checkpoint directory was not discarded by the reset"
STATUS=$(as_client "$WORK/probe" supervisor --socket "$LAYERX_NODE_SUPERVISOR_SOCKET" --request status)
expect_contains "$STATUS" '"state":"running","generation":2'
HANDSHAKE=$(as_client "$WORK/probe" handshake --socket "$LAYERX_NODE_LNI_SOCKET" --network-id "$NETWORK_ID")
expect_contains "$HANDSHAKE" "\"network_id\":$NETWORK_ID"
BALANCE=$(as_client "$WORK/probe" balance --socket "$LAYERX_NODE_LNI_SOCKET" --network-id "$NETWORK_ID" \
    --account "$LAYERX_NODE_TREASURY_ACCOUNT" --asset "$LAYERX_NODE_ASSET_ID")
expect_contains "$BALANCE" "\"balance\":\"$LAYERX_NODE_TREASURY_BALANCE\""

log "stopping"
kill -TERM "$SEQUENCER_PID" "$REPLICA_PID"
wait "$SEQUENCER_PID" || true
wait "$REPLICA_PID" || true
SEQUENCER_PID=""
REPLICA_PID=""
if pgrep -f "$LAYERXD --serve $DATA/" >/dev/null || pgrep -f "$LAYERXD --authority-replica $DATA/" >/dev/null; then
    fail "layerxd processes survived the supervisors"
fi
[ ! -e "$RUN/supervisor.sock" ] || fail "supervisor socket was not removed"
KEEP=0
log "PASS"
