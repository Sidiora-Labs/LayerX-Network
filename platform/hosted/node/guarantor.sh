#!/usr/bin/env bash
set -euo pipefail
if [ "${1:-}" = --checkpoint-authority-public ]; then
    [ "$#" = 2 ] || exit 2
    exec python3 "$(dirname "$0")/checkpoint-authority.py" "$2"
fi
: "${LAYERX_GUARANTOR_IDENTITY_DIR:?identity directory is required}"
: "${LAYERX_GUARANTOR_LNI_SOCKET:?LNI socket is required}"
: "${LAYERX_GUARANTOR_STATE_DIR:?state directory is required}"
: "${LAYERX_GUARANTOR_SETTLEMENT_ENV:?settlement environment is required}"
: "${LAYERX_GUARANTOR_SUBMITTER_KEY_FILE:?submitter key is required}"
state_root=$LAYERX_GUARANTOR_STATE_DIR
submitter_source=$LAYERX_GUARANTOR_SUBMITTER_KEY_FILE
child=""
stop_child() {
    if [ -n "$child" ]; then
        kill -TERM "$child" 2>/dev/null || true
        wait "$child" 2>/dev/null || true
        child=""
    fi
}
trap 'stop_child; exit 0' TERM INT
trap stop_child EXIT
while :; do
    while [ ! -r "$LAYERX_GUARANTOR_IDENTITY_DIR/producer.env" ] || \
          [ ! -r "$LAYERX_GUARANTOR_SETTLEMENT_ENV" ] || \
          [ ! -S "$LAYERX_GUARANTOR_LNI_SOCKET" ]; do
        sleep 1
    done
    generation=$(stat -c %i "$LAYERX_GUARANTOR_IDENTITY_DIR/producer.env")
    settlement=$("$(dirname "$0")/bootstrap.sh" --check-settlement "$LAYERX_GUARANTOR_SETTLEMENT_ENV")
    set -a
    . "$LAYERX_GUARANTOR_IDENTITY_DIR/producer.env"
    eval "$settlement"
    set +a
    [[ $LAYERX_GUARANTOR_ID =~ ^[0-9a-f]{64}$ ]]
    export LAYERX_GUARANTOR_STATE_DIR="$state_root/$LAYERX_GUARANTOR_ID"
    export LAYERX_GUARANTOR_KEY_FILE="$LAYERX_GUARANTOR_IDENTITY_DIR/key.pem"
    export LAYERX_NODE_SNAPSHOT="$LAYERX_GUARANTOR_IDENTITY_DIR/genesis.lxs"
    export LAYERX_NODE_GENESIS_MANIFEST="$LAYERX_GUARANTOR_IDENTITY_DIR/genesis.manifest"
    export LAYERX_NODE_GENESIS_REGISTRATION="$LAYERX_GUARANTOR_IDENTITY_DIR/genesis.registration"
    export LAYERX_NODE_IDENTITIES="$LAYERX_GUARANTOR_IDENTITY_DIR/identities.txt"
    export LAYERX_GUARANTOR_NODE_CONFIG="$LAYERX_GUARANTOR_IDENTITY_DIR/node.conf"
    umask 077
    mkdir -p "$LAYERX_GUARANTOR_STATE_DIR/signer"
    chmod 0700 "$LAYERX_GUARANTOR_STATE_DIR/signer"
    install -m 0600 "$submitter_source" "$LAYERX_GUARANTOR_STATE_DIR/signer/submitter.key"
    export LAYERX_GUARANTOR_SUBMITTER_KEY_FILE="$LAYERX_GUARANTOR_STATE_DIR/signer/submitter.key"
    /usr/local/bin/layerx-guarantor &
    child=$!
    current=$generation
    while kill -0 "$child" 2>/dev/null; do
        current=$(stat -c %i "$LAYERX_GUARANTOR_IDENTITY_DIR/producer.env")
        [ "$current" = "$generation" ] || break
        sleep 1
    done
    if [ "$current" != "$generation" ]; then
        stop_child
        continue
    fi
    status=0
    wait "$child" || status=$?
    child=""
    exit "$status"
done
