#!/usr/bin/env bash

human_secrets_generate() (
    set -euo pipefail
    umask 077
    local root="$SECRETS_DIR/human" token
    mkdir -p "$root/components" "$root/kms" "$root/agent" "$root/config" "$root/agent-config"
    chmod 0700 "$root" "$root"/*
    issue_cert human-kms layerx-human-kms serverAuth 'DNS:layerx-human-kms,IP:127.0.0.1'
    issue_cert human-kms-client layerx-human-components clientAuth ''
    for token in authority-token program-authority-token program-token session-operator; do
        write_token "$root/agent/$token"
        printf '%s' "$(cat "$root/agent/$token")" > "$root/agent/$token.next"
        mv "$root/agent/$token.next" "$root/agent/$token"
    done
    cp "$SECRETS_DIR/node-program.token" "$root/agent/node-token"
    cp "$SECRETS_DIR/trust-history" "$root/agent/trust-history"
    cp "$CA_DIR/ca.der" "$root/components/ca.der"
    cp "$CA_DIR/human-kms-client/cert.der" "$root/components/kms-client.der"
    cp "$CA_DIR/human-kms-client/key.der" "$root/components/kms-client-key.der"
    cp "$CA_DIR/ca.der" "$root/kms/ca.der"
    cp "$CA_DIR/human-kms-client/cert.der" "$root/kms/kms-client.der"
    cp "$CA_DIR/human-kms/cert.der" "$root/kms/kms-server.der"
    cp "$CA_DIR/human-kms/key.der" "$root/kms/kms-server-key.der"
    openssl rand 32 > "$root/kms/kms-seal"
    python3 "$REPO_ROOT/platform/hosted/human/material.py" "$root" "$NODE_NETWORK_ID" \
        "$PAXEER_CHAIN_ID" "${LAYERX_BETA_HUMAN_POLICY_FILE:-}"
)

human_secrets_apply() {
    local root="$SECRETS_DIR/human"
    apply_secret "$TESTNET_NAMESPACE" layerx-human-component-material --from-file="$root/components"
    apply_secret "$TESTNET_NAMESPACE" layerx-human-kms-material --from-file="$root/kms"
    apply_secret "$TESTNET_NAMESPACE" layerx-human-agent-material --from-file="$root/agent"
    apply_secret "$TESTNET_NAMESPACE" layerx-human-components-config --from-file="$root/config"
    apply_secret "$TESTNET_NAMESPACE" layerx-human-agent-journal --from-file="$root/journal"
    apply_secret "$TESTNET_NAMESPACE" layerx-human-agent-config --from-file="$root/agent-config"
    if [ -z "${LAYERX_BETA_HUMAN_POLICY_FILE:-}" ]; then
        MISSING_INPUTS+=("LAYERX_BETA_HUMAN_POLICY_FILE: registered Human authority/owner/recovery policy, verified Programs probe and journal, live module registry and deployed custody contract bindings; see platform/hosted/human/README.md")
    fi
}
