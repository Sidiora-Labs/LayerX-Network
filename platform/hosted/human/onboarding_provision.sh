#!/usr/bin/env bash

human_kms_prepare() (
    set -euo pipefail
    umask 077
    local root="$SECRETS_DIR/human" input="$WORK_DIR/human-evidence-input"
    local current="$WORK_DIR/human-kms-native-state.json" manifest="$WORK_DIR/human-kms-bootstrap.json"
    local funding="${LAYERX_BETA_HUMAN_INITIAL_FUNDING:-1000000000000}" tenant
    tenant=$(python3 - "$WORK_DIR/identity/source-binding.json" <<'PY'
import json, sys
value = json.load(open(sys.argv[1]))
assert set(value) == {'tenant', 'principal'}
assert isinstance(value['tenant'], str) and value['tenant']
print(value['tenant'])
PY
)
    python3 - "$REPO_ROOT/platform/hosted/human" "$SECRETS_DIR/module-registry.json" "$root/kms/registry.json" <<'PY'
import sys
from pathlib import Path
sys.path.insert(0, sys.argv[1])
from provision import protected_json, write_json, require
registry = protected_json(sys.argv[2])
require(registry['schema_version'] == 2, sys.argv[2], 'canonical module registry')
modules = [dict(module_id=module['module'], activity_types=[(module['module'] << 16) | ordinal
    for ordinal in module['ordinals']]) for module in registry['modules']]
write_json(Path(sys.argv[3]), dict(network_id=registry['network_id'], protocol_version=3, modules=modules))
PY
    install -m 0600 "$input/recovery-policy.json" "$root/identity/recovery-policy.json"
    apply_secret "$TESTNET_NAMESPACE" layerx-human-component-material --from-file="$root/components"
    apply_secret "$TESTNET_NAMESPACE" layerx-human-kms-material --from-file="$root/kms"
    apply_secret "$TESTNET_NAMESPACE" layerx-human-identity-material --from-file="$root/identity"
    apply_secret "$TESTNET_NAMESPACE" layerx-human-onboarding-input \
        --from-file=owner-request.json="$input/owner-request.json" \
        --from-file=recovery-policy.json="$input/recovery-policy.json" \
        --from-file=module-registry.json="$SECRETS_DIR/module-registry.json"
    kube -n "$TESTNET_NAMESPACE" get statefulset layerx-node -o json > "$current"
    python3 "$REPO_ROOT/platform/hosted/human/onboarding_manifest.py" "$current" \
        "$REPO_ROOT/platform/hosted/node/deployment.yaml" "$manifest" "$(image_ref layerx-human)" \
        "$NODE_NETWORK_ID" "$PAXEER_CHAIN_ID" "$tenant" "$funding"
    kube apply -f "$manifest" > /dev/null
    kube -n "$TESTNET_NAMESPACE" rollout status statefulset/layerx-node --timeout=300s > /dev/null
    local name config="$SECRETS_DIR/human-onboarding-config"
    mkdir -m 0700 "$config"
    for name in TENANCY_DIGEST AUTH_INDEX_KEY STREAM_CURSOR_KEY; do
        kube -n "$TESTNET_NAMESPACE" exec layerx-node-0 -c human-onboarding -- \
            cat "/var/lib/layerx/human/onboarding-config/LAYERX_HUMAN_$name" \
            > "$config/LAYERX_HUMAN_$name"
    done
    for name in owner-kms.json owner-admission.json owner-admission.txt; do
        kube -n "$TESTNET_NAMESPACE" exec layerx-node-0 -c human-onboarding -- \
            cat "/var/lib/layerx/human/onboarding/human-evidence-input/$name" > "$input/$name"
    done
    python3 - "$REPO_ROOT/platform/hosted/human" "$WORK_DIR" "$config" "$funding" <<'PY'
import sys
from pathlib import Path
sys.path.insert(0, sys.argv[1])
from provision import protected_json, require, write_json
root, config = Path(sys.argv[2]), Path(sys.argv[3])
owner = protected_json(root / 'human-evidence-input/owner-kms.json')
previous = protected_json(root / 'human-owner-result.json')
require(previous == {key: owner[key] for key in previous}, root, 'same actual IdentityProvider principal and recovery')
write_json(root / 'human-evidence-input/onboarding-configuration.json', dict(directory=str(config),
    sponsor_principal=owner['principal'], initial_funding=int(sys.argv[4])))
PY
)
