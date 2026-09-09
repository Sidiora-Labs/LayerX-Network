#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/../../../.." && pwd)
cd "$ROOT"
. "$ROOT/platform/hosted/tests/beta-cluster.sh"
WORK_DIR=$(mktemp -d "${TMPDIR:-/tmp}/guarantor-provision.XXXXXX")
CA_DIR="$WORK_DIR/ca"
SECRETS_DIR="$WORK_DIR/secrets"
mkdir -p "$WORK_DIR/authority"
authority_key="$WORK_DIR/authority/checkpoint-authority.pem"
authority_command=(bash "$ROOT/platform/hosted/node/guarantor.sh" --checkpoint-authority-public "$authority_key")
"${authority_command[@]}" > "$WORK_DIR/authority/public.hex" &
authority_first=$!
"${authority_command[@]}" > "$WORK_DIR/authority/concurrent.hex" &
authority_second=$!
wait "$authority_first"
wait "$authority_second"
cmp "$WORK_DIR/authority/public.hex" "$WORK_DIR/authority/concurrent.hex"
[[ $(cat "$WORK_DIR/authority/public.hex") =~ ^0x[0-9a-f]{64}$ ]]
[ "$(stat -c %a "$authority_key")" = 600 ]
authority_inode=$(stat -c %i "$authority_key")
"${authority_command[@]}" > "$WORK_DIR/authority/restart.hex"
cmp "$WORK_DIR/authority/public.hex" "$WORK_DIR/authority/restart.hex"
[ "$authority_inode" = "$(stat -c %i "$authority_key")" ]
printf 'LX:PAXEER:DEPOSIT:ROOT:v1' > "$WORK_DIR/authority/message"
openssl pkey -in "$authority_key" -pubout -out "$WORK_DIR/authority/public.pem"
openssl pkeyutl -sign -rawin -inkey "$authority_key" -in "$WORK_DIR/authority/message" -out "$WORK_DIR/authority/signature"
openssl pkeyutl -verify -rawin -pubin -inkey "$WORK_DIR/authority/public.pem" -in "$WORK_DIR/authority/message" -sigfile "$WORK_DIR/authority/signature"
python3 - "$WORK_DIR/authority" <<'PY'
import pathlib
import subprocess
import sys
directory = pathlib.Path(sys.argv[1])
der = subprocess.check_output(['openssl', 'pkey', '-pubin', '-in', str(directory / 'public.pem'), '-outform', 'DER'])
assert (directory / 'public.hex').read_text().strip() == '0x' + der[12:].hex()
PY
chmod 0640 "$authority_key"
if "${authority_command[@]}" > "$WORK_DIR/authority/refused.stdout" 2> "$WORK_DIR/authority/refused.stderr"; then
    fail "checkpoint authority accepted group-readable private key"
fi
[ ! -s "$WORK_DIR/authority/refused.stdout" ]
[ "$(stat -c %a "$authority_key")" = 640 ]
chmod 0600 "$authority_key"
ln -s "$authority_key" "$WORK_DIR/authority/linked.pem"
if bash "$ROOT/platform/hosted/node/guarantor.sh" --checkpoint-authority-public "$WORK_DIR/authority/linked.pem" > "$WORK_DIR/authority/symlink.stdout" 2> "$WORK_DIR/authority/symlink.stderr"; then
    fail "checkpoint authority accepted symlink"
fi
[ ! -s "$WORK_DIR/authority/symlink.stdout" ]
printf 'guarantor-provision: Ed25519 authority generation, concurrent reuse, restart, signing and unsafe-file refusals passed\n'
NATIVE_BIN=${LAYERX_TEST_NATIVE_BIN_DIR:-$ROOT/build/bin}
ca_generate
for identity in 1 2; do
    openssl verify -CAfile "$CA_DIR/ca.crt" -purpose sslclient "$CA_DIR/guarantor-$identity/cert.pem"
    openssl verify -CAfile "$CA_DIR/ca.crt" -purpose sslserver -verify_ip 127.0.0.1 "$CA_DIR/guarantor-$identity/cert.pem"
done
if cmp -s "$CA_DIR/guarantor-1/cert.pem" "$CA_DIR/guarantor-2/cert.pem"; then
    fail "guarantor TLS identities must differ"
fi
if openssl verify -CAfile "$CA_DIR/ca.crt" -purpose sslserver -verify_ip 127.0.0.2 "$CA_DIR/guarantor-1/cert.pem" > "$WORK_DIR/wrong-peer.log" 2>&1; then
    fail "wrong TLS peer IP was accepted"
fi
printf 'guarantor-provision: both mTLS identities and wrong-peer refusal passed\n'
[ -x "$NATIVE_BIN/layerxd" ] && [ -x "$NATIVE_BIN/layerx-genesis-build" ] || fail "native bootstrap binaries missing at $NATIVE_BIN"
(umask 077; openssl rand 32 > "$SECRETS_DIR/treasury.key")
client_uid=4021
[ "$(id -u)" != "$client_uid" ] || client_uid=4022
bootstrap=(bash "$ROOT/platform/hosted/node/bootstrap.sh" --data-dir "$WORK_DIR/node" --run-dir "$WORK_DIR/run" \
    --network-id 4242 --sequencer-key "$CA_DIR/sequencer.seed.hex" --treasury-key "$SECRETS_DIR/treasury.key" \
    --lni-uid "$client_uid" --lni-gid "$(id -g)" --layerxd "$NATIVE_BIN/layerxd" \
    --genesis-build "$NATIVE_BIN/layerx-genesis-build" --settlement-env "$WORK_DIR/settlement.env")
"${bootstrap[@]}"
. "$WORK_DIR/node/node.env"
[ "$LAYERX_NODE_GENESIS_GUARANTOR_ID" != "$LAYERX_NODE_SECOND_GUARANTOR_ID" ]
for identity in 1 2; do
    dir="$WORK_DIR/guarantor-$identity"
    [ "$(stat -c %a "$dir/identity/key.pem")" = 440 ]
    [ "$(stat -c %a "$dir/state")" = 2770 ]
    [ "$(stat -c %g "$dir/state")" = "$(id -g)" ]
    openssl pkey -in "$dir/identity/key.pem" -check -noout > /dev/null
    cmp "$dir/identity/genesis.lxs" "$WORK_DIR/node/genesis/00000000000000000000.lxs"
    cmp "$dir/identity/genesis.manifest" "$WORK_DIR/node/genesis/genesis.manifest"
done
printf 'guarantor-provision: two real genesis guarantors and isolated bootstrap artifacts passed\n'
generation=$(stat -c %i "$WORK_DIR/guarantor-1/identity/producer.env")
first_identity=$LAYERX_NODE_GENESIS_GUARANTOR_ID
"${bootstrap[@]}" --force
. "$WORK_DIR/node/node.env"
[ "$first_identity" != "$LAYERX_NODE_GENESIS_GUARANTOR_ID" ]
[ "$generation" != "$(stat -c %i "$WORK_DIR/guarantor-1/identity/producer.env")" ]
[ "$(stat -c %a "$WORK_DIR/guarantor-1/identity/key.pem")" = 440 ]
printf 'guarantor-provision: reset atomically rotates identity and preserves protected artifact modes\n'
