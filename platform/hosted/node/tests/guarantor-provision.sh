#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/../../../.." && pwd)
cd "$ROOT"
. "$ROOT/platform/hosted/tests/beta-cluster.sh"
mkdir -p "$ROOT/qual-logs/gp1"
WORK_DIR=$(mktemp -d "$ROOT/qual-logs/gp1/guarantor-provision.XXXXXX")
CA_DIR="$WORK_DIR/ca"
SECRETS_DIR="$WORK_DIR/secrets"
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
