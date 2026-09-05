#!/usr/bin/env bash
# LayerX beta node bootstrap.
#
# Generates a signed genesis manifest and snapshot with layerx-genesis-build,
# the bootstrap registration the sequencer daemon verifies at first start, the
# identity file that registers the treasury signer, the eight-line layerxd
# configurations for the sequencer and the receipt-authority replica, and the
# environment files the supervisor sources when it starts both daemons.
#
# Usage:
#   bootstrap.sh --data-dir DIR --run-dir DIR --network-id N \
#       --sequencer-key FILE --treasury-key FILE [options]
#
# Required:
#   --data-dir DIR          Node data directory (created 0700; must be empty
#                           unless --force is given, which discards its contents).
#   --run-dir DIR           Directory holding the LNI socket and the supervisor
#                           socket. Made mode 0750 and owned by the daemon uid
#                           with --lni-gid as its group. Must differ from the
#                           data directory.
#   --network-id N          Decimal network id, 1..4294967295.
#   --sequencer-key FILE    Sequencer ed25519 seed: 32 raw bytes or 64 hex
#                           characters. Signs genesis and every batch.
#   --treasury-key FILE     Treasury ed25519 seed (same format). The treasury
#                           DID did:layerx:<public-key-hex> is registered as
#                           an identity so the admin plane can sign from it.
#
# Options:
#   --asset HEX64           Genesis asset id (32 bytes hex). Default: the beta
#                           asset sha256("layerx-beta-asset:LXT").
#   --treasury-balance N    Treasury balance the genesis carries. The protocol
#                           genesis manifest admits only the three system
#                           accounts at balance zero, so any value other than 0
#                           is refused with a typed error instead of being
#                           silently dropped. Default 0.
#   --program-port P        Daemon program listener port on 127.0.0.1. Default 9401.
#   --replica-port P        Receipt-authority replica port on 127.0.0.1. Default 9402.
#   --lni-uid U             Uid the LNI admits (must differ from the daemon uid).
#                           Default: 4021.
#   --lni-gid G             Gid the LNI admits and the run directory group.
#                           Default: the daemon's primary gid.
#   --program-token-file F  Bearer token for the program listener (32..128
#                           printable bytes). Default: generated.
#   --replica-token-file F  Bearer token for the replica listener (32..128
#                           bytes, distinct from the program token). Default: generated.
#   --replica-id HEX64      Receipt-authority replica id. Default: derived from
#                           the sequencer public key.
#   --genesis-timestamp-ms T  Genesis timestamp in milliseconds. Default: now.
#   --migrations FILE       History migration SQL. Default: repository
#                           migrations/0007_history_index.sql or
#                           /opt/layerx/migrations/0007_history_index.sql.
#   --layerxd PATH          layerxd binary. Default: build/bin/layerxd or
#                           /usr/local/bin/layerxd.
#   --genesis-build PATH    layerx-genesis-build binary. Same lookup.
#   --force                 Discard the data directory contents first.
#
# Outputs under DATA_DIR:
#   genesis/genesis.manifest, genesis/00000000000000000000.lxs,
#   genesis/paxeer-registration-request.lxrr, genesis/paxeer-deployment-descriptor.lxgd,
#   genesis/genesis.registration (LXGR bootstrap registration),
#   identities.txt, checkpoints/, logs/, replica/, secrets/{program-token,replica-token},
#   sequencer.conf, replica.conf, sequencer.env, replica.env, node.env, treasury.json
set -euo pipefail

usage() {
    sed -n '2,60p' "$0" | sed 's/^# \{0,1\}//'
    exit 2
}

fail() {
    printf 'bootstrap: %s\n' "$*" >&2
    exit 1
}

DATA_DIR=""
RUN_DIR=""
NETWORK_ID=""
SEQUENCER_KEY_FILE=""
TREASURY_KEY_FILE=""
ASSET_ID="b5a32b12029f8ddfb905f90f280f664b46390de0fc62770fc197dd87b18cd898"
TREASURY_BALANCE=0
PROGRAM_PORT=9401
REPLICA_PORT=9402
LNI_UID=4021
LNI_GID=""
PROGRAM_TOKEN_FILE=""
REPLICA_TOKEN_FILE=""
REPLICA_ID=""
GENESIS_TIMESTAMP_MS=""
MIGRATIONS=""
LAYERXD=""
GENESIS_BUILD=""
FORCE=0

while [ $# -gt 0 ]; do
    case "$1" in
        --data-dir) DATA_DIR=$2; shift 2 ;;
        --run-dir) RUN_DIR=$2; shift 2 ;;
        --network-id) NETWORK_ID=$2; shift 2 ;;
        --sequencer-key) SEQUENCER_KEY_FILE=$2; shift 2 ;;
        --treasury-key) TREASURY_KEY_FILE=$2; shift 2 ;;
        --asset) ASSET_ID=$2; shift 2 ;;
        --treasury-balance) TREASURY_BALANCE=$2; shift 2 ;;
        --program-port) PROGRAM_PORT=$2; shift 2 ;;
        --replica-port) REPLICA_PORT=$2; shift 2 ;;
        --lni-uid) LNI_UID=$2; shift 2 ;;
        --lni-gid) LNI_GID=$2; shift 2 ;;
        --program-token-file) PROGRAM_TOKEN_FILE=$2; shift 2 ;;
        --replica-token-file) REPLICA_TOKEN_FILE=$2; shift 2 ;;
        --replica-id) REPLICA_ID=$2; shift 2 ;;
        --genesis-timestamp-ms) GENESIS_TIMESTAMP_MS=$2; shift 2 ;;
        --migrations) MIGRATIONS=$2; shift 2 ;;
        --layerxd) LAYERXD=$2; shift 2 ;;
        --genesis-build) GENESIS_BUILD=$2; shift 2 ;;
        --force) FORCE=1; shift ;;
        -h|--help) usage ;;
        *) fail "unknown argument $1" ;;
    esac
done

[ -n "$DATA_DIR" ] || fail "--data-dir is required"
[ -n "$RUN_DIR" ] || fail "--run-dir is required"
[ -n "$NETWORK_ID" ] || fail "--network-id is required"
[ -n "$SEQUENCER_KEY_FILE" ] || fail "--sequencer-key is required"
[ -n "$TREASURY_KEY_FILE" ] || fail "--treasury-key is required"

is_decimal() { [[ $1 =~ ^[0-9]+$ ]]; }
is_hex64() { [[ $1 =~ ^[0-9a-f]{64}$ ]]; }

is_decimal "$NETWORK_ID" || fail "--network-id must be decimal"
[ "$NETWORK_ID" -ge 1 ] && [ "$NETWORK_ID" -le 4294967295 ] || fail "--network-id out of range"
is_decimal "$PROGRAM_PORT" && [ "$PROGRAM_PORT" -ge 1 ] && [ "$PROGRAM_PORT" -le 65535 ] || fail "--program-port out of range"
is_decimal "$REPLICA_PORT" && [ "$REPLICA_PORT" -ge 1 ] && [ "$REPLICA_PORT" -le 65535 ] || fail "--replica-port out of range"
[ "$PROGRAM_PORT" != "$REPLICA_PORT" ] || fail "--program-port and --replica-port must differ"
is_decimal "$LNI_UID" || fail "--lni-uid must be decimal"
is_decimal "$TREASURY_BALANCE" || fail "--treasury-balance must be decimal"
ASSET_ID=$(printf '%s' "$ASSET_ID" | tr 'A-F' 'a-f')
is_hex64 "$ASSET_ID" || fail "--asset must be 64 hex characters"
[ "$ASSET_ID" != "$(printf '0%.0s' $(seq 1 64))" ] || fail "--asset must not be zero"
if [ "$TREASURY_BALANCE" != 0 ]; then
    fail "treasury_balance_unsupported: the protocol genesis manifest (src/protocol/lxp_genesis.c validate) admits only the three system accounts at balance zero; the treasury is funded after genesis, not in it"
fi

DAEMON_UID=$(id -u)
DAEMON_GID=$(id -g)
[ -n "$LNI_GID" ] || LNI_GID=$DAEMON_GID
is_decimal "$LNI_GID" || fail "--lni-gid must be decimal"
[ "$LNI_UID" != "$DAEMON_UID" ] || fail "--lni-uid must differ from the daemon uid $DAEMON_UID"

resolve_binary() {
    local given=$1 name=$2 candidate
    if [ -n "$given" ]; then
        [ -x "$given" ] || fail "$name is not executable: $given"
        printf '%s' "$given"
        return
    fi
    for candidate in "$PWD/build/bin/$name" "/usr/local/bin/$name"; do
        if [ -x "$candidate" ]; then printf '%s' "$candidate"; return; fi
    done
    fail "$name not found; pass --${name#layerx-} or build it with make"
}

LAYERXD=$(resolve_binary "$LAYERXD" layerxd)
GENESIS_BUILD=$(resolve_binary "$GENESIS_BUILD" layerx-genesis-build)
if [ -z "$MIGRATIONS" ]; then
    for candidate in "$PWD/migrations/0007_history_index.sql" /opt/layerx/migrations/0007_history_index.sql; do
        if [ -r "$candidate" ]; then MIGRATIONS=$candidate; break; fi
    done
fi
[ -n "$MIGRATIONS" ] && [ -r "$MIGRATIONS" ] || fail "history migrations SQL not found; pass --migrations"
MIGRATIONS=$(readlink -f "$MIGRATIONS")

command -v openssl >/dev/null || fail "openssl is required"
command -v sha256sum >/dev/null || fail "sha256sum is required"
command -v od >/dev/null || fail "od is required"

bin_to_hex() { od -An -v -tx1 | tr -d ' \n'; }

hex_to_bin() {
    local hex=$1 i
    for ((i = 0; i < ${#hex}; i += 2)); do
        printf "\\$(printf '%03o' "0x${hex:i:2}")"
    done
}

be_hex() {
    # be_hex VALUE BYTES -> big-endian hex of VALUE padded to BYTES bytes
    printf "%0$(( $2 * 2 ))x" "$1"
}

sha256_hex() { sha256sum | cut -c1-64; }

load_seed_hex() {
    local file=$1 name=$2 size text
    [ -r "$file" ] || fail "$name key file is not readable: $file"
    size=$(stat -c %s "$file")
    if [ "$size" -eq 32 ]; then
        bin_to_hex < "$file"
        return
    fi
    text=$(tr -d ' \t\r\n' < "$file" | tr 'A-F' 'a-f')
    is_hex64 "$text" || fail "$name key file must hold 32 raw bytes or 64 hex characters"
    printf '%s' "$text"
}

public_key_hex() {
    # ed25519 public key from a 32-byte seed via the PKCS#8 wrapper openssl reads.
    { hex_to_bin "302e020100300506032b657004220420"; hex_to_bin "$1"; } \
        | openssl pkey -inform DER -pubout -outform DER | tail -c 32 | bin_to_hex
}

load_token() {
    local file=$1 name=$2 token
    [ -r "$file" ] || fail "$name token file is not readable: $file"
    token=$(tr -d '\r\n' < "$file")
    [ ${#token} -ge 32 ] && [ ${#token} -le 128 ] || fail "$name token must be 32..128 bytes"
    [[ $token =~ ^[\!-~]+$ ]] || fail "$name token must be printable ASCII without spaces"
    printf '%s' "$token"
}

SEQUENCER_PRIVATE=$(load_seed_hex "$SEQUENCER_KEY_FILE" sequencer)
TREASURY_PRIVATE=$(load_seed_hex "$TREASURY_KEY_FILE" treasury)
SEQUENCER_PUBLIC=$(public_key_hex "$SEQUENCER_PRIVATE")
TREASURY_PUBLIC=$(public_key_hex "$TREASURY_PRIVATE")
[ "$SEQUENCER_PUBLIC" != "$TREASURY_PUBLIC" ] || fail "sequencer and treasury keys must differ"
SEQUENCER_ID=$(printf 'layerx-sequencer:%s' "$SEQUENCER_PUBLIC" | sha256_hex)
GUARANTOR_ID=$(printf 'layerx-beta-guarantor:%s' "$SEQUENCER_PUBLIC" | sha256_hex)
if [ -z "$REPLICA_ID" ]; then
    REPLICA_ID=$(printf 'layerx-authority-replica:%s' "$SEQUENCER_PUBLIC" | sha256_hex)
fi
REPLICA_ID=$(printf '%s' "$REPLICA_ID" | tr 'A-F' 'a-f')
is_hex64 "$REPLICA_ID" || fail "--replica-id must be 64 hex characters"
[ -n "$GENESIS_TIMESTAMP_MS" ] || GENESIS_TIMESTAMP_MS=$(( $(date +%s) * 1000 ))
is_decimal "$GENESIS_TIMESTAMP_MS" && [ "$GENESIS_TIMESTAMP_MS" -gt 0 ] || fail "--genesis-timestamp-ms must be a positive decimal"

TREASURY_DID="did:layerx:$TREASURY_PUBLIC"
TREASURY_DID_HEX=$(printf '%s' "$TREASURY_DID" | bin_to_hex)
TREASURY_ACCOUNT="agent:$TREASURY_DID:main"

if [ -n "$PROGRAM_TOKEN_FILE" ]; then
    PROGRAM_TOKEN=$(load_token "$PROGRAM_TOKEN_FILE" program)
else
    PROGRAM_TOKEN=$(openssl rand -hex 32)
fi
if [ -n "$REPLICA_TOKEN_FILE" ]; then
    REPLICA_TOKEN=$(load_token "$REPLICA_TOKEN_FILE" replica)
else
    REPLICA_TOKEN=$(openssl rand -hex 32)
fi
[ "$PROGRAM_TOKEN" != "$REPLICA_TOKEN" ] || fail "program and replica tokens must differ"

DATA_DIR=$(readlink -f "$DATA_DIR" 2>/dev/null || printf '%s' "$DATA_DIR")
mkdir -p "$DATA_DIR"
chmod 0700 "$DATA_DIR"
DATA_DIR=$(readlink -f "$DATA_DIR")
mkdir -p "$RUN_DIR"
RUN_DIR=$(readlink -f "$RUN_DIR")
[ "$DATA_DIR" != "$RUN_DIR" ] || fail "--data-dir and --run-dir must differ"
case "$RUN_DIR" in "$DATA_DIR"/*) fail "--run-dir must not be inside --data-dir" ;; esac
if [ "$FORCE" -eq 1 ]; then
    find "$DATA_DIR" -mindepth 1 -delete
fi
if [ -n "$(ls -A "$DATA_DIR")" ]; then
    fail "data directory is not empty: $DATA_DIR (pass --force to discard it)"
fi
chgrp "$LNI_GID" "$RUN_DIR" 2>/dev/null || [ "$(stat -c %g "$RUN_DIR")" = "$LNI_GID" ] \
    || fail "cannot set the run directory group to $LNI_GID: $RUN_DIR"
chmod 0750 "$RUN_DIR"
LNI_SOCKET="$RUN_DIR/layerxd.lni.sock"
SUPERVISOR_SOCKET="$RUN_DIR/supervisor.sock"
[ ${#LNI_SOCKET} -lt 108 ] || fail "LNI socket path is too long: $LNI_SOCKET"

umask 077
mkdir -p "$DATA_DIR/checkpoints" "$DATA_DIR/logs" "$DATA_DIR/replica" "$DATA_DIR/secrets" "$DATA_DIR/work"

# --- genesis request (LXGB v1) -------------------------------------------
PARAMETER_KEY=$(printf 'parameter-version' | bin_to_hex)
PARAMETER_KEY="$PARAMETER_KEY$(printf '0%.0s' $(seq 1 $(( 64 - ${#PARAMETER_KEY} ))))"
PARAMETER_VALUE="$(printf '0%.0s' $(seq 1 56))00000001"
REQUEST="$DATA_DIR/work/genesis-request.lxgb"
{
    printf 'LXGB'
    hex_to_bin 01
    hex_to_bin "$(be_hex 2 2)"
    hex_to_bin "$(be_hex "$NETWORK_ID" 4)"
    hex_to_bin "$(be_hex "$GENESIS_TIMESTAMP_MS" 8)"
    hex_to_bin "$(be_hex 1 2)"
    hex_to_bin "$(be_hex 7 2)"
    hex_to_bin "$PARAMETER_KEY"
    hex_to_bin "$PARAMETER_VALUE"
    hex_to_bin "$(be_hex 1 2)"
    hex_to_bin "$GUARANTOR_ID"
    hex_to_bin "02$SEQUENCER_PUBLIC"
    hex_to_bin "$(be_hex 0 16)"
    hex_to_bin "$ASSET_ID"
    hex_to_bin "$(be_hex 1 4)"
    for coefficient in 1 1 1 1 1 8 8 64 8; do hex_to_bin "$(be_hex "$coefficient" 8)"; done
    hex_to_bin "$(be_hex 1 8)"
    hex_to_bin 01
    hex_to_bin "$(be_hex 1 4)"
    for price in 1 1 2 4 1 1 100; do hex_to_bin "$(be_hex "$price" 8)"; done
    for demand in 100 1 1 10 1 1000; do hex_to_bin "$(be_hex "$demand" 8)"; done
} > "$REQUEST"
[ "$(stat -c %s "$REQUEST")" -eq 395 ] || fail "genesis request has an unexpected length"

SIGNER_KEY="$DATA_DIR/work/genesis-signer.key"
hex_to_bin "$SEQUENCER_PRIVATE" > "$SIGNER_KEY"
chmod 0600 "$SIGNER_KEY" "$REQUEST"
GENESIS_DIR="$DATA_DIR/genesis"
"$GENESIS_BUILD" "$REQUEST" "$SIGNER_KEY" "$GENESIS_DIR" || fail "layerx-genesis-build refused the genesis request"
rm -f "$SIGNER_KEY"
MANIFEST="$GENESIS_DIR/genesis.manifest"
SNAPSHOT="$GENESIS_DIR/00000000000000000000.lxs"
REGISTRATION_REQUEST="$GENESIS_DIR/paxeer-registration-request.lxrr"
for artifact in "$MANIFEST" "$SNAPSHOT" "$REGISTRATION_REQUEST" "$GENESIS_DIR/paxeer-deployment-descriptor.lxgd"; do
    [ -s "$artifact" ] || fail "genesis artifact missing: $artifact"
done
[ "$(stat -c %s "$REGISTRATION_REQUEST")" -eq 73 ] || fail "registration request has an unexpected length"
GENESIS_STATE_ROOT=$(tail -c +10 "$REGISTRATION_REQUEST" | head -c 32 | bin_to_hex)
GENESIS_RECEIPT_STATE_ROOT=$(tail -c 32 "$REGISTRATION_REQUEST" | bin_to_hex)

# Bootstrap registration (LXGR v1): the beta anchors genesis to its own
# receipt state root, the same self-registration the conformance node performs.
REGISTRATION="$GENESIS_DIR/genesis.registration"
{
    printf 'LXGR'
    hex_to_bin 01
    hex_to_bin "$(be_hex "$NETWORK_ID" 4)"
    hex_to_bin "$(be_hex 0 8)"
    hex_to_bin "$GENESIS_RECEIPT_STATE_ROOT"
    hex_to_bin "$GENESIS_RECEIPT_STATE_ROOT"
    hex_to_bin 01
} > "$REGISTRATION"
[ "$(stat -c %s "$REGISTRATION")" -eq 82 ] || fail "bootstrap registration has an unexpected length"

# --- identities, tokens, configurations ------------------------------------
IDENTITIES="$DATA_DIR/identities.txt"
printf '%s:%s:0\n' "$TREASURY_DID_HEX" "$TREASURY_PUBLIC" > "$IDENTITIES"

for logfile in logs/program-feed.log logs/canonical.log logs/receipt-authority.log logs/batch.log logs/evidence.log replica/receipt-authority.log; do
    : > "$DATA_DIR/$logfile"
    chmod 0600 "$DATA_DIR/$logfile"
done

printf '%s' "$PROGRAM_TOKEN" > "$DATA_DIR/secrets/program-token"
printf '%s' "$REPLICA_TOKEN" > "$DATA_DIR/secrets/replica-token"
printf '%s' "$TREASURY_PRIVATE" > "$DATA_DIR/secrets/treasury-key.hex"

write_config() {
    printf 'role=%s\nnetwork_id=%s\nstart_sequence=0\nverify_workers=2\nnetwork_workers=2\nprojection_workers=2\ncheckpoint_workers=1\nserial_execution=false\n' "$1" "$NETWORK_ID" > "$2"
}
write_config sequencer "$DATA_DIR/sequencer.conf"
write_config replica "$DATA_DIR/replica.conf"
"$LAYERXD" --check-config "$DATA_DIR/sequencer.conf" || fail "layerxd refused the sequencer configuration"
"$LAYERXD" --check-config "$DATA_DIR/replica.conf" || fail "layerxd refused the replica configuration"

LAST_BATCH=18446744073709551615
cat > "$DATA_DIR/sequencer.env" <<EOF
LAYERX_NODE_CHECKPOINT_DIRECTORY=$DATA_DIR/checkpoints
LAYERX_NODE_SNAPSHOT=$SNAPSHOT
LAYERX_NODE_GENESIS_MANIFEST=$MANIFEST
LAYERX_NODE_GENESIS_REGISTRATION=$REGISTRATION
LAYERX_NODE_IDENTITIES=$IDENTITIES
LAYERX_NODE_PROGRAM_FEED_LOG=$DATA_DIR/logs/program-feed.log
LAYERX_NODE_CANONICAL_LOG=$DATA_DIR/logs/canonical.log
LAYERX_NODE_RECEIPT_AUTHORITY_LOG=$DATA_DIR/logs/receipt-authority.log
LAYERX_NODE_BATCH_LOG=$DATA_DIR/logs/batch.log
LAYERX_NODE_EVIDENCE_LOG=$DATA_DIR/logs/evidence.log
LAYERX_NODE_HISTORY_DATABASE=$DATA_DIR/history.sqlite
LAYERX_NODE_HISTORY_MIGRATIONS=$MIGRATIONS
LAYERX_NODE_SEQUENCER_ID=$SEQUENCER_ID
LAYERX_NODE_SEQUENCER_PUBLIC_KEY=$SEQUENCER_PUBLIC
LAYERX_NODE_SEQUENCER_PRIVATE_KEY=$SEQUENCER_PRIVATE
LAYERX_NODE_FIRST_BATCH=1
LAYERX_NODE_LAST_BATCH=$LAST_BATCH
LAYERX_NODE_AUTHORITY_REPLICA_ADDRESS=127.0.0.1
LAYERX_NODE_AUTHORITY_REPLICA_PORT=$REPLICA_PORT
LAYERX_NODE_AUTHORITY_REPLICA_ID=$REPLICA_ID
LAYERX_NODE_AUTHORITY_REPLICA_BEARER_TOKEN=$REPLICA_TOKEN
LAYERX_NODE_PROGRAM_ADDRESS=127.0.0.1
LAYERX_NODE_PROGRAM_PORT=$PROGRAM_PORT
LAYERX_NODE_PROGRAM_BEARER_TOKEN=$PROGRAM_TOKEN
LAYERX_NODE_LNI_SOCKET=$LNI_SOCKET
LAYERX_NODE_LNI_ALLOWED_UID=$LNI_UID
LAYERX_NODE_LNI_ALLOWED_GID=$LNI_GID
LAYERX_NODE_LNI_FRAME_BYTES=1146902
LAYERX_NODE_LNI_DEADLINE_MS=2000
EOF

cat > "$DATA_DIR/replica.env" <<EOF
LAYERX_AUTHORITY_REPLICA_LOG=$DATA_DIR/replica/receipt-authority.log
LAYERX_AUTHORITY_REPLICA_ID=$REPLICA_ID
LAYERX_AUTHORITY_SEQUENCER_ID=$SEQUENCER_ID
LAYERX_AUTHORITY_SEQUENCER_PUBLIC_KEY=$SEQUENCER_PUBLIC
LAYERX_AUTHORITY_FIRST_BATCH=1
LAYERX_AUTHORITY_LAST_BATCH=$LAST_BATCH
LAYERX_AUTHORITY_BEARER_TOKEN=$REPLICA_TOKEN
LAYERX_AUTHORITY_ADDRESS=127.0.0.1
LAYERX_AUTHORITY_PORT=$REPLICA_PORT
EOF

umask 022
cat > "$DATA_DIR/node.env" <<EOF
LAYERX_NODE_NETWORK_ID=$NETWORK_ID
LAYERX_NODE_ASSET_ID=$ASSET_ID
LAYERX_NODE_LNI_SOCKET=$LNI_SOCKET
LAYERX_NODE_SUPERVISOR_SOCKET=$SUPERVISOR_SOCKET
LAYERX_NODE_PROGRAM_URL=http://127.0.0.1:$PROGRAM_PORT
LAYERX_NODE_REPLICA_URL=http://127.0.0.1:$REPLICA_PORT
LAYERX_NODE_PROGRAM_BEARER_TOKEN_FILE=$DATA_DIR/secrets/program-token
LAYERX_NODE_REPLICA_BEARER_TOKEN_FILE=$DATA_DIR/secrets/replica-token
LAYERX_NODE_SEQUENCER_ID=$SEQUENCER_ID
LAYERX_NODE_SEQUENCER_PUBLIC_KEY=$SEQUENCER_PUBLIC
LAYERX_NODE_REPLICA_ID=$REPLICA_ID
LAYERX_NODE_GENESIS_STATE_ROOT=$GENESIS_STATE_ROOT
LAYERX_NODE_GENESIS_RECEIPT_STATE_ROOT=$GENESIS_RECEIPT_STATE_ROOT
LAYERX_NODE_TREASURY_DID=$TREASURY_DID
LAYERX_NODE_TREASURY_PUBLIC_KEY=$TREASURY_PUBLIC
LAYERX_NODE_TREASURY_ACCOUNT=$TREASURY_ACCOUNT
LAYERX_NODE_TREASURY_BALANCE=$TREASURY_BALANCE
LAYERX_NODE_TREASURY_KEY_FILE=$DATA_DIR/secrets/treasury-key.hex
LAYERX_NODE_SEQUENCER_CONFIG=$DATA_DIR/sequencer.conf
LAYERX_NODE_REPLICA_CONFIG=$DATA_DIR/replica.conf
LAYERX_NODE_SEQUENCER_ENV=$DATA_DIR/sequencer.env
LAYERX_NODE_REPLICA_ENV=$DATA_DIR/replica.env
EOF
chmod 0644 "$DATA_DIR/node.env"

cat > "$DATA_DIR/treasury.json" <<EOF
{"did":"$TREASURY_DID","public_key":"$TREASURY_PUBLIC","account":"$TREASURY_ACCOUNT","asset":"$ASSET_ID","genesis_balance":"$TREASURY_BALANCE","network_id":$NETWORK_ID}
EOF
chmod 0644 "$DATA_DIR/treasury.json"
rm -rf "$DATA_DIR/work"

printf 'bootstrap: network %s genesis state root %s\n' "$NETWORK_ID" "$GENESIS_STATE_ROOT"
printf 'bootstrap: treasury %s\n' "$TREASURY_ACCOUNT"
printf 'bootstrap: data %s run %s\n' "$DATA_DIR" "$RUN_DIR"
