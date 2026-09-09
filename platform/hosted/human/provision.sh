#!/usr/bin/env bash

human_owner_provision() (
    set -euo pipefail
    umask 077
    local input="$WORK_DIR/human-evidence-input" output="$WORK_DIR/human-owner-result.json"
    local manifest="$WORK_DIR/human-provision-owner-job.json" state="$WORK_DIR/human-provision-node.json"
    [ ! -e "$output" ] || fail "$output: existing owner result requires explicit reconciliation with retained state"
    python3 "$REPO_ROOT/platform/hosted/human/provision.py" --validate-job-input --work-dir "$WORK_DIR"
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
        --from-file=recovery-policy.json="$input/recovery-policy.json"
    kube create -f "$manifest" > /dev/null
    kube -n "$TESTNET_NAMESPACE" wait --for=condition=complete --timeout=150s job/layerx-human-provision-owner > /dev/null \
        || fail 'layerx-human-provision-owner: Job did not complete; owner result not published'
    kube -n "$TESTNET_NAMESPACE" logs job/layerx-human-provision-owner -c provision-owner > "$output.pending"
    python3 "$REPO_ROOT/platform/hosted/human/provision.py" --validate-owner-result \
        --work-dir "$WORK_DIR" --request "$output.pending"
    mv "$output.pending" "$output"
)
