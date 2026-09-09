#!/usr/bin/env bash

human_secrets_generate() (
    set -euo pipefail
    umask 077
    local root="$SECRETS_DIR/human" token
    mkdir -p "$root/components" "$root/kms" "$root/agent" "$root/config" "$root/agent-config" "$root/identity" "$root/security" \
        "$root/movement" "$root/movement-config" "$root/authority" "$root/authority-config"
    chmod 0700 "$root" "$root"/*
    issue_cert human-kms layerx-human-kms serverAuth 'DNS:layerx-human-kms,IP:127.0.0.1'
    issue_cert human-kms-client layerx-human-components clientAuth ''
    issue_cert human-kms-executor layerx-human-movement clientAuth ''
    for token in authority-token program-authority-token program-token session-operator; do
        write_token "$root/agent/$token"
        printf '%s' "$(cat "$root/agent/$token")" > "$root/agent/$token.next"
        mv "$root/agent/$token.next" "$root/agent/$token"
    done
    cp "$SECRETS_DIR/node-program.token" "$root/agent/node-token"
    cp "$SECRETS_DIR/trust-history" "$root/agent/trust-history"
    cp "$root/agent/authority-token" "$root/authority/authority-token"
    cp "$SECRETS_DIR/trust-history" "$root/security/trust-history"
    cp "$CA_DIR/ca.der" "$root/agent/ca.der"
    cp "$CA_DIR/ca.der" "$root/movement/ca.der"
    cp "$CA_DIR/human-kms-executor/cert.der" "$root/movement/kms-executor.der"
    cp "$CA_DIR/human-kms-executor/key.der" "$root/movement/kms-executor-key.der"
    cp "$CA_DIR/human-kms-executor/cert.der" "$root/kms/kms-executor.der"
    cp "$CA_DIR/ca.der" "$root/components/ca.der"
    cp "$CA_DIR/human-kms-client/cert.der" "$root/components/kms-client.der"
    cp "$CA_DIR/human-kms-client/key.der" "$root/components/kms-client-key.der"
    cp "$CA_DIR/ca.der" "$root/kms/ca.der"
    cp "$CA_DIR/human-kms-client/cert.der" "$root/kms/kms-client.der"
    cp "$CA_DIR/human-kms/cert.der" "$root/kms/kms-server.der"
    cp "$CA_DIR/human-kms/key.der" "$root/kms/kms-server-key.der"
    openssl rand 32 > "$root/kms/kms-seal"
)

human_secrets_apply() {
    local root="$SECRETS_DIR/human"
    apply_secret "$TESTNET_NAMESPACE" layerx-human-component-material --from-file="$root/components"
    apply_secret "$TESTNET_NAMESPACE" layerx-human-kms-material --from-file="$root/kms"
    apply_secret "$TESTNET_NAMESPACE" layerx-human-agent-material --from-file="$root/agent"
    apply_secret "$TESTNET_NAMESPACE" layerx-human-components-config --from-file="$root/config"
    apply_secret "$TESTNET_NAMESPACE" layerx-human-agent-journal --from-file="$root/journal"
    apply_secret "$TESTNET_NAMESPACE" layerx-human-agent-config --from-file="$root/agent-config"
    local role
    for role in identity security movement authority; do
        apply_secret "$TESTNET_NAMESPACE" "layerx-human-$role-material" --from-file="$root/$role"
    done
    for role in movement authority; do
        apply_secret "$TESTNET_NAMESPACE" "layerx-human-$role-config" --from-file="$root/$role-config"
    done
}


human_policy_publish() {
    local evidence="$WORK_DIR/human-evidence"
    LAYERX_BETA_HUMAN_POLICY_FILE="$WORK_DIR/human-policy.json"
    python3 "$REPO_ROOT/platform/hosted/human/material.py" --assemble \
        "$evidence" "$WORK_DIR/paxeer/deployment.json" "$SECRETS_DIR/module-registry.json" \
        "$LAYERX_BETA_HUMAN_POLICY_FILE" "$NODE_NETWORK_ID" "$PAXEER_CHAIN_ID"
    python3 "$REPO_ROOT/platform/hosted/human/material.py" "$SECRETS_DIR/human" \
        "$NODE_NETWORK_ID" "$PAXEER_CHAIN_ID" "$LAYERX_BETA_HUMAN_POLICY_FILE"
    human_secrets_apply
}
