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

retained_material_inventory() {
    python3 - "$CA_DIR" "$SECRETS_DIR" "$1" <<'PY'
import json
import os
from pathlib import Path
import stat
import sys
ca, secrets = map(Path, sys.argv[1:3])
manifest = secrets / 'retained-material.json'

def refuse(detail):
    raise SystemExit('beta-cluster: error: retained material refused: ' + detail)

def entries():
    result = {}
    for root in (ca, secrets):
        if root.is_symlink() or not root.is_dir():
            refuse('missing directory ' + str(root))
        for path in [root, *sorted(root.rglob('*'))]:
            if path == manifest:
                continue
            info = path.lstat()
            if not (stat.S_ISDIR(info.st_mode) or stat.S_ISREG(info.st_mode)):
                refuse('non-regular entry ' + str(path))
            if info.st_uid != os.geteuid() or (stat.S_ISREG(info.st_mode) and info.st_nlink != 1):
                refuse('ownership or link count ' + str(path))
            expected = 0o700 if path.is_dir() else 0o600
            if stat.S_IMODE(info.st_mode) != expected:
                refuse('expected mode ' + oct(expected) + ' for ' + str(path))
            result[root.name + '/' + str(path.relative_to(root))] = expected
    return result

if sys.argv[3] == 'save':
    for root in (ca, secrets):
        for path in [root, *root.rglob('*')]:
            if path.is_symlink() or not (path.is_dir() or path.is_file()):
                refuse('non-regular entry ' + str(path))
            path.chmod(0o700 if path.is_dir() else 0o600)
    value = entries()
    with manifest.open('w') as output:
        os.chmod(manifest, 0o600)
        json.dump(value, output, sort_keys=True)
else:
    if manifest.is_symlink() or not manifest.is_file():
        refuse('missing inventory ' + str(manifest))
    info = manifest.stat()
    if stat.S_IMODE(info.st_mode) != 0o600 or info.st_uid != os.geteuid() or info.st_nlink != 1:
        refuse('inventory ownership or mode')
    if info.st_size > 1048576:
        refuse('inventory exceeds bound')
    try:
        expected = json.loads(manifest.read_text())
    except (ValueError, OSError):
        refuse('invalid inventory')
    actual = entries()
    if actual != expected:
        missing = sorted(set(expected) - set(actual))
        refuse('incomplete or changed file set' + (': ' + missing[0] if missing else ''))
    for name in ('ca/ca.key', 'ca/ca.crt', 'ca/ca.der',
                 'secrets/module-registry.json', 'secrets/human/kms/kms-seal',
                 'secrets/human/kms/registry.json', 'secrets/retained-context.json'):
        if name not in actual:
            refuse('missing required file ' + name)
    if (secrets / 'human/kms/kms-seal').stat().st_size != 32:
        refuse('kms-seal must contain exactly 32 bytes')
    for name in ('config', 'agent-config', 'movement-config', 'authority-config', 'journal'):
        directory = secrets / 'human' / name
        if not directory.is_dir() or not any(directory.iterdir()):
            refuse('empty Human directory ' + name)
PY
}

retained_material_live_check() {
    kube -n "$TESTNET_NAMESPACE" get secret layerx-human-kms-material --ignore-not-found -o json \
        | python3 -c '
import base64, hashlib, json, pathlib, sys
raw = sys.stdin.buffer.read()
if not raw.strip():
    sys.exit(0)
try:
    seal = base64.b64decode(json.loads(raw)["data"]["kms-seal"], validate=True)
    local = pathlib.Path(sys.argv[1]).read_bytes()
    if len(seal) != 32 or len(local) != 32 or hashlib.sha256(seal).digest() != hashlib.sha256(local).digest():
        raise ValueError()
except (ValueError, KeyError, OSError):
    sys.exit("beta-cluster: error: retained material refused: live layerx-human-kms-material kms-seal digest differs from disk")
' "$SECRETS_DIR/human/kms/kms-seal" \
        || fail "retained material refused: live KMS seal verification failed"
}
