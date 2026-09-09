#!/usr/bin/env bash

human_owner_provision() (
    set -euo pipefail
    umask 077
    local input="$WORK_DIR/human-evidence-input" output="$WORK_DIR/human-owner-result.json"
    local manifest="$WORK_DIR/human-provision-owner-job.json" state="$WORK_DIR/human-provision-node.json"
    [ ! -e "$output" ] || fail "$output: existing owner result requires explicit reconciliation with retained state"
    python3 "$REPO_ROOT/platform/hosted/human/provision.py" --validate-job-input --work-dir "$WORK_DIR"
    python3 "$REPO_ROOT/platform/hosted/human/provision.py" --account-requests --work-dir "$WORK_DIR"
    kube -n "$TESTNET_NAMESPACE" get statefulset layerx-node -o json > "$state"
    python3 - "$state" <<'PY'
import json
import sys
value = json.load(open(sys.argv[1]))
containers = value['spec']['template']['spec']['containers']
if any(c['name'].startswith('human') for c in containers):
    raise SystemExit('layerx-node: Human runtime must not be enabled during owner provisioning')
PY
    kube -n "$TESTNET_NAMESPACE" get pods -l app=layerx-node -o json > "$state"
    python3 - "$state" <<'PY'
import json
import sys
value = json.load(open(sys.argv[1]))
if len(value['items']) != 1:
    raise SystemExit('layerx-node: exactly one bootstrap pod is required')
pod = value['items'][0]
if any(c['name'].startswith('human') for c in pod['spec']['containers']):
    raise SystemExit('layerx-node: Human runtime pod must be stopped before owner provisioning')
if not pod['spec'].get('nodeName'):
    raise SystemExit('layerx-node: bootstrap pod is not scheduled')
PY
    python3 - "$REPO_ROOT/platform/hosted/human/provision-owner-job.yaml" "$manifest" \
        "$TESTNET_NAMESPACE" "$(image_ref layerx-human)" "$state" <<'PY'
import json
import sys
import yaml
value = yaml.safe_load(open(sys.argv[1]))
value['metadata']['namespace'] = sys.argv[3]
pod = value['spec']['template']['spec']
for container in pod['containers'] + pod['initContainers']:
    container['image'] = sys.argv[4]
pod['nodeName'] = json.load(open(sys.argv[5]))['items'][0]['spec']['nodeName']
with open(sys.argv[2], 'x') as output:
    json.dump(value, output)
PY
    apply_secret "$TESTNET_NAMESPACE" layerx-human-provision-owner-input \
        --from-file=owner-request.json="$input/owner-request.json" \
        --from-file=recovery-policy.json="$input/recovery-policy.json" \
        --from-file=treasury-request.json="$input/treasury-request.json" \
        --from-file=sequencer-request.json="$input/sequencer-request.json" \
        --from-file=account-head-request.json="$input/account-head-request.json"
    kube create -f "$manifest" > /dev/null
    kube -n "$TESTNET_NAMESPACE" wait --for=condition=complete --timeout=150s job/layerx-human-provision-owner > /dev/null \
        || fail 'layerx-human-provision-owner: Job did not complete; owner result not published'
    kube -n "$TESTNET_NAMESPACE" logs job/layerx-human-provision-owner -c provision-owner > "$output.pending"
    python3 "$REPO_ROOT/platform/hosted/human/provision.py" --validate-owner-result \
        --work-dir "$WORK_DIR" --request "$output.pending"
    mv "$output.pending" "$output"
    kube -n "$TESTNET_NAMESPACE" logs job/layerx-human-provision-owner -c validate-account-head > "$input/account-head-result.json"
    local account
    for account in treasury sequencer; do
        kube -n "$TESTNET_NAMESPACE" logs job/layerx-human-provision-owner -c "provision-$account" > "$input/$account.json"
    done
)

human_evidence_provision() (
    set -euo pipefail
    umask 077
    local input="$WORK_DIR/human-evidence-input" status
    local provision="$REPO_ROOT/platform/hosted/human/provision.py"
    [ -d "$input" ] && [ ! -L "$input" ] || fail "$input: owner registration producer inputs required"
    python3 "$provision" --validate-owner-registration --work-dir "$WORK_DIR"
    python3 "$provision" --validate-job-input --work-dir "$WORK_DIR"
    [ -n "${LAYERX_REGISTRY_JOURNAL:-}" ] || fail 'LAYERX_REGISTRY_JOURNAL: registry admission/deployment pairs required; deployment_proof_unavailable is not evidence'
    kube -n "$TESTNET_NAMESPACE" get secret layerx-guarantor-checkpoint-authority \
        -o 'jsonpath={.data.public\.hex}' > "$input/checkpoint-public.base64" \
        || fail 'Secret layerx-guarantor-checkpoint-authority/public.hex: checkpoint producer output required'
    python3 "$provision" --movement-source --work-dir "$WORK_DIR" --secrets-dir "$SECRETS_DIR"
    port_forward human-account-head "$TESTNET_NAMESPACE" layerx-agent-boundary 19454 9443
    status=$(curl --silent --show-error --max-time 30 --max-filesize 1048576 \
        --cacert "$CA_DIR/ca.crt" --header "Authorization: Bearer $(cat "$SECRETS_DIR/registry-node.token")" \
        --output "$input/account-head.json" --write-out '%{http_code}' \
        'https://localhost:19454/v1/protocol/account-state/head')
    [ "$status" = 200 ] || fail "$input/account-head.json: agent boundary refused account-state head with status $status"
    python3 - "$provision" "$input" "$NODE_NETWORK_ID" "$NODE_SEQUENCER_ID" "$NODE_SEQUENCER_PUBLIC_KEY" <<'PYHEAD'
import importlib.util
from pathlib import Path
import sys
spec = importlib.util.spec_from_file_location('provision', sys.argv[1])
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
root = Path(sys.argv[2])
module.write_json(root / 'account-head-request.json', {
    'head': module.protected_json(root / 'account-head.json'), 'network_id': int(sys.argv[3]),
    'sequencer_id': sys.argv[4], 'public_key': sys.argv[5]})
PYHEAD
    human_owner_provision
    python3 "$provision" --prepare-owner-admission --work-dir "$WORK_DIR" --secrets-dir "$SECRETS_DIR"
    human_custody_step deposit
    python3 "$provision" --validate-owner-registration --work-dir "$WORK_DIR"
    python3 "$provision" --validate-evidence-inputs --work-dir "$WORK_DIR" \
        --registry "$SECRETS_DIR/module-registry.json" --journal "$LAYERX_REGISTRY_JOURNAL"
    python3 "$provision" --assemble --work-dir "$WORK_DIR" \
        --registry "$SECRETS_DIR/module-registry.json" --asset "$NODE_ASSET_ID" \
        --journal "$LAYERX_REGISTRY_JOURNAL"
)

human_custody_step() (
    set -euo pipefail
    umask 077
    export PATH="$FOUNDRY_BIN:$PATH"
    [ "$PAXEER_URL" = 'https://localhost:19449' ] && [ "$PAXEER_OBSERVER_URL" = 'https://localhost:19452' ] \
        || fail 'owner custody requires the disposable in-cluster Paxeer port forwards'
    python3 "$REPO_ROOT/platform/hosted/human/owner_custody.py" "$1" \
        --work-dir "$WORK_DIR" --rpc "$PAXEER_URL" --rpc "$PAXEER_OBSERVER_URL" \
        --ca-bundle "$CA_DIR/ca.pem" --disposable-identity "$WORK_DIR/paxeer/rpc-origins.json" \
        --key-file "$SECRETS_DIR/paxeer-deployer.key" --attestor-key "$SECRETS_DIR/custody-attestor.seed" \
        --network-id "$NODE_NETWORK_ID" --asset "$NODE_ASSET_ID"
)
